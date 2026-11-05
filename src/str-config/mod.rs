//! Persistent configuration. Stored as JSON so it stays easy to inspect and
//! repair by hand.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::paths::{atomic_write, AppDirs};

pub const CONFIG_FILE: &str = "config.json";
/// Nexus mod id for Address Library for SKSE Plugins (All in One lives here).
pub const NEXUS_ADDRESS_LIBRARY: u64 = 32444;
/// Nexus mod id for Skyrim Together Reborn.
pub const NEXUS_STR: u64 = 69993;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub game_dir: Option<PathBuf>,
    #[serde(default)]
    pub steam_root: Option<PathBuf>,
    /// Linux: compatibility tool name, e.g. `GE-Proton10-34`.
    #[serde(default)]
    pub proton_tool: Option<String>,
    #[serde(default)]
    pub installed_str_version: Option<String>,
    #[serde(default)]
    pub installed_str_file_id: Option<u64>,
    #[serde(default)]
    pub installed_al_version: Option<String>,
    #[serde(default)]
    pub installed_al_file_id: Option<u64>,
}

/// How the game is launched. The two modes are mutually exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LaunchMode {
    /// Symlink `SkyrimSELauncher.exe` → `SkyrimTogether.exe` so starting
    /// Skyrim through Steam (or Game Mode) begins multiplayer.
    #[default]
    LauncherReplacement,
    /// Register "Skyrim Together Reborn" as a separate non-Steam game entry
    /// (with SteamGridDB artwork) that runs the multiplayer client directly.
    /// The original Skyrim launcher stays untouched.
    SteamGame,
}

impl std::fmt::Display for LaunchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl LaunchMode {
    pub const ALL: [LaunchMode; 2] = [LaunchMode::LauncherReplacement, LaunchMode::SteamGame];

    pub fn label(self) -> &'static str {
        match self {
            LaunchMode::LauncherReplacement => "Replace the Skyrim launcher",
            LaunchMode::SteamGame => "Add a separate Steam game",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            LaunchMode::LauncherReplacement => {
                "Starting Skyrim through Steam launches Skyrim Together directly (the original \
                 launcher is kept as _SkyrimSELauncher.exe)."
            }
            LaunchMode::SteamGame => {
                "\"Skyrim Together Reborn\" appears as its own game in your Steam library (with \
                 SteamGridDB artwork); Skyrim itself stays 100% vanilla."
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub profiles: Vec<Profile>,
    pub active_profile: usize,
    pub nexus_api_key: String,
    /// Back up and remove the bundled Creation Club plugins on install.
    pub clean_cc: bool,
    /// Create Start Menu / desktop / applications shortcuts.
    pub create_shortcuts: bool,
    /// How launching integrates with Steam / the game folder.
    pub launch_mode: LaunchMode,
    /// Cached download bookkeeping (per mod id -> file id).
    pub cached_downloads: std::collections::BTreeMap<String, u64>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            profiles: vec![Profile {
                name: "Default".into(),
                ..Default::default()
            }],
            active_profile: 0,
            nexus_api_key: String::new(),
            clean_cc: true,
            create_shortcuts: true,
            launch_mode: LaunchMode::default(),
            cached_downloads: Default::default(),
        }
    }
}

impl Config {
    pub fn active(&self) -> &Profile {
        self.profiles
            .get(self.active_profile)
            .unwrap_or_else(|| self.profiles.first().expect("always at least one profile"))
    }

    pub fn active_mut(&mut self) -> &mut Profile {
        let idx = self
            .active_profile
            .min(self.profiles.len().saturating_sub(1));
        &mut self.profiles[idx]
    }

    pub fn load(dirs: &AppDirs) -> Config {
        let path = dirs.config.join(CONFIG_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("config.json is invalid ({e}); starting with defaults");
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self, dirs: &AppDirs) -> Result<(), String> {
        let path = dirs.config.join(CONFIG_FILE);
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        atomic_write(&path, text.as_bytes())
    }
}

/// Manifest of the files we installed into the game folder. Used for clean
/// updates and the "Undo" action.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallManifest {
    pub str_version: Option<String>,
    pub str_file_id: Option<u64>,
    pub address_library_version: Option<String>,
    pub address_library_file_id: Option<u64>,
    /// Game-root relative file paths installed by us.
    pub files: Vec<String>,
    pub installed_at: u64,
}

impl InstallManifest {
    pub fn load(game_dir: &Path) -> InstallManifest {
        let path = game_dir.join(".str-launcher-manifest.json");
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, game_dir: &Path) -> Result<(), String> {
        let path = game_dir.join(".str-launcher-manifest.json");
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        atomic_write(&path, text.as_bytes())
    }

    pub fn remove(game_dir: &Path) {
        let _ = std::fs::remove_file(game_dir.join(".str-launcher-manifest.json"));
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let cfg = Config::default();
        let text = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert!(back.clean_cc);
        assert_eq!(back.profiles.len(), 1);
    }
}
