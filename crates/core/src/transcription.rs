use std::path::Path;

use async_trait::async_trait;
use reqwest::{Body, Client, multipart};
use serde::Deserialize;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{Result, RustCutError, Transcript, TranscriptSegment, TranscriptWord};

#[async_trait]
pub trait Transcriber: Send + Sync {
    async fn transcribe(&self, asset_id: Uuid, media_path: &Path) -> Result<Transcript>;
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleTranscriber {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    language: Option<String>,
}

impl OpenAiCompatibleTranscriber {
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
            language: None,
        }
    }

    pub fn with_language(mut self, language: Option<String>) -> Self {
        self.language = language;
        self
    }
}

#[async_trait]
impl Transcriber for OpenAiCompatibleTranscriber {
    async fn transcribe(&self, asset_id: Uuid, media_path: &Path) -> Result<Transcript> {
        let file = tokio::fs::File::open(media_path).await?;
        let file_length = file.metadata().await?.len();
        let file_name = media_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("media.bin")
            .to_string();
        let file_part = multipart::Part::stream_with_length(
            Body::wrap_stream(ReaderStream::new(file)),
            file_length,
        )
        .file_name(file_name)
        .mime_str("application/octet-stream")
        .map_err(|error| RustCutError::Transcription(error.to_string()))?;
        let mut form = multipart::Form::new()
            .text("model", self.model.clone())
            .text("response_format", "verbose_json")
            .text("timestamp_granularities[]", "segment")
            .text("timestamp_granularities[]", "word")
            .part("file", file_part);
        if let Some(language) = &self.language {
            form = form.text("language", language.clone());
        }

        let response = self
            .client
            .post(format!("{}/audio/transcriptions", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?
            .error_for_status()?;
        let payload: TranscriptionResponse = response.json().await?;
        let mut segments = payload
            .segments
            .unwrap_or_default()
            .into_iter()
            .map(|segment| TranscriptSegment {
                start_ms: seconds_to_ms(segment.start),
                end_ms: seconds_to_ms(segment.end),
                text: segment.text,
                speaker: segment.speaker,
                words: Vec::new(),
            })
            .collect::<Vec<_>>();

        let words = payload.words.unwrap_or_default();
        for word in words {
            let item = TranscriptWord {
                start_ms: seconds_to_ms(word.start),
                end_ms: seconds_to_ms(word.end),
                text: word.word,
                confidence: word.probability,
            };
            if let Some(segment) = segments
                .iter_mut()
                .find(|segment| item.start_ms >= segment.start_ms && item.start_ms < segment.end_ms)
            {
                segment.words.push(item);
            }
        }

        if segments.is_empty() && !payload.text.trim().is_empty() {
            segments.push(TranscriptSegment {
                start_ms: 0,
                end_ms: 0,
                text: payload.text,
                speaker: None,
                words: Vec::new(),
            });
        }

        Ok(Transcript {
            asset_id,
            language: payload.language,
            segments,
        })
    }
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    #[serde(default)]
    text: String,
    language: Option<String>,
    segments: Option<Vec<ApiSegment>>,
    words: Option<Vec<ApiWord>>,
}

#[derive(Debug, Deserialize)]
struct ApiSegment {
    start: f64,
    end: f64,
    text: String,
    speaker: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiWord {
    start: f64,
    end: f64,
    #[serde(alias = "text")]
    word: String,
    probability: Option<f32>,
}

fn seconds_to_ms(seconds: f64) -> u64 {
    (seconds.max(0.0) * 1_000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::{
        Json, Router,
        extract::{DefaultBodyLimit, Multipart},
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use serde_json::{Value, json};

    use super::*;

    async fn transcription_endpoint(
        headers: HeaderMap,
        mut multipart: Multipart,
    ) -> (StatusCode, Json<Value>) {
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test-key")
        );

        let mut text_fields: HashMap<String, Vec<String>> = HashMap::new();
        let mut file_length = 0;
        while let Some(field) = multipart.next_field().await.unwrap() {
            let name = field.name().unwrap().to_string();
            if name == "file" {
                assert_eq!(field.file_name(), Some("sample.bin"));
                file_length = field.bytes().await.unwrap().len();
            } else {
                text_fields
                    .entry(name)
                    .or_default()
                    .push(field.text().await.unwrap());
            }
        }

        assert_eq!(file_length, 2 * 1024 * 1024);
        assert_eq!(text_fields["model"], ["test-model"]);
        assert_eq!(text_fields["response_format"], ["verbose_json"]);
        assert_eq!(
            text_fields["timestamp_granularities[]"],
            ["segment", "word"]
        );
        assert_eq!(text_fields["language"], ["zh"]);

        (
            StatusCode::OK,
            Json(json!({
                "text": "hello",
                "language": "zh",
                "segments": [{
                    "start": 0.25,
                    "end": 1.5,
                    "text": "hello",
                    "speaker": "speaker-1"
                }],
                "words": [{
                    "start": 0.25,
                    "end": 0.75,
                    "word": "hello",
                    "probability": 0.98
                }]
            })),
        )
    }

    #[tokio::test]
    async fn streams_openai_compatible_multipart_and_maps_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/audio/transcriptions", post(transcription_endpoint))
                    .layer(DefaultBodyLimit::max(3 * 1024 * 1024)),
            )
            .await
            .unwrap();
        });
        let media_dir =
            std::env::temp_dir().join(format!("rustcut-transcription-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&media_dir).await.unwrap();
        let media_path = media_dir.join("sample.bin");
        tokio::fs::write(&media_path, vec![7_u8; 2 * 1024 * 1024])
            .await
            .unwrap();

        let asset_id = Uuid::new_v4();
        let transcript =
            OpenAiCompatibleTranscriber::new(format!("http://{address}"), "test-key", "test-model")
                .with_language(Some("zh".to_string()))
                .transcribe(asset_id, &media_path)
                .await
                .unwrap();

        tokio::fs::remove_dir_all(media_dir).await.unwrap();
        server.abort();

        assert_eq!(transcript.asset_id, asset_id);
        assert_eq!(transcript.language.as_deref(), Some("zh"));
        assert_eq!(transcript.segments[0].start_ms, 250);
        assert_eq!(transcript.segments[0].words[0].text, "hello");
    }
}
