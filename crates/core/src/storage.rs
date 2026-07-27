use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{Project, Result, RustCutError, TimelineSettings};

#[derive(Debug, Clone)]
pub struct FsProjectStore {
    root: PathBuf,
    save_lock: Arc<Mutex<()>>,
}

impl FsProjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            save_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn initialize(&self) -> Result<()> {
        fs::create_dir_all(self.projects_root())?;
        Ok(())
    }

    pub fn create_project(
        &self,
        name: impl Into<String>,
        settings: TimelineSettings,
    ) -> Result<Project> {
        self.initialize()?;
        let project = Project::new(name, settings);
        let directory = self.project_dir(project.id);
        fs::create_dir_all(directory.join("assets"))?;
        fs::create_dir_all(directory.join("renders"))?;
        fs::create_dir_all(directory.join("transcripts"))?;
        self.save_project(&project)?;
        Ok(project)
    }

    pub fn save_project(&self, project: &Project) -> Result<()> {
        let _guard = self.save_lock.lock();
        let directory = self.project_dir(project.id);
        fs::create_dir_all(&directory)?;
        let destination = directory.join("project.json");
        let temporary = directory.join(format!("project.json.{}.tmp", Uuid::new_v4()));
        let backup = directory.join("project.json.bak");
        let data = serde_json::to_vec_pretty(project)?;
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&data)?;
        file.sync_all()?;
        if destination.exists() {
            fs::copy(&destination, &backup)?;
        }

        #[cfg(windows)]
        {
            if destination.exists() {
                // Windows rename() cannot replace an existing file. Keep a verified
                // backup and restore it if promoting the temporary file fails.
                fs::remove_file(&destination)?;
            }
            if let Err(error) = fs::rename(&temporary, &destination) {
                let restore_result = if backup.exists() {
                    fs::copy(&backup, &destination).map(|_| ())
                } else {
                    Ok(())
                };
                let _ = fs::remove_file(&temporary);
                if let Err(restore_error) = restore_result {
                    return Err(RustCutError::Io(std::io::Error::other(format!(
                        "could not replace project file: {error}; backup restore failed: {restore_error}"
                    ))));
                }
                return Err(error.into());
            }
        }

        #[cfg(not(windows))]
        fs::rename(&temporary, &destination)?;

        Ok(())
    }

    pub fn load_project(&self, project_id: Uuid) -> Result<Project> {
        let path = self.project_dir(project_id).join("project.json");
        if !path.exists() {
            return Err(RustCutError::ProjectNotFound(project_id.to_string()));
        }
        let bytes = fs::read(path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>> {
        self.initialize()?;
        let mut projects = Vec::new();
        for entry in fs::read_dir(self.projects_root())? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path().join("project.json");
            if !path.exists() {
                continue;
            }
            let bytes = fs::read(path)?;
            let project: Project = serde_json::from_slice(&bytes)?;
            projects.push(ProjectSummary::from(&project));
        }
        projects.sort_by_key(|project| std::cmp::Reverse(project.updated_at));
        Ok(projects)
    }

    pub fn delete_project(&self, project_id: Uuid) -> Result<()> {
        let directory = self.project_dir(project_id);
        if !directory.exists() {
            return Err(RustCutError::ProjectNotFound(project_id.to_string()));
        }
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    pub fn project_dir(&self, project_id: Uuid) -> PathBuf {
        self.projects_root().join(project_id.to_string())
    }

    pub fn render_dir(&self, project_id: Uuid) -> PathBuf {
        self.project_dir(project_id).join("renders")
    }

    pub fn project_size_bytes(&self, project_id: Uuid) -> Result<u64> {
        let directory = self.project_dir(project_id);
        if !directory.exists() {
            return Err(RustCutError::ProjectNotFound(project_id.to_string()));
        }
        let mut total = 0_u64;
        for entry in WalkDir::new(directory) {
            let entry = entry
                .map_err(|error| RustCutError::Io(std::io::Error::other(error.to_string())))?;
            if entry.file_type().is_file() {
                let metadata = entry
                    .metadata()
                    .map_err(|error| RustCutError::Io(std::io::Error::other(error.to_string())))?;
                total = total.saturating_add(metadata.len());
            }
        }
        Ok(total)
    }

    fn projects_root(&self) -> PathBuf {
        self.root.join("projects")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: Uuid,
    pub name: String,
    pub updated_at: DateTime<Utc>,
    pub revision: u64,
    pub asset_count: usize,
    pub duration_ms: u64,
}

impl From<&Project> for ProjectSummary {
    fn from(project: &Project) -> Self {
        Self {
            id: project.id,
            name: project.name.clone(),
            updated_at: project.updated_at,
            revision: project.revision,
            asset_count: project.assets.len(),
            duration_ms: project.timeline.duration_ms(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_latest_project_and_retains_previous_backup() {
        let root = std::env::temp_dir().join(format!("rustcut-store-{}", Uuid::new_v4()));
        let store = FsProjectStore::new(&root);
        let mut project = store
            .create_project("Original", TimelineSettings::default())
            .unwrap();
        project.name = "Updated".to_string();
        store.save_project(&project).unwrap();

        assert_eq!(store.load_project(project.id).unwrap().name, "Updated");
        let backup = fs::read(store.project_dir(project.id).join("project.json.bak")).unwrap();
        let previous: Project = serde_json::from_slice(&backup).unwrap();
        assert_eq!(previous.name, "Original");

        fs::remove_dir_all(root).unwrap();
    }
}
