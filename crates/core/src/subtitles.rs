use std::path::Path;

use uuid::Uuid;

use crate::{Result, RustCutError, Transcript, TranscriptSegment, TranscriptWord};

pub fn load_transcript_file(path: &Path, asset_id: Uuid) -> Result<Transcript> {
    let content = std::fs::read_to_string(path)?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mut transcript = match extension.as_str() {
        "json" => parse_json_transcript(&content, asset_id)?,
        "srt" => parse_srt(&content, asset_id)?,
        "vtt" => parse_vtt(&content, asset_id)?,
        _ => {
            return Err(RustCutError::Validation(format!(
                "unsupported transcript format: .{extension}"
            )));
        }
    };
    transcript.normalize();
    Ok(transcript)
}

pub fn transcript_to_srt(transcript: &Transcript) -> String {
    let mut output = String::new();
    for (index, segment) in transcript.segments.iter().enumerate() {
        output.push_str(&(index + 1).to_string());
        output.push('\n');
        output.push_str(&format!(
            "{} --> {}\n",
            format_srt_time(segment.start_ms),
            format_srt_time(segment.end_ms)
        ));
        output.push_str(segment.text.trim());
        output.push_str("\n\n");
    }
    output
}

fn parse_json_transcript(content: &str, asset_id: Uuid) -> Result<Transcript> {
    let value: serde_json::Value = serde_json::from_str(content)?;
    if value.get("asset_id").is_some() && value.get("segments").is_some() {
        let mut transcript: Transcript = serde_json::from_value(value)?;
        transcript.asset_id = asset_id;
        return Ok(transcript);
    }

    let language = value
        .get("language")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let segments = value
        .get("segments")
        .and_then(|value| value.as_array())
        .ok_or_else(|| RustCutError::Transcription("JSON is missing segments[]".to_string()))?
        .iter()
        .map(parse_generic_json_segment)
        .collect::<Result<Vec<_>>>()?;

    Ok(Transcript {
        asset_id,
        language,
        segments,
    })
}

fn parse_generic_json_segment(value: &serde_json::Value) -> Result<TranscriptSegment> {
    let start_ms = json_time_to_ms(value, "start_ms", "start")?;
    let end_ms = json_time_to_ms(value, "end_ms", "end")?;
    let text = value
        .get("text")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let words = value
        .get("words")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .map(|word| {
                    Ok(TranscriptWord {
                        start_ms: json_time_to_ms(word, "start_ms", "start")?,
                        end_ms: json_time_to_ms(word, "end_ms", "end")?,
                        text: word
                            .get("word")
                            .or_else(|| word.get("text"))
                            .and_then(|value| value.as_str())
                            .unwrap_or("")
                            .to_string(),
                        confidence: word
                            .get("probability")
                            .or_else(|| word.get("confidence"))
                            .and_then(|value| value.as_f64())
                            .map(|value| value as f32),
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(TranscriptSegment {
        start_ms,
        end_ms,
        text,
        speaker: value
            .get("speaker")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        words,
    })
}

fn json_time_to_ms(value: &serde_json::Value, ms_key: &str, seconds_key: &str) -> Result<u64> {
    if let Some(milliseconds) = value.get(ms_key).and_then(|value| value.as_u64()) {
        return Ok(milliseconds);
    }
    if let Some(seconds) = value.get(seconds_key).and_then(|value| value.as_f64()) {
        return Ok((seconds.max(0.0) * 1_000.0).round() as u64);
    }
    Err(RustCutError::Transcription(format!(
        "missing time field `{ms_key}` or `{seconds_key}`"
    )))
}

fn parse_srt(content: &str, asset_id: Uuid) -> Result<Transcript> {
    let normalized = content.replace("\r\n", "\n");
    let mut segments = Vec::new();
    for block in normalized.split("\n\n") {
        let mut lines = block.lines().filter(|line| !line.trim().is_empty());
        let first = lines.next();
        let timing = match first {
            Some(line) if line.contains("-->") => line,
            Some(_) => lines.next().unwrap_or(""),
            None => continue,
        };
        let Some((start, end)) = timing.split_once("-->") else {
            continue;
        };
        let text = lines.collect::<Vec<_>>().join("\n");
        if text.trim().is_empty() {
            continue;
        }
        segments.push(TranscriptSegment {
            start_ms: parse_subtitle_time(start.trim())?,
            end_ms: parse_subtitle_time(end.split_whitespace().next().unwrap_or(""))?,
            text,
            speaker: None,
            words: Vec::new(),
        });
    }
    Ok(Transcript {
        asset_id,
        language: None,
        segments,
    })
}

fn parse_vtt(content: &str, asset_id: Uuid) -> Result<Transcript> {
    let content = content
        .replace("\r\n", "\n")
        .trim_start_matches("WEBVTT")
        .trim()
        .to_string();
    parse_srt(&content, asset_id)
}

fn parse_subtitle_time(input: &str) -> Result<u64> {
    let clean = input.replace(',', ".");
    let parts: Vec<&str> = clean.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [hours, minutes, seconds] => (*hours, *minutes, *seconds),
        [minutes, seconds] => ("0", *minutes, *seconds),
        _ => {
            return Err(RustCutError::Transcription(format!(
                "invalid subtitle timestamp: {input}"
            )));
        }
    };
    let hours: u64 = hours
        .parse()
        .map_err(|_| RustCutError::Transcription(format!("invalid hours: {input}")))?;
    let minutes: u64 = minutes
        .parse()
        .map_err(|_| RustCutError::Transcription(format!("invalid minutes: {input}")))?;
    let seconds: f64 = seconds
        .parse()
        .map_err(|_| RustCutError::Transcription(format!("invalid seconds: {input}")))?;
    Ok(hours * 3_600_000 + minutes * 60_000 + (seconds * 1_000.0).round() as u64)
}

fn format_srt_time(milliseconds: u64) -> String {
    let hours = milliseconds / 3_600_000;
    let remainder = milliseconds % 3_600_000;
    let minutes = remainder / 60_000;
    let remainder = remainder % 60_000;
    let seconds = remainder / 1_000;
    let millis = remainder % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt() {
        let transcript = parse_srt(
            "1\n00:00:01,000 --> 00:00:02,500\nHello world\n",
            Uuid::nil(),
        )
        .unwrap();
        assert_eq!(transcript.segments[0].start_ms, 1_000);
        assert_eq!(transcript.segments[0].end_ms, 2_500);
    }

    #[test]
    fn writes_srt() {
        let transcript = Transcript {
            asset_id: Uuid::nil(),
            language: None,
            segments: vec![TranscriptSegment {
                start_ms: 1_000,
                end_ms: 2_500,
                text: "Hello".to_string(),
                speaker: None,
                words: vec![],
            }],
        };
        assert!(transcript_to_srt(&transcript).contains("00:00:01,000 --> 00:00:02,500"));
    }
}
