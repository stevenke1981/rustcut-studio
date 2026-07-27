use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    CaptionPreset, EditCommand, EditPlan, Project, Result, RustCutError, TextAnimation,
    TextOverlay, TextPosition,
};

#[async_trait]
pub trait Planner: Send + Sync {
    async fn plan(&self, project: &Project, prompt: &str) -> Result<EditPlan>;
}

#[derive(Debug, Clone, Default)]
pub struct RulePlanner;

#[async_trait]
impl Planner for RulePlanner {
    async fn plan(&self, project: &Project, prompt: &str) -> Result<EditPlan> {
        let normalized = prompt.to_lowercase();
        let asset_id = resolve_asset_reference(project, prompt);
        let mut commands = Vec::new();
        let mut warnings = Vec::new();
        let mut actions = Vec::new();

        if contains_any(
            &normalized,
            &["匯入時間軸", "放到時間軸", "add to timeline"],
        ) && let Some(asset_id) = asset_id
        {
            commands.push(EditCommand::AddAssetToTimeline {
                asset_id,
                track_kind: None,
                at_ms: None,
            });
            actions.push("加入素材到時間軸");
        }

        if contains_any(
            &normalized,
            &[
                "移除靜音",
                "刪除靜音",
                "移除空白",
                "cut silence",
                "remove silence",
                "dead air",
            ],
        ) && let Some(asset_id) = asset_id
        {
            commands.push(EditCommand::RemoveSilence {
                asset_id,
                min_gap_ms: 650,
                keep_ms: 90,
            });
            actions.push("移除長靜音");
        }

        if contains_any(
            &normalized,
            &[
                "移除贅詞",
                "刪除贅詞",
                "口頭禪",
                "remove filler",
                "cut filler",
            ],
        ) && let Some(asset_id) = asset_id
        {
            commands.push(EditCommand::RemoveFillers {
                asset_id,
                words: vec![
                    "嗯".to_string(),
                    "呃".to_string(),
                    "那個".to_string(),
                    "就是".to_string(),
                    "um".to_string(),
                    "uh".to_string(),
                    "erm".to_string(),
                ],
                padding_ms: 35,
            });
            actions.push("移除常見贅詞");
        }

        if contains_any(&normalized, &["字幕", "caption", "subtitles"])
            && let Some(asset_id) = asset_id
        {
            let preset = caption_preset_from_prompt(&normalized);
            let style = TextOverlay {
                position: TextPosition::Bottom,
                font_size: if normalized.contains("直式") || normalized.contains("9:16") {
                    54
                } else {
                    64
                },
                ..TextOverlay::default()
            };
            commands.push(EditCommand::AddCaptions {
                asset_id,
                style,
                preset,
            });
            actions.push(match preset {
                CaptionPreset::Karaoke => "產生逐字卡拉 OK 字幕",
                CaptionPreset::WordPop => "產生逐字彈出字幕",
                CaptionPreset::Hormozi => "產生 Hormozi 衝擊字幕",
                CaptionPreset::Minimal => "產生極簡字幕",
                CaptionPreset::Standard => "產生逐段字幕",
            });
        }

        if contains_any(&normalized, &["直式", "9:16", "tiktok", "reels", "shorts"]) {
            commands.push(EditCommand::Reframe {
                width: 1080,
                height: 1920,
            });
            actions.push("改為 9:16 直式");
        } else if contains_any(&normalized, &["方形", "1:1", "square"]) {
            commands.push(EditCommand::Reframe {
                width: 1080,
                height: 1080,
            });
            actions.push("改為 1:1 方形");
        } else if contains_any(&normalized, &["橫式", "16:9", "youtube"]) {
            commands.push(EditCommand::Reframe {
                width: 1920,
                height: 1080,
            });
            actions.push("改為 16:9 橫式");
        }

        if contains_any(
            &normalized,
            &["正規化音量", "音量標準化", "normalize audio", "loudness"],
        ) {
            commands.push(EditCommand::NormalizeAudio { target_lufs: -16.0 });
            actions.push("套用音量正規化");
        }

        if contains_any(&normalized, &["邊框", "畫框", "border", "frame border"]) {
            commands.push(EditCommand::AddBorder {
                color: "white".to_string(),
                width: 12,
            });
            actions.push("加入全片邊框");
        }

        if let Some(watermark) = extract_quoted_text(prompt)
            && contains_any(&normalized, &["浮水印", "watermark"])
        {
            commands.push(EditCommand::AddWatermark {
                text: watermark,
                font_file: None,
                position: TextPosition::Bottom,
                font_size: 32,
                color: "white".to_string(),
                opacity: 0.65,
                start_ms: 0,
                end_ms: None,
            });
            actions.push("加入全片浮水印");
        }

        if let Some(title) = extract_quoted_text(prompt)
            && contains_any(
                &normalized,
                &["動態字卡", "字卡", "標題", "title", "片頭文字", "文字"],
            )
            && !contains_any(&normalized, &["浮水印", "watermark"])
        {
            let text = TextOverlay {
                text: title,
                position: TextPosition::Center,
                font_size: 96,
                animation: if contains_any(&normalized, &["動態", "animated"]) {
                    TextAnimation::Pop
                } else {
                    TextAnimation::None
                },
                ..TextOverlay::default()
            };
            commands.push(EditCommand::AddText {
                text,
                start_ms: 0,
                end_ms: 3_000,
                track_id: None,
            });
            actions.push("新增片頭標題");
        }

        if asset_id.is_none() && commands.iter().any(command_requires_asset) {
            warnings
                .push("找不到可套用的素材；請先匯入素材，或在提示中加入素材 UUID。".to_string());
        }

        if commands.is_empty() {
            warnings.push(
                "規則規劃器無法可靠解析這個要求；可設定 LLM provider，或使用明確指令，例如「移除靜音、加字幕、改成 9:16」。"
                    .to_string(),
            );
        }

        let summary = if actions.is_empty() {
            "未產生可執行的剪輯步驟".to_string()
        } else {
            format!("預計執行：{}", actions.join("、"))
        };

        Ok(EditPlan {
            summary,
            commands,
            warnings,
        })
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatiblePlanner {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatiblePlanner {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }
}

#[async_trait]
impl Planner for OpenAiCompatiblePlanner {
    async fn plan(&self, project: &Project, prompt: &str) -> Result<EditPlan> {
        let project_context = serde_json::to_value(ProjectPlannerContext::from(project))?;
        let system = r#"You are a non-destructive video-editing planner. Return JSON only.
The response schema is:
{"summary":"...","commands":[EditCommand...],"warnings":["..."]}
Allowed EditCommand values use a tagged `type` field:
add_asset_to_timeline, trim_clip, split_clip, delete_clip, move_clip, set_volume,
set_speed, remove_silence, remove_fillers, add_captions, add_text, reframe,
add_watermark, add_border, add_fade, normalize_audio.
Text overlays support animation values: none, fade, slide_up, slide_left, pop.
add_captions supports preset values: standard, hormozi, minimal, karaoke, word_pop.
Use only UUIDs present in the project context. Do not invent assets, tracks, or clips.
Prefer reversible timeline edits. Keep all time values in milliseconds."#;

        let body = json!({
            "model": self.model,
            "temperature": 0.1,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": format!("Project context:\n{}\n\nEditing request:\n{}", project_context, prompt) }
            ]
        });

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        let payload: ChatCompletionResponse = response.json().await?;
        let content = payload
            .choices
            .first()
            .map(|choice| choice.message.content.as_str())
            .ok_or_else(|| RustCutError::Planner("provider returned no choices".to_string()))?;
        serde_json::from_str(content).map_err(|error| {
            RustCutError::Planner(format!(
                "could not decode EditPlan: {error}; content={content}"
            ))
        })
    }
}

#[derive(Debug, Serialize)]
struct ProjectPlannerContext {
    project_id: Uuid,
    assets: Vec<PlannerAsset>,
    tracks: Vec<PlannerTrack>,
    settings: PlannerSettings,
}

impl From<&Project> for ProjectPlannerContext {
    fn from(project: &Project) -> Self {
        Self {
            project_id: project.id,
            assets: project
                .assets
                .values()
                .map(|asset| PlannerAsset {
                    id: asset.id,
                    name: asset.name.clone(),
                    kind: format!("{:?}", asset.kind).to_lowercase(),
                    duration_ms: asset.duration_ms,
                    has_transcript: project.transcripts.contains_key(&asset.id),
                })
                .collect(),
            tracks: project
                .timeline
                .tracks
                .iter()
                .map(|track| PlannerTrack {
                    id: track.id,
                    name: track.name.clone(),
                    kind: format!("{:?}", track.kind).to_lowercase(),
                    clips: track
                        .clips
                        .iter()
                        .map(|clip| PlannerClip {
                            id: clip.id,
                            name: clip.name.clone(),
                            asset_id: clip.asset_id,
                            start_ms: clip.start_ms,
                            source_in_ms: clip.source_in_ms,
                            source_out_ms: clip.source_out_ms,
                        })
                        .collect(),
                })
                .collect(),
            settings: PlannerSettings {
                width: project.timeline.settings.width,
                height: project.timeline.settings.height,
                fps: project.timeline.settings.fps,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct PlannerAsset {
    id: Uuid,
    name: String,
    kind: String,
    duration_ms: u64,
    has_transcript: bool,
}

#[derive(Debug, Serialize)]
struct PlannerTrack {
    id: Uuid,
    name: String,
    kind: String,
    clips: Vec<PlannerClip>,
}

#[derive(Debug, Serialize)]
struct PlannerClip {
    id: Uuid,
    name: String,
    asset_id: Option<Uuid>,
    start_ms: u64,
    source_in_ms: u64,
    source_out_ms: u64,
}

#[derive(Debug, Serialize)]
struct PlannerSettings {
    width: u32,
    height: u32,
    fps: f64,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

fn resolve_asset_reference(project: &Project, prompt: &str) -> Option<Uuid> {
    prompt
        .split(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ';' | '，' | '；')
        })
        .filter_map(|token| {
            Uuid::parse_str(
                token.trim_matches(|character| matches!(character, '@' | '(' | ')' | '[' | ']')),
            )
            .ok()
        })
        .find(|id| project.assets.contains_key(id))
        .or_else(|| project.assets.keys().next().copied())
}

fn contains_any(input: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| input.contains(needle))
}

fn caption_preset_from_prompt(input: &str) -> CaptionPreset {
    if contains_any(
        input,
        &["逐字彈出", "彈跳字幕", "word pop", "word-pop", "word_pop"],
    ) {
        CaptionPreset::WordPop
    } else if contains_any(
        input,
        &["卡拉ok", "卡拉 ok", "karaoke", "逐字高亮", "逐字高亮字幕"],
    ) {
        CaptionPreset::Karaoke
    } else if contains_any(input, &["hormozi", "荷莫茲", "衝擊字幕", "大字字幕"]) {
        CaptionPreset::Hormozi
    } else if contains_any(input, &["極簡字幕", "minimal caption", "minimal subtitles"]) {
        CaptionPreset::Minimal
    } else {
        CaptionPreset::Standard
    }
}

fn extract_quoted_text(input: &str) -> Option<String> {
    let pairs = [('「', '」'), ('“', '”'), ('"', '"'), ('\'', '\'')];
    for (open, close) in pairs {
        let Some(start) = input.find(open) else {
            continue;
        };
        let remainder = &input[start + open.len_utf8()..];
        if let Some(end) = remainder.find(close) {
            let value = remainder[..end].trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn command_requires_asset(command: &EditCommand) -> bool {
    matches!(
        command,
        EditCommand::AddAssetToTimeline { .. }
            | EditCommand::RemoveSilence { .. }
            | EditCommand::RemoveFillers { .. }
            | EditCommand::AddCaptions { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TimelineSettings;

    #[tokio::test]
    async fn rule_planner_understands_vertical_caption_request() {
        let project = Project::new("demo", TimelineSettings::default());
        let plan = RulePlanner
            .plan(&project, "改成 9:16 並加字幕")
            .await
            .unwrap();
        assert!(plan.commands.iter().any(|command| matches!(
            command,
            EditCommand::Reframe {
                width: 1080,
                height: 1920
            }
        )));
    }

    #[tokio::test]
    async fn rule_planner_understands_visual_effect_requests() {
        let project = Project::new("demo", TimelineSettings::default());
        let plan = RulePlanner
            .plan(&project, "加入紫色邊框與浮水印「RustCut」")
            .await
            .unwrap();
        assert!(
            plan.commands
                .iter()
                .any(|command| matches!(command, EditCommand::AddBorder { .. }))
        );
        assert!(plan.commands.iter().any(|command| matches!(
            command,
            EditCommand::AddWatermark { text, .. } if text == "RustCut"
        )));
        assert!(
            !plan
                .commands
                .iter()
                .any(|command| matches!(command, EditCommand::AddText { .. }))
        );
    }

    #[test]
    fn extracts_chinese_title() {
        assert_eq!(
            extract_quoted_text("新增標題「Rust 影片剪輯」"),
            Some("Rust 影片剪輯".to_string())
        );
    }

    #[test]
    fn detects_caption_skill_presets() {
        assert_eq!(
            caption_preset_from_prompt("幫我做逐字彈出字幕"),
            CaptionPreset::WordPop
        );
        assert_eq!(
            caption_preset_from_prompt("use karaoke captions"),
            CaptionPreset::Karaoke
        );
        assert_eq!(
            caption_preset_from_prompt("套用 Hormozi 衝擊字幕"),
            CaptionPreset::Hormozi
        );
        assert_eq!(
            caption_preset_from_prompt("極簡字幕"),
            CaptionPreset::Minimal
        );
    }
}
