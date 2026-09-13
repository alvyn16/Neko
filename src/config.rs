use crate::model::{Provider, Tab};
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub wallpaper_folder: Option<PathBuf>,
    pub last_tab: Tab,
    pub last_source: Provider,
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("com", "Neko", "Neko")
        .context("Could not locate your application data folder")
}

fn config_path() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("NEKO_DATA_DIR").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(root).join("config.toml"));
    }
    Ok(project_dirs()?.config_dir().join("config.toml"))
}

pub fn cache_dir() -> Result<PathBuf> {
    let path = if let Some(root) = std::env::var_os("NEKO_DATA_DIR").filter(|v| !v.is_empty()) {
        PathBuf::from(root).join("cache")
    } else {
        project_dirs()?.cache_dir().to_owned()
    };
    fs::create_dir_all(&path)
        .with_context(|| format!("Could not create cache folder {}", path.display()))?;
    Ok(path)
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path()?)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&config_path()?)
    }

    fn load_from(path: &std::path::Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents)
                .with_context(|| format!("Could not read settings in {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => {
                Err(e).with_context(|| format!("Could not open settings in {}", path.display()))
            }
        }
    }

    fn save_to(&self, path: &std::path::Path) -> Result<()> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let parent = path
            .parent()
            .context("Settings file has no parent folder")?;
        fs::create_dir_all(parent).context("Could not create settings folder")?;
        let temporary = parent.join(format!(
            ".config-{}-{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let contents = toml::to_string_pretty(self).context("Could not serialize settings")?;
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            drop(file);
            // std::fs::rename atomically replaces an existing file on Windows and Unix.
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.with_context(|| format!("Could not save settings to {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_replace() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        assert_eq!(Config::load_from(&path).unwrap().last_tab, Tab::Local);
        let mut config = Config {
            wallpaper_folder: Some(PathBuf::from(r"C:\Pictures\猫 wallpapers")),
            last_tab: Tab::Search,
            last_source: Provider::Bjarneo,
        };
        config.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.wallpaper_folder, config.wallpaper_folder);
        assert_eq!(loaded.last_tab, Tab::Search);
        assert_eq!(loaded.last_source, Provider::Bjarneo);
        config.last_tab = Tab::Local;
        config.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap().last_tab, Tab::Local);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn older_settings_get_defaults_and_invalid_settings_report_error() {
        let old: Config = toml::from_str("last_tab = 'search'").unwrap();
        assert_eq!(old.last_source, Provider::Wallhaven);
        assert!(old.wallpaper_folder.is_none());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "last_tab = [broken").unwrap();
        assert!(Config::load_from(&path).is_err());
    }
}
