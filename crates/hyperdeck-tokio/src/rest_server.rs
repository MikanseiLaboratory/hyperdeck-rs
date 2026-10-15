//! REST server over the same in-memory deck the Ethernet server can use.
//!
//! The routes follow the December 2024 manual. Monitoring routes that do not
//! affect transport return fixed, documented defaults so a client can probe
//! them. Transport, timeline, clips, and system routes read and write
//! [`DeckModel`].

use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use hyperdeck_core::rest::{
    ActiveMedia, AudioFormat, Clip, CodecFormat, DeviceState, PlaybackState, ProductInfo,
    RecordState, TimecodePair, Timeline, TimelineClip, TransportMode, VideoFormatInfo, WorkingSet,
    WorkingSetItem,
};
use serde_json::{Value, json};

/// Shared deck state for the REST server and, when wanted, a UI.
#[derive(Debug, Clone)]
pub struct DeckModel {
    /// `InputPreview`, `InputRecord`, or `Output`.
    pub mode: String,
    /// Playback speed. `1.0` is normal play.
    pub speed: f64,
    /// Timeline position in frames.
    pub position: i64,
    /// Loop the timeline.
    pub loop_playback: bool,
    /// Loop the current clip.
    pub single_clip: bool,
    /// Display timecode.
    pub display_timecode: String,
    /// Timeline timecode.
    pub timeline_timecode: String,
    /// Clips on the active disk.
    pub clips: Vec<Clip>,
    /// Timeline entries.
    pub timeline: Vec<TimelineClip>,
    /// Active working-set index.
    pub active_index: u32,
    /// Codec token.
    pub codec: String,
    /// Container token.
    pub container: String,
    /// REST video-format name.
    pub video_format: String,
    /// Product name.
    pub product_name: String,
    /// User-visible device name.
    pub device_name: String,
    /// Software version.
    pub software_version: String,
    /// Record audio codec.
    pub audio_codec: String,
    /// Record audio channel count.
    pub audio_channels: u32,
}

impl DeckModel {
    /// A stopped deck with the two demo clips.
    pub fn demo() -> Self {
        let clip = |id: u32, path: &str, frames: u64| Clip {
            clip_unique_id: id,
            file_path: path.into(),
            file_size: 1_000_000,
            codec_format: Some(CodecFormat {
                codec: "ProRes:HQ".into(),
                container: Some("MOV".into()),
            }),
            video_format: Some(VideoFormatInfo {
                name: "1920x1080p29.97".into(),
                frame_rate: Some("29.97".into()),
                height: Some(1080),
                width: Some(1920),
                interlaced: Some(false),
            }),
            start_timecode: "00:00:00:00".into(),
            duration_timecode: "00:00:10:00".into(),
            frame_count: frames,
        };
        Self {
            mode: "Output".into(),
            speed: 0.0,
            position: 0,
            loop_playback: false,
            single_clip: false,
            display_timecode: "00:00:00:00".into(),
            timeline_timecode: "00:00:00:00".into(),
            clips: vec![clip(1, "A.mov", 300), clip(2, "B.mov", 150)],
            timeline: vec![timeline_clip(1, 300), timeline_clip(2, 150)],
            active_index: 0,
            codec: "ProRes:HQ".into(),
            container: "MOV".into(),
            video_format: "1920x1080p29.97".into(),
            product_name: "HyperDeck Rust".into(),
            device_name: "Mock HyperDeck".into(),
            software_version: "0.1.0".into(),
            audio_codec: "PCM".into(),
            audio_channels: 16,
        }
    }
}

fn timeline_clip(id: u32, frames: u64) -> TimelineClip {
    TimelineClip {
        clip_unique_id: id,
        frame_count: frames,
        duration_timecode: "00:00:10:00".into(),
        clip_in: "0".into(),
        in_timecode: "00:00:00:00".into(),
        timeline_in: "0".into(),
        timeline_in_timecode: "00:00:00:00".into(),
    }
}

/// Axum state.
#[derive(Clone)]
pub struct AppState {
    /// Deck model. Poisoned locks recover the inner value.
    pub model: Arc<Mutex<DeckModel>>,
}

impl AppState {
    /// Demo deck.
    pub fn demo() -> Self {
        Self {
            model: Arc::new(Mutex::new(DeckModel::demo())),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DeckModel> {
        self.model
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

/// Serves [`router`] until the listener closes.
pub async fn serve_rest(listener: tokio::net::TcpListener, state: AppState) -> std::io::Result<()> {
    axum::serve(listener, router(state))
        .await
        .map_err(std::io::Error::other)
}

/// Router mounted at the API root (`/transports/0`, not `/control/api/v1`).
///
/// Put it behind a nest if you want the prefix the hardware uses:
/// `Router::new().nest("/control/api/v1", router(state))`.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/transports/0", get(get_transport).put(put_transport))
        .route("/transports/0/stop", post(post_stop).get(get_stopped))
        .route("/transports/0/play", post(post_play).get(get_playing))
        .route(
            "/transports/0/playback",
            get(get_playback).put(put_playback),
        )
        .route("/transports/0/record", get(get_record).post(post_record))
        .route("/transports/0/timecode", get(get_timecode))
        .route("/transports/0/clipIndex", get(get_clip_index))
        .route("/system/product", get(get_product))
        .route("/system/codecFormat", get(get_codec).put(put_codec))
        .route("/system/videoFormat", get(get_video_format))
        .route("/system/supportedVideoFormats", get(supported_video))
        .route("/system/supportedCodecFormats", get(supported_codecs))
        .route("/media/workingset", get(get_workingset))
        .route("/media/active", get(get_active).put(put_active))
        .route("/media/devices/{name}", get(get_device))
        .route("/clips", get(get_clips))
        .route("/timelines/0", get(get_timeline).post(post_timeline))
        .route("/timelines/0/clear", post(clear_timeline))
        .route("/timelines/0/clips/{index}", delete(delete_timeline_clip))
        .route("/audio/recordFormat", get(get_audio).put(put_audio))
        .route("/event/list", get(event_list))
        .route("/monitoring/display", get(displays))
        .with_state(state)
}

async fn get_transport(State(state): State<AppState>) -> Json<TransportMode> {
    Json(TransportMode {
        mode: state.lock().mode.clone(),
    })
}

async fn put_transport(
    State(state): State<AppState>,
    Json(body): Json<TransportMode>,
) -> StatusCode {
    state.lock().mode = body.mode;
    StatusCode::NO_CONTENT
}

async fn post_stop(State(state): State<AppState>) -> StatusCode {
    let mut model = state.lock();
    model.mode = "Output".into();
    model.speed = 0.0;
    StatusCode::NO_CONTENT
}

async fn get_stopped(State(state): State<AppState>) -> Json<bool> {
    let model = state.lock();
    Json(model.speed == 0.0 || model.mode == "InputPreview")
}

async fn post_play(State(state): State<AppState>) -> StatusCode {
    let mut model = state.lock();
    model.mode = "Output".into();
    model.speed = 1.0;
    StatusCode::NO_CONTENT
}

async fn get_playing(State(state): State<AppState>) -> Json<bool> {
    let model = state.lock();
    Json(model.mode == "Output" && model.speed != 0.0)
}

async fn get_playback(State(state): State<AppState>) -> Json<PlaybackState> {
    let model = state.lock();
    Json(PlaybackState {
        kind: "Play".into(),
        loop_playback: model.loop_playback,
        single_clip: model.single_clip,
        speed: model.speed,
        position: model.position,
    })
}

async fn put_playback(
    State(state): State<AppState>,
    Json(body): Json<PlaybackState>,
) -> StatusCode {
    let mut model = state.lock();
    model.loop_playback = body.loop_playback;
    model.single_clip = body.single_clip;
    model.speed = body.speed;
    model.position = body.position;
    model.mode = "Output".into();
    StatusCode::NO_CONTENT
}

async fn get_record(State(state): State<AppState>) -> Json<RecordState> {
    Json(RecordState {
        recording: state.lock().mode == "InputRecord",
    })
}

async fn post_record(State(state): State<AppState>, body: Option<Json<Value>>) -> StatusCode {
    let mut model = state.lock();
    model.mode = "InputRecord".into();
    model.speed = 0.0;
    if let Some(Json(value)) = body
        && let Some(name) = value.get("clipName").and_then(|v| v.as_str())
    {
        let id = model.clips.len() as u32 + 1;
        let start_timecode = model.timeline_timecode.clone();
        model.clips.push(Clip {
            clip_unique_id: id,
            file_path: name.into(),
            file_size: 0,
            codec_format: None,
            video_format: None,
            start_timecode,
            duration_timecode: "00:00:00:00".into(),
            frame_count: 0,
        });
    }
    StatusCode::NO_CONTENT
}

async fn get_timecode(State(state): State<AppState>) -> Json<TimecodePair> {
    let model = state.lock();
    Json(TimecodePair {
        display: model.display_timecode.clone(),
        timeline: model.timeline_timecode.clone(),
    })
}

async fn get_clip_index(State(state): State<AppState>) -> Json<Value> {
    let model = state.lock();
    let index = if model.timeline.is_empty() {
        Value::Null
    } else {
        json!(0)
    };
    Json(json!({ "clipIndex": index }))
}

async fn get_product(State(state): State<AppState>) -> Json<ProductInfo> {
    let model = state.lock();
    Json(ProductInfo {
        device_name: model.device_name.clone(),
        product_name: model.product_name.clone(),
        software_version: model.software_version.clone(),
    })
}

async fn get_codec(State(state): State<AppState>) -> Json<CodecFormat> {
    let model = state.lock();
    Json(CodecFormat {
        codec: model.codec.clone(),
        container: Some(model.container.clone()),
    })
}

async fn put_codec(State(state): State<AppState>, Json(body): Json<CodecFormat>) -> StatusCode {
    let mut model = state.lock();
    model.codec = body.codec;
    if let Some(container) = body.container {
        model.container = container;
    }
    StatusCode::NO_CONTENT
}

async fn get_video_format(State(state): State<AppState>) -> Json<VideoFormatInfo> {
    Json(VideoFormatInfo {
        name: state.lock().video_format.clone(),
        frame_rate: Some("29.97".into()),
        height: Some(1080),
        width: Some(1920),
        interlaced: Some(false),
    })
}

async fn supported_video() -> Json<Value> {
    Json(json!({
        "formats": [{ "name": "1920x1080p29.97", "frameRate": "29.97", "height": 1080, "width": 1920, "interlaced": false }]
    }))
}

async fn supported_codecs() -> Json<Value> {
    Json(json!({ "codecs": [{ "codec": "ProRes:HQ", "container": "MOV" }] }))
}

async fn get_workingset(State(state): State<AppState>) -> Json<WorkingSet> {
    let model = state.lock();
    Json(WorkingSet {
        size: 1,
        workingset: vec![Some(WorkingSetItem {
            index: model.active_index,
            active_disk: true,
            volume: "Media".into(),
            device_name: "ssd1".into(),
            remaining_record_time: 3600,
            total_space: 2_000_000_000,
            remaining_space: 1_000_000_000,
            clip_count: model.clips.len() as u32,
        })],
    })
}

async fn get_active(State(state): State<AppState>) -> Json<ActiveMedia> {
    Json(ActiveMedia {
        workingset_index: state.lock().active_index,
        device_name: "ssd1".into(),
    })
}

async fn put_active(State(state): State<AppState>, Json(body): Json<ActiveMedia>) -> StatusCode {
    state.lock().active_index = body.workingset_index;
    StatusCode::NO_CONTENT
}

async fn get_device(Path(_name): Path<String>) -> Json<DeviceState> {
    Json(DeviceState {
        state: "Mounted".into(),
    })
}

async fn get_clips(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "clips": state.lock().clips }))
}

async fn get_timeline(State(state): State<AppState>) -> Json<Timeline> {
    Json(Timeline {
        clips: state.lock().timeline.clone(),
    })
}

async fn post_timeline(State(state): State<AppState>, Json(body): Json<Value>) -> StatusCode {
    let mut model = state.lock();
    if let Some(ids) = body.get("clips").and_then(|v| v.as_array()) {
        for id in ids {
            if let Some(id) = id.as_u64() {
                model.timeline.push(timeline_clip(id as u32, 300));
            }
        }
    }
    StatusCode::NO_CONTENT
}

async fn clear_timeline(State(state): State<AppState>) -> StatusCode {
    state.lock().timeline.clear();
    StatusCode::NO_CONTENT
}

async fn delete_timeline_clip(
    State(state): State<AppState>,
    Path(index): Path<usize>,
) -> StatusCode {
    let mut model = state.lock();
    if index < model.timeline.len() {
        model.timeline.remove(index);
        StatusCode::NO_CONTENT
    } else {
        StatusCode::BAD_REQUEST
    }
}

async fn get_audio(State(state): State<AppState>) -> Json<AudioFormat> {
    let model = state.lock();
    Json(AudioFormat {
        codec: model.audio_codec.clone(),
        num_channels: model.audio_channels,
    })
}

async fn put_audio(State(state): State<AppState>, Json(body): Json<AudioFormat>) -> StatusCode {
    let mut model = state.lock();
    model.audio_codec = body.codec;
    model.audio_channels = body.num_channels;
    StatusCode::NO_CONTENT
}

async fn event_list() -> Json<Value> {
    Json(json!({
        "events": ["/transports/0", "/transports/0/playback", "/timelines/0", "/media/active"]
    }))
}

async fn displays() -> Json<Value> {
    Json(json!({ "displays": ["LCD"] }))
}
