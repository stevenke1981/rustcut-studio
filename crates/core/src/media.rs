use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{Asset, AssetKind, Result, RustCutError};

#[derive(Debug, Clone)]
pub struct MediaProbe {
    ffprobe: String,
}

impl MediaProbe {
    pub fn new(ffprobe: impl Into<String>) -> Self {
        Self {
            ffprobe: ffprobe.into(),
        }
    }

    pub fn probe(&self, path: &Path) -> Result<ProbeResult> {
        if !path.exists() {
            return Err(RustCutError::Validation(format!(
                "media path does not exist: {}",
                path.display()
            )));
        }
        let output = Command::new(&self.ffprobe)
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(path)
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    RustCutError::ExecutableNotFound {
                        program: self.ffprobe.clone(),
                    }
                } else {
                    RustCutError::Io(error)
                }
            })?;

        if !output.status.success() {
            return Err(RustCutError::CommandFailed {
                program: self.ffprobe.clone(),
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        let response: FfprobeResponse = serde_json::from_slice(&output.stdout)?;
        Ok(ProbeResult::from_ffprobe(response))
    }
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub kind: AssetKind,
    pub duration_ms: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
}

impl ProbeResult {
    fn from_ffprobe(response: FfprobeResponse) -> Self {
        let video = response
            .streams
            .iter()
            .find(|stream| stream.codec_type.as_deref() == Some("video"));
        let audio = response
            .streams
            .iter()
            .find(|stream| stream.codec_type.as_deref() == Some("audio"));
        let duration = response
            .format
            .as_ref()
            .and_then(|format| format.duration.as_deref())
            .and_then(|value| value.parse::<f64>().ok())
            .or_else(|| {
                response
                    .streams
                    .iter()
                    .filter_map(|stream| stream.duration.as_deref())
                    .filter_map(|value| value.parse::<f64>().ok())
                    .max_by(f64::total_cmp)
            })
            .unwrap_or(0.0);
        let kind = match (video, audio) {
            (Some(video), _) if duration <= 0.05 && video.nb_frames.as_deref() == Some("1") => {
                AssetKind::Image
            }
            (Some(_), _) => AssetKind::Video,
            (None, Some(_)) => AssetKind::Audio,
            _ => AssetKind::Unknown,
        };
        Self {
            kind,
            duration_ms: (duration.max(0.0) * 1_000.0).round() as u64,
            width: video.and_then(|stream| stream.width),
            height: video.and_then(|stream| stream.height),
            fps: video
                .and_then(|stream| stream.avg_frame_rate.as_deref())
                .and_then(parse_fraction),
            sample_rate: audio
                .and_then(|stream| stream.sample_rate.as_deref())
                .and_then(|value| value.parse().ok()),
            channels: audio.and_then(|stream| stream.channels),
        }
    }
}

#[derive(Debug, Deserialize)]
struct FfprobeResponse {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u16>,
    duration: Option<String>,
    nb_frames: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

pub fn import_asset(
    probe: &MediaProbe,
    source: &Path,
    project_dir: &Path,
    copy_into_project: bool,
) -> Result<Asset> {
    let mut metadata = probe.probe(source)?;
    if metadata.kind == AssetKind::Video && metadata.duration_ms == 0 && is_still_image_path(source)
    {
        metadata.kind = AssetKind::Image;
    }
    let asset_id = Uuid::new_v4();
    let original_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("media.bin");
    let safe_name = sanitize_file_name(original_name);
    let relative_path = if copy_into_project {
        let assets_dir = project_dir.join("assets");
        std::fs::create_dir_all(&assets_dir)?;
        let destination = assets_dir.join(format!("{}_{}", asset_id, safe_name));
        copy_file(source, &destination)?;
        destination
            .strip_prefix(project_dir)
            .map_err(|_| RustCutError::UnsafePath(destination.clone()))?
            .to_path_buf()
    } else {
        source.canonicalize()?
    };

    Ok(Asset {
        id: asset_id,
        name: original_name.to_string(),
        path: path_to_storage_string(&relative_path),
        kind: metadata.kind,
        duration_ms: metadata.duration_ms,
        width: metadata.width,
        height: metadata.height,
        fps: metadata.fps,
        sample_rate: metadata.sample_rate,
        channels: metadata.channels,
        sha256: sha256_file(source)?,
        created_at: Utc::now(),
    })
}

pub fn resolve_asset_path(project_dir: &Path, stored_path: &str) -> PathBuf {
    let path = PathBuf::from(stored_path);
    if path.is_absolute() {
        path
    } else {
        project_dir.join(path)
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    // Keep the large read buffer off the relatively small Windows main-thread stack.
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    let mut input = File::open(source)?;
    let mut output = File::create(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.flush()?;
    Ok(())
}

fn is_still_image_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "tif" | "tiff" | "avif")
    )
}

fn parse_fraction(input: &str) -> Option<f64> {
    let (numerator, denominator) = input.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    if denominator == 0.0 {
        None
    } else {
        Some(numerator / denominator)
    }
}

fn sanitize_file_name(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "media.bin".to_string()
    } else {
        sanitized
    }
}

fn path_to_storage_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_frame_rate() {
        let rate = parse_fraction("30000/1001").unwrap();
        assert!((rate - 29.970).abs() < 0.001);
    }

    #[test]
    fn sanitizes_file_names() {
        assert_eq!(sanitize_file_name("my clip (1).mp4"), "my_clip__1_.mp4");
    }

    #[test]
    fn hashes_files_without_large_stack_allocation() {
        let path = std::env::temp_dir().join(format!("rustcut-sha256-{}.txt", Uuid::new_v4()));
        std::fs::write(&path, b"abc").unwrap();

        let digest = sha256_file(&path).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
