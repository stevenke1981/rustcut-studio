use std::{collections::HashMap, path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Response, StatusCode, header},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use clap::Parser;
use parking_lot::RwLock;
use rustcut_core::{
    EditCommand, EditPlan, EditorService, FsProjectStore, MediaProbe, OpenAiCompatiblePlanner,
    Planner, Project, RenderOptions, Renderer, RulePlanner, RustCutError, TimelineSettings,
    resolve_asset_path,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{error, info};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "rustcut-server", version)]
struct Args {
    #[arg(long, env = "RUSTCUT_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,

    #[arg(long, env = "RUSTCUT_BIND", default_value = "127.0.0.1:8787")]
    bind: String,

    #[arg(long, env = "RUSTCUT_FFMPEG", default_value = "ffmpeg")]
    ffmpeg: String,

    #[arg(long, env = "RUSTCUT_FFPROBE", default_value = "ffprobe")]
    ffprobe: String,
}

#[derive(Clone)]
struct AppState {
    service: Arc<EditorService>,
    jobs: Arc<RwLock<HashMap<Uuid, JobRecord>>>,
    llm_planner: Option<Arc<OpenAiCompatiblePlanner>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustcut=info,tower_http=info".into()),
        )
        .init();
    let args = Args::parse();
    let render_options = RenderOptions {
        ffmpeg: args.ffmpeg,
        ..RenderOptions::default()
    };
    let service = Arc::new(EditorService::new(
        FsProjectStore::new(args.data_dir),
        MediaProbe::new(args.ffprobe),
        Renderer::new(render_options),
    ));
    service.initialize()?;
    let llm_planner = load_llm_planner();
    let state = AppState {
        service,
        jobs: Arc::new(RwLock::new(HashMap::new())),
        llm_planner,
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/api/health", get(health))
        .route("/api/projects", get(list_projects).post(create_project))
        .route(
            "/api/projects/{project_id}",
            get(get_project).delete(delete_project),
        )
        .route(
            "/api/projects/{project_id}/assets/import",
            post(import_media),
        )
        .route(
            "/api/projects/{project_id}/timeline/add",
            post(add_to_timeline),
        )
        .route(
            "/api/projects/{project_id}/transcripts/import",
            post(import_transcript),
        )
        .route("/api/projects/{project_id}/plan", post(plan_prompt))
        .route("/api/projects/{project_id}/prompt", post(apply_prompt))
        .route("/api/projects/{project_id}/apply", post(apply_commands))
        .route("/api/projects/{project_id}/undo", post(undo))
        .route("/api/projects/{project_id}/redo", post(redo))
        .route("/api/projects/{project_id}/render", post(start_render))
        .route(
            "/api/projects/{project_id}/assets/{asset_id}/file",
            get(media_file),
        )
        .route("/api/jobs/{job_id}", get(get_job))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    info!(address = %args.bind, "RustCut server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../../../web/index.html"))
}

async fn app_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../../web/app.js"),
    )
}

async fn style_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../../web/style.css"),
    )
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "llm_planner": state.llm_planner.is_some()
    }))
}

async fn list_projects(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let projects = state.service.list_projects()?;
    Ok(Json(serde_json::json!({ "projects": projects })))
}

#[derive(Debug, Deserialize)]
struct CreateProjectRequest {
    name: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    fps: Option<f64>,
}

async fn create_project(
    State(state): State<AppState>,
    Json(request): Json<CreateProjectRequest>,
) -> ApiResult<Json<Project>> {
    if request.name.trim().is_empty() {
        return Err(ApiError::bad_request("project name cannot be empty"));
    }
    let mut settings = TimelineSettings::default();
    if let Some(width) = request.width {
        settings.width = width;
    }
    if let Some(height) = request.height {
        settings.height = height;
    }
    if let Some(fps) = request.fps {
        settings.fps = fps;
    }
    Ok(Json(state.service.create_project(request.name, settings)?))
}

async fn get_project(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Json<Project>> {
    Ok(Json(state.service.get_project(project_id)?))
}

async fn delete_project(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state.service.store.delete_project(project_id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct ImportMediaRequest {
    path: PathBuf,
    #[serde(default = "default_true")]
    copy: bool,
}

async fn import_media(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<ImportMediaRequest>,
) -> ApiResult<Json<Project>> {
    let service = state.service.clone();
    let project = tokio::task::spawn_blocking(move || {
        service.import_media(project_id, &request.path, request.copy)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(project))
}

#[derive(Debug, Deserialize)]
struct AddToTimelineRequest {
    asset_id: Uuid,
    #[serde(default)]
    at_ms: Option<u64>,
    #[serde(default)]
    track_kind: Option<rustcut_core::TrackKind>,
}

async fn add_to_timeline(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<AddToTimelineRequest>,
) -> ApiResult<Json<Project>> {
    Ok(Json(state.service.apply(
        project_id,
        &[EditCommand::AddAssetToTimeline {
            asset_id: request.asset_id,
            track_kind: request.track_kind,
            at_ms: request.at_ms,
        }],
    )?))
}

#[derive(Debug, Deserialize)]
struct ImportTranscriptRequest {
    asset_id: Uuid,
    path: PathBuf,
}

async fn import_transcript(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<ImportTranscriptRequest>,
) -> ApiResult<Json<Project>> {
    Ok(Json(state.service.import_transcript(
        project_id,
        request.asset_id,
        &request.path,
    )?))
}

#[derive(Debug, Deserialize)]
struct PromptRequest {
    prompt: String,
    #[serde(default)]
    planner: PlannerChoice,
    #[serde(default = "default_true")]
    auto_apply: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum PlannerChoice {
    #[default]
    Rule,
    Llm,
}

#[derive(Debug, Serialize)]
struct PromptResponse {
    plan: EditPlan,
    project: Option<Project>,
}

async fn plan_prompt(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<PromptRequest>,
) -> ApiResult<Json<EditPlan>> {
    let planner = select_planner(&state, request.planner)?;
    let plan = state
        .service
        .plan_with(project_id, &request.prompt, planner.as_ref())
        .await?;
    Ok(Json(plan))
}

async fn apply_prompt(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<PromptRequest>,
) -> ApiResult<Json<PromptResponse>> {
    let planner = select_planner(&state, request.planner)?;
    let (plan, project) = state
        .service
        .prompt_with(
            project_id,
            &request.prompt,
            request.auto_apply,
            planner.as_ref(),
        )
        .await?;
    Ok(Json(PromptResponse { plan, project }))
}

#[derive(Debug, Deserialize)]
struct ApplyRequest {
    commands: Vec<EditCommand>,
}

async fn apply_commands(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<ApplyRequest>,
) -> ApiResult<Json<Project>> {
    Ok(Json(state.service.apply(project_id, &request.commands)?))
}

async fn undo(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Json<Project>> {
    Ok(Json(state.service.undo(project_id)?))
}

async fn redo(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<Json<Project>> {
    Ok(Json(state.service.redo(project_id)?))
}

#[derive(Debug, Deserialize)]
struct RenderRequest {
    #[serde(default)]
    output: Option<PathBuf>,
}

async fn start_render(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<RenderRequest>,
) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    let output = request
        .output
        .unwrap_or_else(|| state.service.default_render_output(project_id));
    let job_id = Uuid::new_v4();
    let record = JobRecord {
        id: job_id,
        project_id,
        kind: "render".to_string(),
        status: JobStatus::Queued,
        output: Some(output.clone()),
        error: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    state.jobs.write().insert(job_id, record.clone());

    let service = state.service.clone();
    let jobs = state.jobs.clone();
    tokio::spawn(async move {
        update_job(&jobs, job_id, JobStatus::Running, None);
        let result = tokio::task::spawn_blocking(move || service.render(project_id, output)).await;
        match result {
            Ok(Ok(_)) => update_job(&jobs, job_id, JobStatus::Completed, None),
            Ok(Err(error)) => {
                error!(%job_id, %error, "render failed");
                update_job(&jobs, job_id, JobStatus::Failed, Some(error.to_string()));
            }
            Err(error) => {
                update_job(&jobs, job_id, JobStatus::Failed, Some(error.to_string()));
            }
        }
    });

    Ok((StatusCode::ACCEPTED, Json(record)))
}

async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Json<JobRecord>> {
    let record = state
        .jobs
        .read()
        .get(&job_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("job not found"))?;
    Ok(Json(record))
}

async fn media_file(
    State(state): State<AppState>,
    Path((project_id, asset_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<Response<Body>> {
    let project = state.service.get_project(project_id)?;
    let asset = project
        .assets
        .get(&asset_id)
        .ok_or_else(|| RustCutError::AssetNotFound(asset_id.to_string()))?;
    let file_path = resolve_asset_path(&state.service.store.project_dir(project_id), &asset.path);
    let mut file = tokio::fs::File::open(&file_path).await?;
    let metadata = file.metadata().await?;
    let total = metadata.len();
    let content_type = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .to_string();

    if total == 0 {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CONTENT_LENGTH, "0")
            .body(Body::empty())
            .map_err(|error| ApiError::internal(error.to_string()));
    }

    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_byte_range(value, total));
    let (status, start, end) = range
        .map(|(start, end)| (StatusCode::PARTIAL_CONTENT, start, end))
        .unwrap_or((StatusCode::OK, 0, total.saturating_sub(1)));
    let length = end - start + 1;
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let stream = ReaderStream::new(file.take(length));
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length.to_string());
    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}"),
        );
    }
    response
        .body(Body::from_stream(stream))
        .map_err(|error| ApiError::internal(error.to_string()))
}

fn parse_byte_range(value: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return Some((0, 0));
    }
    let value = value.strip_prefix("bytes=")?;
    let (start, end) = value.split_once('-')?;
    let start = start.parse::<u64>().ok()?;
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<u64>().ok()?.min(total - 1)
    };
    (start <= end && start < total).then_some((start, end))
}

fn select_planner(state: &AppState, choice: PlannerChoice) -> ApiResult<Arc<dyn Planner>> {
    match choice {
        PlannerChoice::Rule => Ok(Arc::new(RulePlanner)),
        PlannerChoice::Llm => {
            let planner = state.llm_planner.as_ref().cloned().ok_or_else(|| {
                ApiError::bad_request(
                    "LLM planner is not configured; set RUSTCUT_LLM_BASE_URL, RUSTCUT_LLM_API_KEY and RUSTCUT_LLM_MODEL",
                )
            })?;
            Ok(planner)
        }
    }
}

fn load_llm_planner() -> Option<Arc<OpenAiCompatiblePlanner>> {
    let base_url = std::env::var("RUSTCUT_LLM_BASE_URL").ok()?;
    let api_key = std::env::var("RUSTCUT_LLM_API_KEY").ok()?;
    let model = std::env::var("RUSTCUT_LLM_MODEL").ok()?;
    Some(Arc::new(OpenAiCompatiblePlanner::new(
        base_url, api_key, model,
    )))
}

fn update_job(
    jobs: &RwLock<HashMap<Uuid, JobRecord>>,
    job_id: Uuid,
    status: JobStatus,
    error: Option<String>,
) {
    if let Some(job) = jobs.write().get_mut(&job_id) {
        job.status = status;
        job.error = error;
        job.updated_at = Utc::now();
    }
}

#[derive(Debug, Clone, Serialize)]
struct JobRecord {
    id: Uuid,
    project_id: Uuid,
    kind: String,
    status: JobStatus,
    output: Option<PathBuf>,
    error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

fn default_true() -> bool {
    true
}

type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl From<RustCutError> for ApiError {
    fn from(error: RustCutError) -> Self {
        let status = match &error {
            RustCutError::ProjectNotFound(_)
            | RustCutError::AssetNotFound(_)
            | RustCutError::TrackNotFound(_)
            | RustCutError::ClipNotFound(_) => StatusCode::NOT_FOUND,
            RustCutError::Validation(_)
            | RustCutError::Planner(_)
            | RustCutError::Transcription(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self {
            status,
            message: error.to_string(),
        }
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::internal(error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let payload = Json(serde_json::json!({ "error": self.message }));
        (self.status, payload).into_response()
    }
}
