use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub spotify_client_id: String,
    pub spotify_client_secret: String,
    /// Where to look for songs. Defaults to ~/Music.
    #[serde(default)]
    pub music_dir: Option<String>,
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir().unwrap_or_default().join("songos/config.json")
    }

    pub fn load() -> Result<Self> {
        let p = Self::path();
        let raw = std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", p.display()))
    }

    pub fn music_dir(&self) -> PathBuf {
        match &self.music_dir {
            Some(d) if d.starts_with("~/") => dirs::home_dir().unwrap_or_default().join(&d[2..]),
            Some(d) => PathBuf::from(d),
            None => dirs::audio_dir().or_else(|| dirs::home_dir().map(|h| h.join("Music"))).unwrap_or_default(),
        }
    }
}
