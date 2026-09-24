use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// User configuration, stored in `%APPDATA%\CellPresence\config.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub ps3_ip: Option<String>,
    pub discord_app_id: Option<String>,
}

impl Config {
    pub fn dir() -> PathBuf {
        match std::env::var_os("APPDATA") {
            Some(appdata) => PathBuf::from(appdata).join("CellPresence"),
            None => std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("CellPresence"),
        }
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.json")
    }

    /// Whether the user already has a configuration file.
    pub fn exists() -> bool {
        Self::path().is_file()
    }

    pub fn load() -> Self {
        match std::fs::read(Self::path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = Self::dir();
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(Self::path(), json)
    }
}