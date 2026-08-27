use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRef {
    pub id: String,
    pub path: String,
    pub checksum: String,
    pub duration_seconds: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub codec: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub workspace_path: PathBuf,
    pub assets: HashMap<String, AssetRef>,
    pub sequences: Vec<SequenceRef>,
    pub active_sequence: String,
    pub settings: ProjectSettings,
    pub autosave_version: u64,
    pub last_modified: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub default_resolution: (u32, u32),
    pub default_fps: f64,
    pub proxy_enabled: bool,
    pub proxy_quality: ProxyQualitySetting,
    pub autosave_interval_seconds: u64,
    pub max_backup_count: usize,
    pub color_space: ColorSpace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProxyQualitySetting {
    Quarter,
    Half,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ColorSpace {
    Rec709,
    Rec2020,
    DCIP3,
    SRGB,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            default_resolution: (1920, 1080),
            default_fps: 30.0,
            proxy_enabled: true,
            proxy_quality: ProxyQualitySetting::Half,
            autosave_interval_seconds: 60,
            max_backup_count: 10,
            color_space: ColorSpace::Rec709,
        }
    }
}

pub struct ProjectCodec {
    backup_dir: PathBuf,
}

impl ProjectCodec {
    pub fn new(workspace: &Path) -> Self {
        let backup_dir = workspace.join(".editorcito_backups");
        let _ = fs::create_dir_all(&backup_dir);
        Self { backup_dir }
    }

    pub fn create_project(name: &str, workspace: &Path) -> Result<Project, String> {
        fs::create_dir_all(workspace)
            .map_err(|e| format!("Workspace creation failed: {}", e))?;
        let project = Project {
            id: generate_project_id(),
            name: name.to_string(),
            path: workspace.join(format!("{}.ncproj", name)),
            workspace_path: workspace.to_path_buf(),
            assets: HashMap::new(),
            sequences: Vec::new(),
            active_sequence: String::new(),
            settings: ProjectSettings::default(),
            autosave_version: 0,
            last_modified: current_timestamp(),
        };

        Ok(project)
    }

    pub fn save(&self, project: &mut Project) -> Result<(), String> {
        project.last_modified = current_timestamp();
        project.autosave_version += 1;

        let json = serde_json::to_string_pretty(project)
            .map_err(|e| format!("Serialization failed: {}", e))?;

        fs::write(&project.path, json)
            .map_err(|e| format!("Write failed: {}", e))?;

        self.create_backup(project)?;
        self.prune_backups(project)?;

        Ok(())
    }

    pub fn load(path: &Path, workspace: &Path) -> Result<Project, String> {
        let json = fs::read_to_string(path)
            .map_err(|e| format!("Read failed: {}", e))?;

        let mut project: Project = serde_json::from_str(&json)
            .map_err(|e| format!("Deserialization failed: {}", e))?;

        project.path = path.to_path_buf();
        project.workspace_path = workspace.to_path_buf();

        Ok(project)
    }

    pub fn add_asset(&mut self, project: &mut Project, path: &str) -> Result<AssetRef, String> {
        let id = format!("asset_{}", current_timestamp());
        let metadata = Self::read_asset_metadata(path)?;

        let asset = AssetRef {
            id: id.clone(),
            path: path.to_string(),
            checksum: compute_checksum(path)?,
            duration_seconds: metadata.duration,
            width: metadata.width,
            height: metadata.height,
            fps: metadata.fps,
            codec: metadata.codec,
        };

        project.assets.insert(id, asset.clone());
        Ok(asset)
    }

    pub fn autosave(&self, project: &mut Project) -> Result<(), String> {
        project.autosave_version += 1;
        fs::create_dir_all(&self.backup_dir)
            .map_err(|e| format!("Backup directory creation failed: {}", e))?;
        let backup_path = self.backup_dir.join(format!(
            "autosave_{}_{}.json",
            project.id,
            project.autosave_version
        ));

        let json = serde_json::to_string_pretty(project)
            .map_err(|e| format!("Autosave failed: {}", e))?;

        fs::write(&backup_path, json)
            .map_err(|e| format!("Autosave write failed: {}", e))?;

        self.prune_backups(project)?;
        Ok(())
    }

    fn create_backup(&self, project: &Project) -> Result<(), String> {
        fs::create_dir_all(&self.backup_dir)
            .map_err(|e| format!("Backup directory creation failed: {}", e))?;
        let backup_path = self.backup_dir.join(format!(
            "backup_{}_{}.json",
            project.id,
            project.autosave_version
        ));

        let json = serde_json::to_string_pretty(project)
            .map_err(|e| format!("Backup serialization failed: {}", e))?;

        fs::write(&backup_path, json)
            .map_err(|e| format!("Backup write failed: {}", e))?;

        Ok(())
    }

    fn prune_backups(&self, project: &Project) -> Result<(), String> {
        let entries = fs::read_dir(&self.backup_dir)
            .map_err(|e| format!("Read backup dir failed: {}", e))?;

        let backup_prefix = format!("backup_{}_", project.id);
        let autosave_prefix = format!("autosave_{}_", project.id);
        let mut backups: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with(&backup_prefix) || name.starts_with(&autosave_prefix)
            })
            .collect();

        backups.sort_by_key(|e| {
            e.metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH)
        });

        while backups.len() > project.settings.max_backup_count {
            let first = backups.remove(0);
            let _ = fs::remove_file(first.path());
        }

        Ok(())
    }

    fn read_asset_metadata(path: &str) -> Result<AssetMetadata, String> {
        let p = Path::new(path);
        if !p.exists() {
            return Err(format!("Asset not found: {}", path));
        }

        Ok(AssetMetadata {
            duration: 60.0,
            width: 1920,
            height: 1080,
            fps: 30.0,
            codec: String::from("h264"),
        })
    }
}

#[derive(Debug)]
struct AssetMetadata {
    duration: f64,
    width: u32,
    height: u32,
    fps: f64,
    codec: String,
}

fn generate_project_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("editorcito_{}_{}_{}", std::process::id(), millis, sequence)
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn compute_checksum(path: &str) -> Result<String, String> {
    let mut archivo = fs::File::open(path).map_err(|e| format!("Asset checksum failed: {}", e))?;
    let mut buffer = [0_u8; 1024 * 1024];
    let mut hash = 0xcbf29ce484222325_u64;
    loop {
        let leidos = archivo
            .read(&mut buffer)
            .map_err(|e| format!("Asset checksum failed: {}", e))?;
        if leidos == 0 { break; }
        for byte in &buffer[..leidos] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    Ok(format!("fnv1a64:{:016x}", hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_project() {
        let workspace = std::env::temp_dir().join("editorcito_test_workspace");
        let _ = fs::remove_dir_all(&workspace);

        let project = ProjectCodec::create_project("TestProject", &workspace).unwrap();
        assert_eq!(project.name, "TestProject");
        assert_eq!(project.settings.default_resolution, (1920, 1080));
        assert_eq!(project.settings.default_fps, 30.0);

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn test_save_and_load() {
        let workspace = std::env::temp_dir().join("editorcito_test_workspace2");
        let _ = fs::remove_dir_all(&workspace);
        let _ = fs::create_dir_all(&workspace);

        let codec = ProjectCodec::new(&workspace);
        let mut project = ProjectCodec::create_project("SaveTest", &workspace).unwrap();

        codec.save(&mut project).unwrap();
        let loaded = ProjectCodec::load(&project.path, &workspace).unwrap();

        assert_eq!(loaded.name, "SaveTest");
        assert_eq!(loaded.autosave_version, 1);

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn test_project_settings_default() {
        let settings = ProjectSettings::default();
        assert!(settings.proxy_enabled);
        assert_eq!(settings.autosave_interval_seconds, 60);
        assert_eq!(settings.max_backup_count, 10);
    }

    #[test]
    fn test_backup_limit_applies_to_all_snapshots() {
        let workspace = std::env::temp_dir().join("editorcito_test_backups");
        let _ = fs::remove_dir_all(&workspace);

        let codec = ProjectCodec::new(&workspace);
        let mut project = ProjectCodec::create_project("BackupTest", &workspace).unwrap();
        project.settings.max_backup_count = 2;
        codec.save(&mut project).unwrap();
        codec.autosave(&mut project).unwrap();
        codec.save(&mut project).unwrap();

        let snapshot_count = fs::read_dir(workspace.join(".editorcito_backups"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(snapshot_count, 2);

        let _ = fs::remove_dir_all(workspace);
    }
}
