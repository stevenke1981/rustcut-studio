use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use rustcut_core::{
    EditCommand, EditorService, FsProjectStore, MediaProbe, OpenAiCompatiblePlanner,
    OpenAiCompatibleTranscriber, Planner, RenderOptions, Renderer, RulePlanner, TimelineSettings,
};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "rustcut",
    version,
    about = "Prompt-driven video editing in Rust"
)]
struct Cli {
    #[arg(
        long,
        env = "RUSTCUT_DATA_DIR",
        default_value = "./data",
        global = true
    )]
    data_dir: PathBuf,

    #[arg(long, env = "RUSTCUT_FFMPEG", default_value = "ffmpeg", global = true)]
    ffmpeg: String,

    #[arg(
        long,
        env = "RUSTCUT_FFPROBE",
        default_value = "ffprobe",
        global = true
    )]
    ffprobe: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    New {
        name: String,
        #[arg(long, default_value_t = 1920)]
        width: u32,
        #[arg(long, default_value_t = 1080)]
        height: u32,
        #[arg(long, default_value_t = 30.0)]
        fps: f64,
    },
    List,
    Show {
        project_id: Uuid,
    },
    Import {
        project_id: Uuid,
        path: PathBuf,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        copy: bool,
    },
    Add {
        project_id: Uuid,
        asset_id: Uuid,
        #[arg(long)]
        at_ms: Option<u64>,
    },
    ImportTranscript {
        project_id: Uuid,
        asset_id: Uuid,
        path: PathBuf,
    },
    Transcribe {
        project_id: Uuid,
        asset_id: Uuid,
        #[arg(long, env = "RUSTCUT_TRANSCRIBE_BASE_URL")]
        base_url: String,
        #[arg(long, env = "RUSTCUT_TRANSCRIBE_API_KEY")]
        api_key: String,
        #[arg(long, env = "RUSTCUT_TRANSCRIBE_MODEL", default_value = "whisper-1")]
        model: String,
        #[arg(long)]
        language: Option<String>,
    },
    Plan {
        project_id: Uuid,
        prompt: String,
        #[arg(long, value_enum, default_value_t = PlannerKind::Rule)]
        planner: PlannerKind,
    },
    Prompt {
        project_id: Uuid,
        prompt: String,
        #[arg(long, value_enum, default_value_t = PlannerKind::Rule)]
        planner: PlannerKind,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        apply: bool,
    },
    Apply {
        project_id: Uuid,
        commands_json: PathBuf,
    },
    Undo {
        project_id: Uuid,
    },
    Redo {
        project_id: Uuid,
    },
    Render {
        project_id: Uuid,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
    ExportFcpxml {
        project_id: Uuid,
        output: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PlannerKind {
    Rule,
    Llm,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    let service = make_service(&cli);
    service.initialize()?;

    match cli.command {
        Commands::New {
            name,
            width,
            height,
            fps,
        } => {
            let project = service.create_project(
                name,
                TimelineSettings {
                    width,
                    height,
                    fps,
                    ..TimelineSettings::default()
                },
            )?;
            print_json(&project)?;
        }
        Commands::List => print_json(&service.list_projects()?)?,
        Commands::Show { project_id } => print_json(&service.get_project(project_id)?)?,
        Commands::Import {
            project_id,
            path,
            copy,
        } => print_json(&service.import_media(project_id, &path, copy)?)?,
        Commands::Add {
            project_id,
            asset_id,
            at_ms,
        } => {
            let project = service.apply(
                project_id,
                &[EditCommand::AddAssetToTimeline {
                    asset_id,
                    track_kind: None,
                    at_ms,
                }],
            )?;
            print_json(&project)?;
        }
        Commands::ImportTranscript {
            project_id,
            asset_id,
            path,
        } => print_json(&service.import_transcript(project_id, asset_id, &path)?)?,
        Commands::Transcribe {
            project_id,
            asset_id,
            base_url,
            api_key,
            model,
            language,
        } => {
            let transcriber =
                OpenAiCompatibleTranscriber::new(base_url, api_key, model).with_language(language);
            let project = service
                .transcribe(project_id, asset_id, &transcriber)
                .await?;
            print_json(&project)?;
        }
        Commands::Plan {
            project_id,
            prompt,
            planner,
        } => {
            let planner = planner_from_env(planner)?;
            let plan = service
                .plan_with(project_id, &prompt, planner.as_ref())
                .await?;
            print_json(&plan)?;
        }
        Commands::Prompt {
            project_id,
            prompt,
            planner,
            apply,
        } => {
            let planner = planner_from_env(planner)?;
            let (plan, project) = service
                .prompt_with(project_id, &prompt, apply, planner.as_ref())
                .await?;
            print_json(&serde_json::json!({ "plan": plan, "project": project }))?;
        }
        Commands::Apply {
            project_id,
            commands_json,
        } => {
            let bytes = std::fs::read(&commands_json)
                .with_context(|| format!("failed to read {}", commands_json.display()))?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            let commands: Vec<EditCommand> = if value.is_array() {
                serde_json::from_value(value)?
            } else {
                serde_json::from_value(
                    value
                        .get("commands")
                        .cloned()
                        .context("JSON must be an array or contain a commands field")?,
                )?
            };
            print_json(&service.apply(project_id, &commands)?)?;
        }
        Commands::Undo { project_id } => print_json(&service.undo(project_id)?)?,
        Commands::Redo { project_id } => print_json(&service.redo(project_id)?)?,
        Commands::Render {
            project_id,
            output,
            dry_run,
        } => {
            let output = output.unwrap_or_else(|| service.default_render_output(project_id));
            if dry_run {
                let command = service.build_render_command(project_id, output)?;
                println!("{}", command.display_shell());
            } else {
                let command = service.render(project_id, output)?;
                print_json(&command)?;
            }
        }
        Commands::ExportFcpxml { project_id, output } => {
            service.export_fcpxml(project_id, &output)?;
            println!("{}", output.display());
        }
    }
    Ok(())
}

fn make_service(cli: &Cli) -> EditorService {
    let options = RenderOptions {
        ffmpeg: cli.ffmpeg.clone(),
        ..RenderOptions::default()
    };
    EditorService::new(
        FsProjectStore::new(&cli.data_dir),
        MediaProbe::new(&cli.ffprobe),
        Renderer::new(options),
    )
}

fn planner_from_env(kind: PlannerKind) -> Result<Box<dyn Planner>> {
    match kind {
        PlannerKind::Rule => Ok(Box::new(RulePlanner)),
        PlannerKind::Llm => {
            let base_url = std::env::var("RUSTCUT_LLM_BASE_URL")
                .context("RUSTCUT_LLM_BASE_URL is required for --planner llm")?;
            let api_key = std::env::var("RUSTCUT_LLM_API_KEY")
                .context("RUSTCUT_LLM_API_KEY is required for --planner llm")?;
            let model = std::env::var("RUSTCUT_LLM_MODEL")
                .context("RUSTCUT_LLM_MODEL is required for --planner llm")?;
            Ok(Box::new(OpenAiCompatiblePlanner::new(
                base_url, api_key, model,
            )))
        }
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
