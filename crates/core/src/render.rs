use std::{
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    Asset, AssetKind, Clip, Effect, Project, Result, RustCutError, TextAlignment, TextPosition,
    TrackKind, resolve_asset_path,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderOptions {
    pub ffmpeg: String,
    pub video_codec: String,
    pub audio_codec: String,
    pub preset: String,
    pub crf: u8,
    pub audio_bitrate: String,
    pub overwrite: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            ffmpeg: "ffmpeg".to_string(),
            video_codec: "libx264".to_string(),
            audio_codec: "aac".to_string(),
            preset: "medium".to_string(),
            crf: 20,
            audio_bitrate: "192k".to_string(),
            overwrite: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderCommand {
    pub program: String,
    pub args: Vec<String>,
    pub output: PathBuf,
    pub filter_complex: String,
}

impl RenderCommand {
    pub fn display_shell(&self) -> String {
        std::iter::once(shell_quote(&self.program))
            .chain(self.args.iter().map(|arg| shell_quote(arg)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone)]
pub struct Renderer {
    options: RenderOptions,
}

impl Renderer {
    pub fn new(options: RenderOptions) -> Self {
        Self { options }
    }

    pub fn build_command(
        &self,
        project: &Project,
        project_dir: &Path,
        output: impl Into<PathBuf>,
    ) -> Result<RenderCommand> {
        let output = output.into();
        let primary_track = project
            .timeline
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Video && !track.muted)
            .min_by_key(|track| track.order)
            .ok_or_else(|| RustCutError::Validation("timeline has no video track".to_string()))?;
        let mut primary_clips: Vec<&Clip> = primary_track
            .clips
            .iter()
            .filter(|clip| clip.enabled && clip.asset_id.is_some())
            .collect();
        primary_clips.sort_by_key(|clip| clip.start_ms);
        if primary_clips.is_empty() {
            return Err(RustCutError::Validation(
                "primary video track has no media clips".to_string(),
            ));
        }

        let mut args = Vec::new();
        if self.options.overwrite {
            args.push("-y".to_string());
        } else {
            args.push("-n".to_string());
        }
        args.extend([
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "warning".to_string(),
        ]);

        let mut filters = Vec::new();
        let mut input_index = 0_usize;
        let width = project.timeline.settings.width;
        let height = project.timeline.settings.height;
        let fps = project.timeline.settings.fps;
        let sample_rate = project.timeline.settings.sample_rate;
        let mut primary_video_labels = Vec::new();
        let mut primary_audio_labels = Vec::new();

        for (clip_index, clip) in primary_clips.iter().enumerate() {
            let asset = asset_for_clip(project, clip)?;
            add_input_args(&mut args, asset, clip, project_dir);
            let duration_seconds = clip.duration_ms() as f64 / 1_000.0;
            let source_in = clip.source_in_ms as f64 / 1_000.0;
            let source_out = clip.source_out_ms as f64 / 1_000.0;
            let mut video_chain = format!(
                "[{input_index}:v:0]trim=start={source_in:.3}:end={source_out:.3},setpts=(PTS-STARTPTS)/{speed:.6},scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black,fps={fps:.6},setsar=1",
                speed = clip.speed
            );
            append_video_effects(&mut video_chain, clip, duration_seconds);
            let video_label = format!("pv{clip_index}");
            video_chain.push_str(&format!(",format=yuv420p[{video_label}]"));
            filters.push(video_chain);
            primary_video_labels.push(video_label);

            let audio_label = format!("pa{clip_index}");
            if asset.channels.unwrap_or(0) > 0 {
                let mut audio_chain = format!(
                    "[{input_index}:a:0]atrim=start={source_in:.3}:end={source_out:.3},asetpts=PTS-STARTPTS"
                );
                if (clip.speed - 1.0).abs() > f64::EPSILON {
                    audio_chain.push(',');
                    audio_chain.push_str(&atempo_chain(clip.speed));
                }
                audio_chain.push_str(&format!(",volume={:.4}", clip.volume));
                append_audio_effects(&mut audio_chain, clip, duration_seconds);
                audio_chain.push_str(&format!(",aresample={sample_rate}[{audio_label}]"));
                filters.push(audio_chain);
            } else {
                filters.push(format!(
                    "anullsrc=r={sample_rate}:cl=stereo,atrim=duration={duration_seconds:.3}[{audio_label}]"
                ));
            }
            primary_audio_labels.push(audio_label);
            input_index += 1;
        }

        let (mut video_output, dialogue_audio) = if primary_video_labels.len() == 1 {
            filters.push(format!("[{}]null[vbase]", primary_video_labels[0]));
            filters.push(format!("[{}]anull[dialogue]", primary_audio_labels[0]));
            ("vbase".to_string(), "dialogue".to_string())
        } else {
            filters.push(format!(
                "{}concat=n={}:v=1:a=0[vbase]",
                join_labels(&primary_video_labels),
                primary_video_labels.len()
            ));
            filters.push(format!(
                "{}concat=n={}:v=0:a=1[dialogue]",
                join_labels(&primary_audio_labels),
                primary_audio_labels.len()
            ));
            ("vbase".to_string(), "dialogue".to_string())
        };

        let overlay_tracks = project.timeline.tracks.iter().filter(|track| {
            track.kind == TrackKind::Video && track.id != primary_track.id && !track.muted
        });
        let mut overlay_number = 0_usize;
        for track in overlay_tracks {
            let mut clips: Vec<&Clip> = track
                .clips
                .iter()
                .filter(|clip| clip.enabled && clip.asset_id.is_some())
                .collect();
            clips.sort_by_key(|clip| clip.start_ms);
            for clip in clips {
                let asset = asset_for_clip(project, clip)?;
                add_input_args(&mut args, asset, clip, project_dir);
                let source_in = clip.source_in_ms as f64 / 1_000.0;
                let source_out = clip.source_out_ms as f64 / 1_000.0;
                let start = clip.start_ms as f64 / 1_000.0;
                let end = clip.end_ms() as f64 / 1_000.0;
                let overlay_label = format!("ov{overlay_number}");
                filters.push(format!(
                    "[{input_index}:v:0]trim=start={source_in:.3}:end={source_out:.3},setpts=(PTS-STARTPTS)/{speed:.6}+{start:.3}/TB,scale={width}:{height}:force_original_aspect_ratio=decrease,format=rgba,colorchannelmixer=aa={opacity:.4}[{overlay_label}]",
                    speed = clip.speed,
                    opacity = clip.opacity
                ));
                let next = format!("v_overlay_{overlay_number}");
                let x = overlay_x_expression(clip, width);
                let y = overlay_y_expression(clip, height);
                filters.push(format!(
                    "[{video_output}][{overlay_label}]overlay=x='{x}':y='{y}':eof_action=pass:enable='between(t,{start:.3},{end:.3})'[{next}]"
                ));
                video_output = next;
                input_index += 1;
                overlay_number += 1;
            }
        }

        let mut text_number = 0_usize;
        for track in project.timeline.tracks.iter().filter(|track| {
            matches!(track.kind, TrackKind::Caption | TrackKind::Graphic) && !track.muted
        }) {
            let mut clips: Vec<&Clip> = track
                .clips
                .iter()
                .filter(|clip| clip.enabled && clip.text.is_some())
                .collect();
            clips.sort_by_key(|clip| clip.start_ms);
            for clip in clips {
                let text = clip.text.as_ref().expect("filtered to text clips");
                let start = clip.start_ms as f64 / 1_000.0;
                let end = clip.end_ms() as f64 / 1_000.0;
                let next = format!("v_text_{text_number}");
                let drawtext = drawtext_filter(text, start, end, width, height);
                filters.push(format!("[{video_output}]{drawtext}[{next}]"));
                video_output = next;
                text_number += 1;
            }
        }

        let mut audio_labels = vec![dialogue_audio];
        for track in project
            .timeline
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Audio && !track.muted)
        {
            let mut clips: Vec<&Clip> = track
                .clips
                .iter()
                .filter(|clip| clip.enabled && clip.asset_id.is_some())
                .collect();
            clips.sort_by_key(|clip| clip.start_ms);
            for clip in clips {
                let asset = asset_for_clip(project, clip)?;
                if asset.channels.unwrap_or(0) == 0 {
                    continue;
                }
                add_input_args(&mut args, asset, clip, project_dir);
                let source_in = clip.source_in_ms as f64 / 1_000.0;
                let source_out = clip.source_out_ms as f64 / 1_000.0;
                let delay = clip.start_ms;
                let label = format!("mix{}", audio_labels.len());
                let mut chain = format!(
                    "[{input_index}:a:0]atrim=start={source_in:.3}:end={source_out:.3},asetpts=PTS-STARTPTS"
                );
                if (clip.speed - 1.0).abs() > f64::EPSILON {
                    chain.push(',');
                    chain.push_str(&atempo_chain(clip.speed));
                }
                chain.push_str(&format!(
                    ",volume={:.4},adelay={delay}|{delay},aresample={sample_rate}[{label}]",
                    clip.volume
                ));
                filters.push(chain);
                audio_labels.push(label);
                input_index += 1;
            }
        }

        let audio_output = if audio_labels.len() == 1 {
            filters.push(format!("[{}]anull[aout]", audio_labels[0]));
            "aout".to_string()
        } else {
            filters.push(format!(
                "{}amix=inputs={}:duration=longest:dropout_transition=0,alimiter=limit=0.98[aout]",
                join_labels(&audio_labels),
                audio_labels.len()
            ));
            "aout".to_string()
        };

        let filter_complex = filters.join(";");
        args.extend([
            "-filter_complex".to_string(),
            filter_complex.clone(),
            "-map".to_string(),
            format!("[{video_output}]"),
            "-map".to_string(),
            format!("[{audio_output}]"),
            "-c:v".to_string(),
            self.options.video_codec.clone(),
            "-preset".to_string(),
            self.options.preset.clone(),
            "-crf".to_string(),
            self.options.crf.to_string(),
            "-c:a".to_string(),
            self.options.audio_codec.clone(),
            "-b:a".to_string(),
            self.options.audio_bitrate.clone(),
            "-pix_fmt".to_string(),
            "yuv420p".to_string(),
            "-movflags".to_string(),
            "+faststart".to_string(),
            "-t".to_string(),
            format!("{:.3}", project.timeline.duration_ms() as f64 / 1_000.0),
            output.to_string_lossy().to_string(),
        ]);

        Ok(RenderCommand {
            program: self.options.ffmpeg.clone(),
            args,
            output,
            filter_complex,
        })
    }

    pub fn render(
        &self,
        project: &Project,
        project_dir: &Path,
        output: impl Into<PathBuf>,
    ) -> Result<RenderCommand> {
        let command = self.build_command(project, project_dir, output)?;
        if let Some(parent) = command.output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let result = Command::new(&command.program)
            .args(&command.args)
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    RustCutError::ExecutableNotFound {
                        program: command.program.clone(),
                    }
                } else {
                    RustCutError::Io(error)
                }
            })?;
        if !result.status.success() {
            return Err(RustCutError::CommandFailed {
                program: command.program.clone(),
                status: result.status.to_string(),
                stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            });
        }
        Ok(command)
    }
}

pub fn export_fcpxml(project: &Project, project_dir: &Path, destination: &Path) -> Result<()> {
    let track = project
        .timeline
        .first_track_of_kind(TrackKind::Video)
        .ok_or_else(|| RustCutError::Validation("timeline has no video track".to_string()))?;
    let fps = project.timeline.settings.fps;
    let frame_duration = if (fps - 29.97).abs() < 0.01 {
        "1001/30000s".to_string()
    } else {
        format!("1/{}s", fps.round() as u32)
    };
    let mut resources = String::new();
    let mut spine = String::new();
    for (index, clip) in track.clips.iter().filter(|clip| clip.enabled).enumerate() {
        let Some(asset_id) = clip.asset_id else {
            continue;
        };
        let asset = project
            .assets
            .get(&asset_id)
            .ok_or_else(|| RustCutError::AssetNotFound(asset_id.to_string()))?;
        let resource_id = format!("r{}", index + 2);
        let path = resolve_asset_path(project_dir, &asset.path);
        resources.push_str(&format!(
            "    <asset id=\"{resource_id}\" name=\"{}\" src=\"file://{}\" start=\"0s\" duration=\"{:.3}s\" hasVideo=\"1\" hasAudio=\"{}\"/>\n",
            xml_escape(&asset.name),
            xml_escape(&path.to_string_lossy()),
            asset.duration_ms as f64 / 1_000.0,
            if asset.channels.unwrap_or(0) > 0 { "1" } else { "0" }
        ));
        spine.push_str(&format!(
            "          <asset-clip name=\"{}\" ref=\"{resource_id}\" offset=\"{:.3}s\" start=\"{:.3}s\" duration=\"{:.3}s\"/>\n",
            xml_escape(&clip.name),
            clip.start_ms as f64 / 1_000.0,
            clip.source_in_ms as f64 / 1_000.0,
            clip.duration_ms() as f64 / 1_000.0
        ));
    }
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.11\">\n  <resources>\n    <format id=\"r1\" name=\"RustCutFormat\" frameDuration=\"{frame_duration}\" width=\"{}\" height=\"{}\"/>\n{resources}  </resources>\n  <library><event name=\"RustCut\"><project name=\"{}\"><sequence format=\"r1\" duration=\"{:.3}s\"><spine>\n{spine}        </spine></sequence></project></event></library>\n</fcpxml>\n",
        project.timeline.settings.width,
        project.timeline.settings.height,
        xml_escape(&project.name),
        project.timeline.duration_ms() as f64 / 1_000.0,
    );
    std::fs::write(destination, document)?;
    Ok(())
}

fn asset_for_clip<'a>(project: &'a Project, clip: &Clip) -> Result<&'a Asset> {
    let asset_id = clip
        .asset_id
        .ok_or_else(|| RustCutError::Validation("media clip has no asset_id".to_string()))?;
    project
        .assets
        .get(&asset_id)
        .ok_or_else(|| RustCutError::AssetNotFound(asset_id.to_string()))
}

fn add_input_args(args: &mut Vec<String>, asset: &Asset, clip: &Clip, project_dir: &Path) {
    if asset.kind == AssetKind::Image {
        args.extend([
            "-loop".to_string(),
            "1".to_string(),
            "-t".to_string(),
            format!("{:.3}", clip.duration_ms().max(1_000) as f64 / 1_000.0),
        ]);
    }
    args.push("-i".to_string());
    args.push(
        resolve_asset_path(project_dir, &asset.path)
            .to_string_lossy()
            .to_string(),
    );
}

fn append_video_effects(chain: &mut String, clip: &Clip, duration_seconds: f64) {
    for effect in &clip.effects {
        match effect {
            Effect::Fade { in_ms, out_ms } => {
                if *in_ms > 0 {
                    chain.push_str(&format!(",fade=t=in:st=0:d={:.3}", *in_ms as f64 / 1_000.0));
                }
                if *out_ms > 0 {
                    let duration = *out_ms as f64 / 1_000.0;
                    let start = (duration_seconds - duration).max(0.0);
                    chain.push_str(&format!(",fade=t=out:st={start:.3}:d={duration:.3}"));
                }
            }
            Effect::Crop {
                x,
                y,
                width,
                height,
            } => {
                chain.push_str(&format!(",crop={width}:{height}:{x}:{y}"));
            }
            Effect::NormalizeAudio { .. } => {}
        }
    }
}

fn append_audio_effects(chain: &mut String, clip: &Clip, duration_seconds: f64) {
    for effect in &clip.effects {
        match effect {
            Effect::Fade { in_ms, out_ms } => {
                if *in_ms > 0 {
                    chain.push_str(&format!(
                        ",afade=t=in:st=0:d={:.3}",
                        *in_ms as f64 / 1_000.0
                    ));
                }
                if *out_ms > 0 {
                    let duration = *out_ms as f64 / 1_000.0;
                    let start = (duration_seconds - duration).max(0.0);
                    chain.push_str(&format!(",afade=t=out:st={start:.3}:d={duration:.3}"));
                }
            }
            Effect::NormalizeAudio { target_lufs } => {
                chain.push_str(&format!(",loudnorm=I={target_lufs}:TP=-1.5:LRA=11"));
            }
            Effect::Crop { .. } => {}
        }
    }
}

fn atempo_chain(speed: f64) -> String {
    let mut remaining = speed;
    let mut filters = Vec::new();
    while remaining > 2.0 {
        filters.push("atempo=2.0".to_string());
        remaining /= 2.0;
    }
    while remaining < 0.5 {
        filters.push("atempo=0.5".to_string());
        remaining /= 0.5;
    }
    filters.push(format!("atempo={remaining:.6}"));
    filters.join(",")
}

fn join_labels(labels: &[String]) -> String {
    labels
        .iter()
        .map(|label| format!("[{label}]"))
        .collect::<String>()
}

fn drawtext_filter(
    text: &crate::TextOverlay,
    start: f64,
    end: f64,
    width: u32,
    height: u32,
) -> String {
    let escaped_text = escape_drawtext(&text.text);
    let (x, y) = text_position(text.position, text.alignment, width, height);
    let mut options = vec![
        format!("text='{escaped_text}'"),
        format!("fontsize={}", text.font_size),
        format!("fontcolor={}", text.color),
        format!("borderw={}", text.outline_width),
        format!("bordercolor={}", text.outline_color),
        format!("x={x}"),
        format!("y={y}"),
        format!("enable='between(t,{start:.3},{end:.3})'"),
    ];
    if let Some(font_file) = &text.font_file {
        options.push(format!("fontfile='{}'", escape_filter_path(font_file)));
    }
    if let Some(box_color) = &text.box_color {
        options.push("box=1".to_string());
        options.push(format!("boxcolor={box_color}"));
        options.push("boxborderw=20".to_string());
    }
    format!("drawtext={}", options.join(":"))
}

fn text_position(
    position: TextPosition,
    alignment: TextAlignment,
    _width: u32,
    height: u32,
) -> (String, String) {
    let x = match alignment {
        TextAlignment::Left => "w*0.08".to_string(),
        TextAlignment::Center => "(w-text_w)/2".to_string(),
        TextAlignment::Right => "w-text_w-w*0.08".to_string(),
    };
    let y = match position {
        TextPosition::Top => "h*0.08".to_string(),
        TextPosition::Center => "(h-text_h)/2".to_string(),
        TextPosition::Bottom => format!("{}-text_h-h*0.08", height),
        TextPosition::LowerThird => "h*0.72".to_string(),
    };
    (x, y)
}

fn overlay_x_expression(clip: &Clip, width: u32) -> String {
    format!(
        "({width}-overlay_w)*{:.4}",
        clip.transform.x.clamp(0.0, 1.0)
    )
}

fn overlay_y_expression(clip: &Clip, height: u32) -> String {
    format!(
        "({height}-overlay_h)*{:.4}",
        clip.transform.y.clamp(0.0, 1.0)
    )
}

fn escape_drawtext(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace('\'', "\\'")
        .replace('%', "\\%")
        .replace('\n', "\\n")
}

fn escape_filter_path(input: &str) -> String {
    input
        .replace('\\', "/")
        .replace(':', "\\:")
        .replace('\'', "\\'")
}

fn shell_quote(input: &str) -> String {
    if input
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-._/:[]=+".contains(character))
    {
        input.to_string()
    } else {
        format!("'{}'", input.replace('\'', "'\\''"))
    }
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_atempo_chain_for_extremes() {
        assert_eq!(atempo_chain(4.0), "atempo=2.0,atempo=2.000000");
        assert_eq!(atempo_chain(0.25), "atempo=0.5,atempo=0.500000");
    }

    #[test]
    fn escapes_drawtext_content() {
        assert_eq!(escape_drawtext("a:b%"), "a\\:b\\%");
    }
}
