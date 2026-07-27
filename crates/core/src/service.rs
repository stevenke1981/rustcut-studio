use std::path::{Path, PathBuf};

use chrono::Utc;
use uuid::Uuid;

use crate::{
    EditCommand, EditPlan, FsProjectStore, MediaProbe, Planner, Project, ProjectSummary,
    RenderCommand, Renderer, Result, RulePlanner, TimelineSettings, Transcriber, apply_commands,
    export_fcpxml, import_asset, load_transcript_file,
};

#[derive(Debug, Clone)]
pub struct EditorService {
    pub store: FsProjectStore,
    probe: MediaProbe,
    renderer: Renderer,
}

impl EditorService {
    pub fn new(store: FsProjectStore, probe: MediaProbe, renderer: Renderer) -> Self {
        Self {
            store,
            probe,
            renderer,
        }
    }

    pub fn initialize(&self) -> Result<()> {
        self.store.initialize()
    }

    pub fn create_project(
        &self,
        name: impl Into<String>,
        settings: TimelineSettings,
    ) -> Result<Project> {
        self.store.create_project(name, settings)
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>> {
        self.store.list_projects()
    }

    pub fn get_project(&self, project_id: Uuid) -> Result<Project> {
        self.store.load_project(project_id)
    }

    pub fn import_media(
        &self,
        project_id: Uuid,
        source: &Path,
        copy_into_project: bool,
    ) -> Result<Project> {
        let mut project = self.store.load_project(project_id)?;
        let directory = self.store.project_dir(project_id);
        let mut asset = import_asset(&self.probe, source, &directory, copy_into_project)?;
        if asset.kind == crate::AssetKind::Image && asset.duration_ms == 0 {
            asset.duration_ms = 5_000;
        }
        project.assets.insert(asset.id, asset);
        project.revision += 1;
        project.updated_at = Utc::now();
        self.store.save_project(&project)?;
        Ok(project)
    }

    pub fn import_transcript(
        &self,
        project_id: Uuid,
        asset_id: Uuid,
        transcript_path: &Path,
    ) -> Result<Project> {
        let mut project = self.store.load_project(project_id)?;
        if !project.assets.contains_key(&asset_id) {
            return Err(crate::RustCutError::AssetNotFound(asset_id.to_string()));
        }
        let transcript = load_transcript_file(transcript_path, asset_id)?;
        let project_dir = self.store.project_dir(project_id);
        let transcript_dir = project_dir.join("transcripts");
        std::fs::create_dir_all(&transcript_dir)?;
        std::fs::write(
            transcript_dir.join(format!("{asset_id}.json")),
            serde_json::to_vec_pretty(&transcript)?,
        )?;
        project.transcripts.insert(asset_id, transcript);
        project.revision += 1;
        project.updated_at = Utc::now();
        self.store.save_project(&project)?;
        Ok(project)
    }

    pub async fn transcribe<T: Transcriber + ?Sized>(
        &self,
        project_id: Uuid,
        asset_id: Uuid,
        transcriber: &T,
    ) -> Result<Project> {
        let mut project = self.store.load_project(project_id)?;
        let asset = project
            .assets
            .get(&asset_id)
            .ok_or_else(|| crate::RustCutError::AssetNotFound(asset_id.to_string()))?;
        let media_path =
            crate::resolve_asset_path(&self.store.project_dir(project_id), &asset.path);
        let mut transcript = transcriber.transcribe(asset_id, &media_path).await?;
        transcript.normalize();
        let transcript_dir = self.store.project_dir(project_id).join("transcripts");
        std::fs::create_dir_all(&transcript_dir)?;
        std::fs::write(
            transcript_dir.join(format!("{asset_id}.json")),
            serde_json::to_vec_pretty(&transcript)?,
        )?;
        project.transcripts.insert(asset_id, transcript);
        project.revision += 1;
        project.updated_at = Utc::now();
        self.store.save_project(&project)?;
        Ok(project)
    }

    pub async fn plan_rule(&self, project_id: Uuid, prompt: &str) -> Result<EditPlan> {
        self.plan_with(project_id, prompt, &RulePlanner).await
    }

    pub async fn plan_with<P: Planner + ?Sized>(
        &self,
        project_id: Uuid,
        prompt: &str,
        planner: &P,
    ) -> Result<EditPlan> {
        let project = self.store.load_project(project_id)?;
        planner.plan(&project, prompt).await
    }

    pub fn apply(&self, project_id: Uuid, commands: &[EditCommand]) -> Result<Project> {
        let mut project = self.store.load_project(project_id)?;
        apply_commands(&mut project, commands)?;
        self.store.save_project(&project)?;
        Ok(project)
    }

    pub async fn prompt_with<P: Planner + ?Sized>(
        &self,
        project_id: Uuid,
        prompt: &str,
        auto_apply: bool,
        planner: &P,
    ) -> Result<(EditPlan, Option<Project>)> {
        let plan = self.plan_with(project_id, prompt, planner).await?;
        let project = if auto_apply && !plan.commands.is_empty() {
            Some(self.apply(project_id, &plan.commands)?)
        } else {
            None
        };
        Ok((plan, project))
    }

    pub fn undo(&self, project_id: Uuid) -> Result<Project> {
        let mut project = self.store.load_project(project_id)?;
        project.undo();
        self.store.save_project(&project)?;
        Ok(project)
    }

    pub fn redo(&self, project_id: Uuid) -> Result<Project> {
        let mut project = self.store.load_project(project_id)?;
        project.redo();
        self.store.save_project(&project)?;
        Ok(project)
    }

    pub fn build_render_command(
        &self,
        project_id: Uuid,
        output: impl Into<PathBuf>,
    ) -> Result<RenderCommand> {
        let project = self.store.load_project(project_id)?;
        self.renderer
            .build_command(&project, &self.store.project_dir(project_id), output)
    }

    pub fn render(&self, project_id: Uuid, output: impl Into<PathBuf>) -> Result<RenderCommand> {
        let project = self.store.load_project(project_id)?;
        self.renderer
            .render(&project, &self.store.project_dir(project_id), output)
    }

    pub fn default_render_output(&self, project_id: Uuid) -> PathBuf {
        self.store
            .render_dir(project_id)
            .join(format!("render-{}.mp4", Utc::now().format("%Y%m%d-%H%M%S")))
    }

    pub fn export_fcpxml(&self, project_id: Uuid, destination: impl AsRef<Path>) -> Result<()> {
        let project = self.store.load_project(project_id)?;
        export_fcpxml(
            &project,
            &self.store.project_dir(project_id),
            destination.as_ref(),
        )
    }
}
