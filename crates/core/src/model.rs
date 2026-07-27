use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type Millis = u64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revision: u64,
    pub assets: BTreeMap<Uuid, Asset>,
    pub transcripts: BTreeMap<Uuid, Transcript>,
    pub timeline: Timeline,
    #[serde(default)]
    pub undo_stack: Vec<Timeline>,
    #[serde(default)]
    pub redo_stack: Vec<Timeline>,
}

impl Project {
    pub fn new(name: impl Into<String>, settings: TimelineSettings) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            created_at: now,
            updated_at: now,
            revision: 0,
            assets: BTreeMap::new(),
            transcripts: BTreeMap::new(),
            timeline: Timeline::new(settings),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn snapshot(&mut self) {
        self.undo_stack.push(self.timeline.clone());
        self.redo_stack.clear();
        self.revision += 1;
        self.updated_at = Utc::now();
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(self.timeline.clone());
        self.timeline = previous;
        self.revision += 1;
        self.updated_at = Utc::now();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push(self.timeline.clone());
        self.timeline = next;
        self.revision += 1;
        self.updated_at = Utc::now();
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimelineSettings {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub sample_rate: u32,
    pub background: String,
}

impl Default for TimelineSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30.0,
            sample_rate: 48_000,
            background: "#000000".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Timeline {
    pub settings: TimelineSettings,
    pub tracks: Vec<Track>,
}

impl Timeline {
    pub fn new(settings: TimelineSettings) -> Self {
        Self {
            settings,
            tracks: vec![
                Track::new("V1", TrackKind::Video, 0),
                Track::new("A1", TrackKind::Audio, 1),
                Track::new("C1", TrackKind::Caption, 2),
                Track::new("G1", TrackKind::Graphic, 3),
            ],
        }
    }

    pub fn duration_ms(&self) -> Millis {
        self.tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(Clip::end_ms)
            .max()
            .unwrap_or(0)
    }

    pub fn track(&self, id: Uuid) -> Option<&Track> {
        self.tracks.iter().find(|track| track.id == id)
    }

    pub fn track_mut(&mut self, id: Uuid) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|track| track.id == id)
    }

    pub fn first_track_of_kind(&self, kind: TrackKind) -> Option<&Track> {
        self.tracks
            .iter()
            .filter(|track| track.kind == kind)
            .min_by_key(|track| track.order)
    }

    pub fn first_track_of_kind_mut(&mut self, kind: TrackKind) -> Option<&mut Track> {
        self.tracks
            .iter_mut()
            .filter(|track| track.kind == kind)
            .min_by_key(|track| track.order)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Video,
    Audio,
    Image,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Asset {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub kind: AssetKind,
    pub duration_ms: Millis,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub sha256: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transcript {
    pub asset_id: Uuid,
    pub language: Option<String>,
    pub segments: Vec<TranscriptSegment>,
}

impl Transcript {
    pub fn normalize(&mut self) {
        self.segments.sort_by_key(|segment| segment.start_ms);
        for segment in &mut self.segments {
            segment.words.sort_by_key(|word| word.start_ms);
        }
    }

    pub fn words(&self) -> impl Iterator<Item = &TranscriptWord> {
        self.segments
            .iter()
            .flat_map(|segment| segment.words.iter())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptSegment {
    pub start_ms: Millis,
    pub end_ms: Millis,
    pub text: String,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub words: Vec<TranscriptWord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptWord {
    pub start_ms: Millis,
    pub end_ms: Millis,
    pub text: String,
    #[serde(default)]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub id: Uuid,
    pub name: String,
    pub kind: TrackKind,
    pub order: i32,
    pub muted: bool,
    pub locked: bool,
    pub clips: Vec<Clip>,
}

impl Track {
    pub fn new(name: impl Into<String>, kind: TrackKind, order: i32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind,
            order,
            muted: false,
            locked: false,
            clips: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
    Caption,
    Graphic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Clip {
    pub id: Uuid,
    pub name: String,
    pub asset_id: Option<Uuid>,
    pub start_ms: Millis,
    pub source_in_ms: Millis,
    pub source_out_ms: Millis,
    pub speed: f64,
    pub volume: f32,
    pub opacity: f32,
    pub enabled: bool,
    pub transform: Transform,
    pub text: Option<TextOverlay>,
    #[serde(default)]
    pub effects: Vec<Effect>,
}

impl Clip {
    pub fn media(
        name: impl Into<String>,
        asset_id: Uuid,
        duration_ms: Millis,
        start_ms: Millis,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            asset_id: Some(asset_id),
            start_ms,
            source_in_ms: 0,
            source_out_ms: duration_ms,
            speed: 1.0,
            volume: 1.0,
            opacity: 1.0,
            enabled: true,
            transform: Transform::default(),
            text: None,
            effects: Vec::new(),
        }
    }

    pub fn text(
        name: impl Into<String>,
        text: TextOverlay,
        start_ms: Millis,
        end_ms: Millis,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            asset_id: None,
            start_ms,
            source_in_ms: 0,
            source_out_ms: end_ms.saturating_sub(start_ms),
            speed: 1.0,
            volume: 0.0,
            opacity: 1.0,
            enabled: true,
            transform: Transform::default(),
            text: Some(text),
            effects: Vec::new(),
        }
    }

    pub fn source_duration_ms(&self) -> Millis {
        self.source_out_ms.saturating_sub(self.source_in_ms)
    }

    pub fn duration_ms(&self) -> Millis {
        if self.speed <= 0.0 {
            return self.source_duration_ms();
        }
        (self.source_duration_ms() as f64 / self.speed).round() as Millis
    }

    pub fn end_ms(&self) -> Millis {
        self.start_ms.saturating_add(self.duration_ms())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation_degrees: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_degrees: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextOverlay {
    pub text: String,
    pub font_file: Option<String>,
    pub font_size: u32,
    pub color: String,
    pub outline_color: String,
    pub outline_width: u32,
    pub box_color: Option<String>,
    pub position: TextPosition,
    pub alignment: TextAlignment,
}

impl Default for TextOverlay {
    fn default() -> Self {
        Self {
            text: String::new(),
            font_file: None,
            font_size: 64,
            color: "white".to_string(),
            outline_color: "black".to_string(),
            outline_width: 4,
            box_color: None,
            position: TextPosition::Bottom,
            alignment: TextAlignment::Center,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "snake_case")]
pub enum TextPosition {
    Top,
    Center,
    Bottom,
    LowerThird,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    Fade {
        in_ms: Millis,
        out_ms: Millis,
    },
    NormalizeAudio {
        target_lufs: f32,
    },
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}
