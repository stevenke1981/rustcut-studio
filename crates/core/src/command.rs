use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    CaptionPreset, Clip, Effect, Millis, Project, Result, RustCutError, TextAnimation, TextOverlay,
    TextPosition, Track, TrackKind,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EditPlan {
    pub summary: String,
    #[serde(default)]
    pub commands: Vec<EditCommand>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EditCommand {
    AddAssetToTimeline {
        asset_id: Uuid,
        #[serde(default)]
        track_kind: Option<TrackKind>,
        #[serde(default)]
        at_ms: Option<Millis>,
    },
    TrimClip {
        clip_id: Uuid,
        source_in_ms: Millis,
        source_out_ms: Millis,
    },
    SplitClip {
        clip_id: Uuid,
        at_ms: Millis,
    },
    DeleteClip {
        clip_id: Uuid,
    },
    MoveClip {
        clip_id: Uuid,
        start_ms: Millis,
        #[serde(default)]
        track_id: Option<Uuid>,
    },
    SetVolume {
        clip_id: Uuid,
        volume: f32,
    },
    SetSpeed {
        clip_id: Uuid,
        speed: f64,
    },
    RemoveSilence {
        asset_id: Uuid,
        #[serde(default = "default_silence_gap")]
        min_gap_ms: Millis,
        #[serde(default = "default_keep_ms")]
        keep_ms: Millis,
    },
    RemoveFillers {
        asset_id: Uuid,
        #[serde(default = "default_fillers")]
        words: Vec<String>,
        #[serde(default = "default_filler_padding")]
        padding_ms: Millis,
    },
    AddCaptions {
        asset_id: Uuid,
        #[serde(default)]
        style: TextOverlay,
        #[serde(default)]
        preset: CaptionPreset,
    },
    AddText {
        text: TextOverlay,
        start_ms: Millis,
        end_ms: Millis,
        #[serde(default)]
        track_id: Option<Uuid>,
    },
    AddWatermark {
        text: String,
        #[serde(default)]
        font_file: Option<String>,
        #[serde(default = "default_watermark_position")]
        position: TextPosition,
        #[serde(default = "default_watermark_font_size")]
        font_size: u32,
        #[serde(default = "default_watermark_color")]
        color: String,
        #[serde(default = "default_watermark_opacity")]
        opacity: f32,
        #[serde(default)]
        start_ms: Millis,
        #[serde(default)]
        end_ms: Option<Millis>,
    },
    AddBorder {
        #[serde(default = "default_border_color")]
        color: String,
        #[serde(default = "default_border_width")]
        width: u32,
    },
    Reframe {
        width: u32,
        height: u32,
    },
    AddFade {
        clip_id: Uuid,
        #[serde(default)]
        in_ms: Millis,
        #[serde(default)]
        out_ms: Millis,
    },
    NormalizeAudio {
        #[serde(default = "default_target_lufs")]
        target_lufs: f32,
    },
}

fn default_silence_gap() -> Millis {
    650
}

fn default_keep_ms() -> Millis {
    90
}

fn default_filler_padding() -> Millis {
    35
}

fn default_target_lufs() -> f32 {
    -16.0
}

fn default_watermark_position() -> TextPosition {
    TextPosition::Bottom
}

fn default_watermark_font_size() -> u32 {
    32
}

fn default_watermark_color() -> String {
    "white".to_string()
}

fn default_watermark_opacity() -> f32 {
    0.65
}

fn default_border_color() -> String {
    "white".to_string()
}

fn default_border_width() -> u32 {
    12
}

fn default_fillers() -> Vec<String> {
    [
        "嗯", "呃", "那個", "就是", "然後", "um", "uh", "erm", "like",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub fn apply_commands(project: &mut Project, commands: &[EditCommand]) -> Result<()> {
    if commands.is_empty() {
        return Ok(());
    }
    project.snapshot();
    for command in commands {
        apply_command_in_place(project, command)?;
    }
    for track in &mut project.timeline.tracks {
        track.clips.sort_by_key(|clip| clip.start_ms);
    }
    Ok(())
}

fn apply_command_in_place(project: &mut Project, command: &EditCommand) -> Result<()> {
    match command {
        EditCommand::AddAssetToTimeline {
            asset_id,
            track_kind,
            at_ms,
        } => {
            let asset = project
                .assets
                .get(asset_id)
                .ok_or_else(|| RustCutError::AssetNotFound(asset_id.to_string()))?
                .clone();
            let kind = (*track_kind).unwrap_or(match asset.kind {
                crate::AssetKind::Audio => TrackKind::Audio,
                crate::AssetKind::Image | crate::AssetKind::Video | crate::AssetKind::Unknown => {
                    TrackKind::Video
                }
            });
            let timeline_end = project.timeline.duration_ms();
            let track = ensure_track(&mut project.timeline.tracks, kind);
            let start = (*at_ms).unwrap_or(timeline_end);
            track
                .clips
                .push(Clip::media(asset.name, asset.id, asset.duration_ms, start));
        }
        EditCommand::TrimClip {
            clip_id,
            source_in_ms,
            source_out_ms,
        } => {
            if source_out_ms <= source_in_ms {
                return Err(RustCutError::Validation(
                    "source_out_ms must be greater than source_in_ms".to_string(),
                ));
            }
            let clip = find_clip_mut(project, *clip_id)?;
            clip.source_in_ms = *source_in_ms;
            clip.source_out_ms = *source_out_ms;
        }
        EditCommand::SplitClip { clip_id, at_ms } => split_clip(project, *clip_id, *at_ms)?,
        EditCommand::DeleteClip { clip_id } => {
            let (track_index, clip_index) = find_clip_location(project, *clip_id)?;
            project.timeline.tracks[track_index]
                .clips
                .remove(clip_index);
        }
        EditCommand::MoveClip {
            clip_id,
            start_ms,
            track_id,
        } => {
            let (from_track_index, clip_index) = find_clip_location(project, *clip_id)?;
            let mut clip = project.timeline.tracks[from_track_index]
                .clips
                .remove(clip_index);
            clip.start_ms = *start_ms;
            if let Some(track_id) = track_id {
                let target = project
                    .timeline
                    .track_mut(*track_id)
                    .ok_or_else(|| RustCutError::TrackNotFound(track_id.to_string()))?;
                target.clips.push(clip);
            } else {
                project.timeline.tracks[from_track_index].clips.push(clip);
            }
        }
        EditCommand::SetVolume { clip_id, volume } => {
            if !(0.0..=4.0).contains(volume) {
                return Err(RustCutError::Validation(
                    "volume must be between 0.0 and 4.0".to_string(),
                ));
            }
            find_clip_mut(project, *clip_id)?.volume = *volume;
        }
        EditCommand::SetSpeed { clip_id, speed } => {
            if !(0.25..=4.0).contains(speed) {
                return Err(RustCutError::Validation(
                    "speed must be between 0.25 and 4.0".to_string(),
                ));
            }
            find_clip_mut(project, *clip_id)?.speed = *speed;
        }
        EditCommand::RemoveSilence {
            asset_id,
            min_gap_ms,
            keep_ms,
        } => remove_silence(project, *asset_id, *min_gap_ms, *keep_ms)?,
        EditCommand::RemoveFillers {
            asset_id,
            words,
            padding_ms,
        } => remove_fillers(project, *asset_id, words, *padding_ms)?,
        EditCommand::AddCaptions {
            asset_id,
            style,
            preset,
        } => {
            validate_text_overlay(style, true)?;
            add_captions(project, *asset_id, style.clone(), *preset)?
        }
        EditCommand::AddText {
            text,
            start_ms,
            end_ms,
            track_id,
        } => {
            if end_ms <= start_ms {
                return Err(RustCutError::Validation(
                    "text end_ms must be greater than start_ms".to_string(),
                ));
            }
            validate_text_overlay(text, false)?;
            let track = match track_id {
                Some(id) => project
                    .timeline
                    .track_mut(*id)
                    .ok_or_else(|| RustCutError::TrackNotFound(id.to_string()))?,
                None => ensure_track(&mut project.timeline.tracks, TrackKind::Graphic),
            };
            track
                .clips
                .push(Clip::text("Text overlay", text.clone(), *start_ms, *end_ms));
        }
        EditCommand::AddWatermark {
            text,
            font_file,
            position,
            font_size,
            color,
            opacity,
            start_ms,
            end_ms,
        } => {
            if text.trim().is_empty() {
                return Err(RustCutError::Validation(
                    "watermark text cannot be empty".to_string(),
                ));
            }
            if !(8..=512).contains(font_size) {
                return Err(RustCutError::Validation(
                    "watermark font_size must be between 8 and 512".to_string(),
                ));
            }
            if !(0.0..=1.0).contains(opacity) {
                return Err(RustCutError::Validation(
                    "watermark opacity must be between 0.0 and 1.0".to_string(),
                ));
            }
            validate_filter_color(color)?;
            let end_ms = end_ms.unwrap_or_else(|| project.timeline.duration_ms());
            if end_ms <= *start_ms {
                return Err(RustCutError::Validation(
                    "watermark end_ms must be greater than start_ms".to_string(),
                ));
            }
            let overlay = TextOverlay {
                text: text.trim().to_string(),
                font_file: font_file.clone(),
                font_size: *font_size,
                color: color.clone(),
                opacity: *opacity,
                outline_width: 2,
                position: *position,
                animation: TextAnimation::None,
                ..TextOverlay::default()
            };
            ensure_track(&mut project.timeline.tracks, TrackKind::Graphic)
                .clips
                .push(Clip::text("Watermark", overlay, *start_ms, end_ms));
        }
        EditCommand::AddBorder { color, width } => {
            let max_width = project
                .timeline
                .settings
                .width
                .min(project.timeline.settings.height)
                / 4;
            validate_filter_color(color)?;
            if *width == 0 || *width > max_width {
                return Err(RustCutError::Validation(format!(
                    "border width must be between 1 and {max_width}"
                )));
            }
            let mut applied = false;
            for track in &mut project.timeline.tracks {
                if track.kind != TrackKind::Video {
                    continue;
                }
                for clip in &mut track.clips {
                    if clip.asset_id.is_some() {
                        clip.effects
                            .retain(|effect| !matches!(effect, Effect::Border { .. }));
                        clip.effects.push(Effect::Border {
                            color: color.clone(),
                            width: *width,
                        });
                        applied = true;
                    }
                }
            }
            if !applied {
                return Err(RustCutError::Validation(
                    "timeline has no video clips for a border".to_string(),
                ));
            }
        }
        EditCommand::Reframe { width, height } => {
            if *width < 64 || *height < 64 {
                return Err(RustCutError::Validation(
                    "output dimensions must be at least 64×64".to_string(),
                ));
            }
            project.timeline.settings.width = *width;
            project.timeline.settings.height = *height;
        }
        EditCommand::AddFade {
            clip_id,
            in_ms,
            out_ms,
        } => {
            find_clip_mut(project, *clip_id)?
                .effects
                .push(Effect::Fade {
                    in_ms: *in_ms,
                    out_ms: *out_ms,
                });
        }
        EditCommand::NormalizeAudio { target_lufs } => {
            for track in &mut project.timeline.tracks {
                if matches!(track.kind, TrackKind::Audio | TrackKind::Video) {
                    for clip in &mut track.clips {
                        clip.effects.push(Effect::NormalizeAudio {
                            target_lufs: *target_lufs,
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn ensure_track(tracks: &mut Vec<Track>, kind: TrackKind) -> &mut Track {
    if let Some(index) = tracks.iter().position(|track| track.kind == kind) {
        return &mut tracks[index];
    }
    let order = tracks.iter().map(|track| track.order).max().unwrap_or(0) + 1;
    let prefix = match kind {
        TrackKind::Video => "V",
        TrackKind::Audio => "A",
        TrackKind::Caption => "C",
        TrackKind::Graphic => "G",
    };
    tracks.push(Track::new(format!("{prefix}{}", order + 1), kind, order));
    tracks.last_mut().expect("track was just pushed")
}

fn validate_text_overlay(text: &TextOverlay, allow_empty: bool) -> Result<()> {
    if !allow_empty && text.text.trim().is_empty() {
        return Err(RustCutError::Validation(
            "text overlay cannot be empty".to_string(),
        ));
    }
    if !(8..=512).contains(&text.font_size) {
        return Err(RustCutError::Validation(
            "text font_size must be between 8 and 512".to_string(),
        ));
    }
    if !(0.0..=1.0).contains(&text.opacity) {
        return Err(RustCutError::Validation(
            "text opacity must be between 0.0 and 1.0".to_string(),
        ));
    }
    if text.animation != TextAnimation::None && text.animation_duration_ms == 0 {
        return Err(RustCutError::Validation(
            "animated text requires animation_duration_ms greater than 0".to_string(),
        ));
    }
    validate_filter_color(&text.color)?;
    validate_filter_color(&text.outline_color)?;
    if let Some(color) = &text.box_color {
        validate_filter_color(color)?;
    }
    if let Some(color) = &text.shadow_color {
        validate_filter_color(color)?;
    }
    Ok(())
}

fn validate_filter_color(color: &str) -> Result<()> {
    let color = color.trim();
    let mut parts = color.split('@');
    let base = parts.next().unwrap_or_default();
    let alpha = parts.next();
    if parts.next().is_some() {
        return Err(RustCutError::Validation(
            "color must be an FFmpeg color name or hexadecimal value".to_string(),
        ));
    }
    if let Some(alpha) = alpha {
        let alpha = alpha.parse::<f32>().map_err(|_| {
            RustCutError::Validation("color alpha must be between 0.0 and 1.0".to_string())
        })?;
        if !(0.0..=1.0).contains(&alpha) {
            return Err(RustCutError::Validation(
                "color alpha must be between 0.0 and 1.0".to_string(),
            ));
        }
    }
    let valid_hex = base.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
    });
    let valid_0x = base
        .strip_prefix("0x")
        .or_else(|| base.strip_prefix("0X"))
        .is_some_and(|hex| {
            matches!(hex.len(), 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
        });
    let valid_name = matches!(
        base.to_ascii_lowercase().as_str(),
        "black"
            | "white"
            | "red"
            | "green"
            | "blue"
            | "yellow"
            | "cyan"
            | "magenta"
            | "gray"
            | "grey"
            | "orange"
            | "purple"
            | "pink"
            | "lime"
            | "navy"
            | "teal"
            | "silver"
            | "maroon"
            | "olive"
            | "aqua"
            | "fuchsia"
            | "transparent"
    );
    if !(valid_hex || valid_0x || valid_name) {
        return Err(RustCutError::Validation(
            "color must be a supported name, #RRGGBB, #RRGGBBAA, 0xRRGGBB, or 0xRRGGBBAA"
                .to_string(),
        ));
    }
    Ok(())
}

fn find_clip_location(project: &Project, clip_id: Uuid) -> Result<(usize, usize)> {
    project
        .timeline
        .tracks
        .iter()
        .enumerate()
        .find_map(|(track_index, track)| {
            track
                .clips
                .iter()
                .position(|clip| clip.id == clip_id)
                .map(|clip_index| (track_index, clip_index))
        })
        .ok_or_else(|| RustCutError::ClipNotFound(clip_id.to_string()))
}

fn find_clip_mut(project: &mut Project, clip_id: Uuid) -> Result<&mut Clip> {
    let (track_index, clip_index) = find_clip_location(project, clip_id)?;
    Ok(&mut project.timeline.tracks[track_index].clips[clip_index])
}

fn split_clip(project: &mut Project, clip_id: Uuid, at_ms: Millis) -> Result<()> {
    let (track_index, clip_index) = find_clip_location(project, clip_id)?;
    let original = project.timeline.tracks[track_index].clips[clip_index].clone();
    if at_ms <= original.start_ms || at_ms >= original.end_ms() {
        return Err(RustCutError::Validation(
            "split position must be inside the clip".to_string(),
        ));
    }
    let elapsed = at_ms - original.start_ms;
    let source_split = original.source_in_ms + (elapsed as f64 * original.speed).round() as Millis;

    let mut left = original.clone();
    left.source_out_ms = source_split;

    let mut right = original;
    right.id = Uuid::new_v4();
    right.source_in_ms = source_split;
    right.start_ms = at_ms;

    let clips = &mut project.timeline.tracks[track_index].clips;
    clips.splice(clip_index..=clip_index, [left, right]);
    Ok(())
}

fn remove_silence(
    project: &mut Project,
    asset_id: Uuid,
    min_gap_ms: Millis,
    keep_ms: Millis,
) -> Result<()> {
    let transcript = project
        .transcripts
        .get(&asset_id)
        .ok_or_else(|| RustCutError::Validation("transcript is required".to_string()))?;
    if transcript.segments.is_empty() {
        return Ok(());
    }

    let mut keep_ranges: Vec<(Millis, Millis)> = Vec::new();
    for segment in &transcript.segments {
        let start = segment.start_ms.saturating_sub(keep_ms);
        let end = segment.end_ms.saturating_add(keep_ms);
        if let Some(last) = keep_ranges.last_mut()
            && start.saturating_sub(last.1) <= min_gap_ms
        {
            last.1 = last.1.max(end);
            continue;
        }
        keep_ranges.push((start, end));
    }
    replace_asset_clips_with_ranges(project, asset_id, &keep_ranges)
}

fn remove_fillers(
    project: &mut Project,
    asset_id: Uuid,
    words: &[String],
    padding_ms: Millis,
) -> Result<()> {
    let transcript = project
        .transcripts
        .get(&asset_id)
        .ok_or_else(|| RustCutError::Validation("transcript is required".to_string()))?;
    let fillers: HashSet<String> = words.iter().map(|word| normalize_word(word)).collect();
    let mut removed = Vec::new();
    for word in transcript.words() {
        if fillers.contains(&normalize_word(&word.text)) {
            removed.push((
                word.start_ms.saturating_sub(padding_ms),
                word.end_ms.saturating_add(padding_ms),
            ));
        }
    }
    if removed.is_empty() {
        return Ok(());
    }
    removed.sort_by_key(|range| range.0);
    let merged = merge_ranges(&removed, 10);
    let asset_duration = project
        .assets
        .get(&asset_id)
        .ok_or_else(|| RustCutError::AssetNotFound(asset_id.to_string()))?
        .duration_ms;
    let keep = complement_ranges(&merged, 0, asset_duration);
    replace_asset_clips_with_ranges(project, asset_id, &keep)
}

fn replace_asset_clips_with_ranges(
    project: &mut Project,
    asset_id: Uuid,
    ranges: &[(Millis, Millis)],
) -> Result<()> {
    let mut replaced_any = false;
    for track in &mut project.timeline.tracks {
        if track.kind != TrackKind::Video {
            continue;
        }
        let mut output = Vec::new();
        for clip in track.clips.drain(..) {
            if clip.asset_id != Some(asset_id) {
                output.push(clip);
                continue;
            }
            replaced_any = true;
            let mut timeline_cursor = clip.start_ms;
            for &(start, end) in ranges {
                let source_start = start.max(clip.source_in_ms);
                let source_end = end.min(clip.source_out_ms);
                if source_end <= source_start {
                    continue;
                }
                let mut piece = clip.clone();
                piece.id = Uuid::new_v4();
                piece.source_in_ms = source_start;
                piece.source_out_ms = source_end;
                piece.start_ms = timeline_cursor;
                timeline_cursor = timeline_cursor.saturating_add(piece.duration_ms());
                output.push(piece);
            }
        }
        track.clips = output;
    }
    if !replaced_any {
        return Err(RustCutError::Validation(
            "asset is not present on a video track".to_string(),
        ));
    }
    Ok(())
}

fn add_captions(
    project: &mut Project,
    asset_id: Uuid,
    style: TextOverlay,
    preset: CaptionPreset,
) -> Result<()> {
    let transcript = project
        .transcripts
        .get(&asset_id)
        .ok_or_else(|| RustCutError::Validation("transcript is required".to_string()))?
        .clone();

    let video_clips: Vec<Clip> = project
        .timeline
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Video)
        .flat_map(|track| track.clips.iter())
        .filter(|clip| clip.asset_id == Some(asset_id))
        .cloned()
        .collect();
    if video_clips.is_empty() {
        return Err(RustCutError::Validation(
            "asset is not present on a video track".to_string(),
        ));
    }

    let style = caption_style_for_preset(style, preset);
    let mut generated = Vec::new();
    for media_clip in video_clips {
        for segment in &transcript.segments {
            let uses_words = matches!(preset, CaptionPreset::Karaoke | CaptionPreset::WordPop)
                && !segment.words.is_empty();
            if uses_words {
                let generated_before_words = generated.len();
                for word in &segment.words {
                    push_caption(
                        &mut generated,
                        &media_clip,
                        asset_id,
                        &style,
                        word.text.trim(),
                        word.start_ms,
                        word.end_ms,
                        preset,
                    );
                }
                if generated.len() == generated_before_words {
                    push_caption(
                        &mut generated,
                        &media_clip,
                        asset_id,
                        &style,
                        segment.text.trim(),
                        segment.start_ms,
                        segment.end_ms,
                        preset,
                    );
                }
            } else {
                push_caption(
                    &mut generated,
                    &media_clip,
                    asset_id,
                    &style,
                    segment.text.trim(),
                    segment.start_ms,
                    segment.end_ms,
                    preset,
                );
            }
        }
    }

    let caption_track = ensure_track(&mut project.timeline.tracks, TrackKind::Caption);
    caption_track
        .clips
        .retain(|clip| clip.asset_id != Some(asset_id));
    caption_track.clips.extend(generated);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_caption(
    output: &mut Vec<Clip>,
    media_clip: &Clip,
    asset_id: Uuid,
    base_style: &TextOverlay,
    text: &str,
    source_start_ms: Millis,
    source_end_ms: Millis,
    preset: CaptionPreset,
) {
    let source_start = source_start_ms.max(media_clip.source_in_ms);
    let source_end = source_end_ms.min(media_clip.source_out_ms);
    if text.is_empty() || source_end <= source_start {
        return;
    }

    let timeline_start = source_to_timeline(media_clip, source_start);
    let timeline_end = source_to_timeline(media_clip, source_end);
    if timeline_end <= timeline_start {
        return;
    }

    let mut style = base_style.clone();
    style.text = text.to_string();
    let name = match preset {
        CaptionPreset::Karaoke => "Karaoke Caption",
        CaptionPreset::WordPop => "Word Pop Caption",
        _ => "Caption",
    };
    let mut caption = Clip::text(name, style, timeline_start, timeline_end);
    caption.asset_id = Some(asset_id);
    output.push(caption);
}

fn source_to_timeline(media_clip: &Clip, source_ms: Millis) -> Millis {
    media_clip.start_ms
        + ((source_ms - media_clip.source_in_ms) as f64 / media_clip.speed).round() as Millis
}

fn caption_style_for_preset(mut style: TextOverlay, preset: CaptionPreset) -> TextOverlay {
    match preset {
        CaptionPreset::Standard => {}
        CaptionPreset::Hormozi => {
            style.font_size = style.font_size.max(84);
            style.position = TextPosition::Center;
            style.outline_width = style.outline_width.max(6);
            style.box_color = None;
            style.shadow_color = Some("#000000cc".to_string());
            style.shadow_x = 5;
            style.shadow_y = 5;
            style.animation = TextAnimation::Pop;
            style.animation_duration_ms = 260;
        }
        CaptionPreset::Minimal => {
            style.font_size = style.font_size.min(48);
            style.opacity = style.opacity.min(0.92);
            style.outline_width = style.outline_width.min(2);
            style.box_color = None;
            style.shadow_color = None;
            style.animation = TextAnimation::Fade;
            style.animation_duration_ms = 240;
        }
        CaptionPreset::Karaoke => {
            style.font_size = style.font_size.max(72);
            style.color = "#ffff00".to_string();
            style.position = TextPosition::LowerThird;
            style.outline_width = style.outline_width.max(5);
            style.box_color = None;
            style.animation = TextAnimation::Fade;
            style.animation_duration_ms = 160;
        }
        CaptionPreset::WordPop => {
            style.font_size = style.font_size.max(84);
            style.position = TextPosition::Center;
            style.outline_width = style.outline_width.max(6);
            style.box_color = None;
            style.animation = TextAnimation::Pop;
            style.animation_duration_ms = 220;
        }
    }
    style
}

fn normalize_word(input: &str) -> String {
    input
        .trim()
        .trim_matches(|character: char| !character.is_alphanumeric())
        .to_lowercase()
}

fn merge_ranges(ranges: &[(Millis, Millis)], tolerance_ms: Millis) -> Vec<(Millis, Millis)> {
    let mut merged: Vec<(Millis, Millis)> = Vec::new();
    for &(start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start <= last.1.saturating_add(tolerance_ms)
        {
            last.1 = last.1.max(end);
            continue;
        }
        merged.push((start, end));
    }
    merged
}

fn complement_ranges(
    removed: &[(Millis, Millis)],
    range_start: Millis,
    range_end: Millis,
) -> Vec<(Millis, Millis)> {
    let mut keep = Vec::new();
    let mut cursor = range_start;
    for &(start, end) in removed {
        if start > cursor {
            keep.push((cursor, start.min(range_end)));
        }
        cursor = cursor.max(end);
        if cursor >= range_end {
            break;
        }
    }
    if cursor < range_end {
        keep.push((cursor, range_end));
    }
    keep.into_iter()
        .filter(|(start, end)| end > start)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Asset, AssetKind, TimelineSettings, Transcript, TranscriptSegment, TranscriptWord,
    };
    use chrono::Utc;
    use std::collections::BTreeMap;

    fn test_project() -> (Project, Uuid) {
        let mut project = Project::new("test", TimelineSettings::default());
        let asset_id = Uuid::new_v4();
        project.assets.insert(
            asset_id,
            Asset {
                id: asset_id,
                name: "clip.mp4".to_string(),
                path: "assets/clip.mp4".to_string(),
                kind: AssetKind::Video,
                duration_ms: 10_000,
                width: Some(1920),
                height: Some(1080),
                fps: Some(30.0),
                sample_rate: Some(48_000),
                channels: Some(2),
                sha256: "test".to_string(),
                created_at: Utc::now(),
            },
        );
        project.transcripts = BTreeMap::from([(
            asset_id,
            Transcript {
                asset_id,
                language: Some("zh".to_string()),
                segments: vec![TranscriptSegment {
                    start_ms: 1_000,
                    end_ms: 4_000,
                    text: "嗯 測試".to_string(),
                    speaker: None,
                    words: vec![
                        TranscriptWord {
                            start_ms: 1_000,
                            end_ms: 1_300,
                            text: "嗯".to_string(),
                            confidence: None,
                        },
                        TranscriptWord {
                            start_ms: 1_500,
                            end_ms: 2_000,
                            text: "測試".to_string(),
                            confidence: None,
                        },
                    ],
                }],
            },
        )]);
        apply_commands(
            &mut project,
            &[EditCommand::AddAssetToTimeline {
                asset_id,
                track_kind: Some(TrackKind::Video),
                at_ms: Some(0),
            }],
        )
        .unwrap();
        (project, asset_id)
    }

    #[test]
    fn splits_clip_at_timeline_position() {
        let (mut project, _) = test_project();
        let clip_id = project
            .timeline
            .first_track_of_kind(TrackKind::Video)
            .unwrap()
            .clips[0]
            .id;
        apply_commands(
            &mut project,
            &[EditCommand::SplitClip {
                clip_id,
                at_ms: 3_000,
            }],
        )
        .unwrap();
        let clips = &project
            .timeline
            .first_track_of_kind(TrackKind::Video)
            .unwrap()
            .clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].source_out_ms, 3_000);
        assert_eq!(clips[1].source_in_ms, 3_000);
    }

    #[test]
    fn removes_filler_word_range() {
        let (mut project, asset_id) = test_project();
        apply_commands(
            &mut project,
            &[EditCommand::RemoveFillers {
                asset_id,
                words: vec!["嗯".to_string()],
                padding_ms: 0,
            }],
        )
        .unwrap();
        let clips = &project
            .timeline
            .first_track_of_kind(TrackKind::Video)
            .unwrap()
            .clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].source_out_ms, 1_000);
        assert_eq!(clips[1].source_in_ms, 1_300);
    }

    #[test]
    fn creates_caption_clips_from_imported_transcript() {
        let (mut project, asset_id) = test_project();
        let style = TextOverlay {
            animation: TextAnimation::Fade,
            box_color: Some("#00000099".to_string()),
            ..TextOverlay::default()
        };
        apply_commands(
            &mut project,
            &[EditCommand::AddCaptions {
                asset_id,
                style,
                preset: CaptionPreset::Standard,
            }],
        )
        .unwrap();

        let captions = &project
            .timeline
            .first_track_of_kind(TrackKind::Caption)
            .unwrap()
            .clips;
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].start_ms, 1_000);
        assert_eq!(captions[0].end_ms(), 4_000);
        assert_eq!(captions[0].text.as_ref().unwrap().text, "嗯 測試");
        assert_eq!(
            captions[0].text.as_ref().unwrap().animation,
            TextAnimation::Fade
        );
    }

    #[test]
    fn rejects_captions_when_asset_is_not_on_video_timeline() {
        let (mut project, asset_id) = test_project();
        project
            .timeline
            .first_track_of_kind_mut(TrackKind::Video)
            .unwrap()
            .clips
            .clear();
        let error = apply_commands(
            &mut project,
            &[EditCommand::AddCaptions {
                asset_id,
                style: TextOverlay::default(),
                preset: CaptionPreset::Standard,
            }],
        )
        .unwrap_err();
        assert!(error.to_string().contains("not present on a video track"));
    }

    #[test]
    fn creates_word_synchronized_pop_captions() {
        let (mut project, asset_id) = test_project();
        apply_commands(
            &mut project,
            &[EditCommand::AddCaptions {
                asset_id,
                style: TextOverlay::default(),
                preset: CaptionPreset::WordPop,
            }],
        )
        .unwrap();

        let captions = &project
            .timeline
            .first_track_of_kind(TrackKind::Caption)
            .unwrap()
            .clips;
        assert_eq!(captions.len(), 2);
        assert_eq!((captions[0].start_ms, captions[0].end_ms()), (1_000, 1_300));
        assert_eq!((captions[1].start_ms, captions[1].end_ms()), (1_500, 2_000));
        assert_eq!(captions[0].text.as_ref().unwrap().text, "嗯");
        assert_eq!(
            captions[0].text.as_ref().unwrap().animation,
            TextAnimation::Pop
        );
        assert_eq!(captions[0].text.as_ref().unwrap().font_size, 84);
    }

    #[test]
    fn creates_word_synchronized_karaoke_highlights() {
        let (mut project, asset_id) = test_project();
        apply_commands(
            &mut project,
            &[EditCommand::AddCaptions {
                asset_id,
                style: TextOverlay::default(),
                preset: CaptionPreset::Karaoke,
            }],
        )
        .unwrap();

        let captions = &project
            .timeline
            .first_track_of_kind(TrackKind::Caption)
            .unwrap()
            .clips;
        assert_eq!(captions.len(), 2);
        assert!(
            captions
                .iter()
                .all(|clip| clip.text.as_ref().unwrap().color == "#ffff00")
        );
        assert_eq!(captions[1].text.as_ref().unwrap().text, "測試");
    }

    #[test]
    fn word_preset_falls_back_when_all_words_are_invalid() {
        let (mut project, asset_id) = test_project();
        project.transcripts.get_mut(&asset_id).unwrap().segments[0].words = vec![TranscriptWord {
            start_ms: 2_000,
            end_ms: 2_000,
            text: String::new(),
            confidence: None,
        }];

        apply_commands(
            &mut project,
            &[EditCommand::AddCaptions {
                asset_id,
                style: TextOverlay::default(),
                preset: CaptionPreset::WordPop,
            }],
        )
        .unwrap();

        let captions = &project
            .timeline
            .first_track_of_kind(TrackKind::Caption)
            .unwrap()
            .clips;
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].text.as_ref().unwrap().text, "嗯 測試");
        assert_eq!((captions[0].start_ms, captions[0].end_ms()), (1_000, 4_000));
    }

    #[test]
    fn add_captions_json_defaults_to_standard_preset() {
        let asset_id = Uuid::new_v4();
        let command: EditCommand = serde_json::from_value(serde_json::json!({
            "type": "add_captions",
            "asset_id": asset_id,
            "style": {}
        }))
        .unwrap();
        assert!(matches!(
            command,
            EditCommand::AddCaptions {
                preset: CaptionPreset::Standard,
                ..
            }
        ));
    }

    #[test]
    fn adds_watermark_and_border_as_reversible_timeline_edits() {
        let (mut project, _) = test_project();
        apply_commands(
            &mut project,
            &[
                EditCommand::AddWatermark {
                    text: "RustCut".to_string(),
                    font_file: Some("C:/Windows/Fonts/msjh.ttc".to_string()),
                    position: TextPosition::Top,
                    font_size: 30,
                    color: "#ffffff".to_string(),
                    opacity: 0.55,
                    start_ms: 0,
                    end_ms: None,
                },
                EditCommand::AddBorder {
                    color: "#8d7bff".to_string(),
                    width: 16,
                },
            ],
        )
        .unwrap();

        let watermark = project
            .timeline
            .first_track_of_kind(TrackKind::Graphic)
            .unwrap()
            .clips
            .iter()
            .find(|clip| clip.name == "Watermark")
            .unwrap();
        assert_eq!(watermark.end_ms(), 10_000);
        assert_eq!(watermark.text.as_ref().unwrap().opacity, 0.55);

        let video = &project
            .timeline
            .first_track_of_kind(TrackKind::Video)
            .unwrap()
            .clips[0];
        assert!(matches!(
            video.effects.last(),
            Some(Effect::Border { color, width }) if color == "#8d7bff" && *width == 16
        ));

        assert!(project.undo());
        assert!(
            project
                .timeline
                .first_track_of_kind(TrackKind::Graphic)
                .unwrap()
                .clips
                .is_empty()
        );
    }

    #[test]
    fn text_commands_accept_legacy_style_fields() {
        let command: EditCommand = serde_json::from_value(serde_json::json!({
            "type": "add_text",
            "text": {
                "text": "相容舊專案",
                "font_size": 48,
                "position": "center",
                "alignment": "center"
            },
            "start_ms": 0,
            "end_ms": 1000
        }))
        .unwrap();

        let EditCommand::AddText { text, .. } = command else {
            panic!("expected add_text");
        };
        assert_eq!(text.animation, TextAnimation::None);
        assert_eq!(text.opacity, 1.0);
        assert_eq!(text.box_padding, 20);
    }

    #[test]
    fn rejects_filter_color_injection() {
        let (mut project, _) = test_project();
        let error = apply_commands(
            &mut project,
            &[EditCommand::AddBorder {
                color: "white,drawtext=text=oops".to_string(),
                width: 12,
            }],
        )
        .unwrap_err();
        assert!(error.to_string().contains("color must be"));

        for color in ["notacolor", "red@999", "#12345"] {
            let error = apply_commands(
                &mut project,
                &[EditCommand::AddBorder {
                    color: color.to_string(),
                    width: 12,
                }],
            )
            .unwrap_err();
            assert!(error.to_string().contains("color"));
        }
    }
}
