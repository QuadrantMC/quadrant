//! Shared serialization-friendly data models used across core services.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Source provider for a mod or downloadable file.
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub enum ModSource {
    #[serde(rename = "ModSource.curseForge")]
    CurseForge,
    #[serde(rename = "ModSource.modRinth")]
    Modrinth,
    #[serde(rename = "ModSource.online")]
    Online,
}

/// Minimal persisted representation of an installed mod entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    /// Provider-specific mod identifier.
    pub id: String,
    /// Source provider of the mod.
    pub source: ModSource,
    /// URL of the installed file that was selected for this mod.
    pub download_url: String,
}

/// Supported mod loader families used throughout Quadrant.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq)]
pub enum ModLoader {
    #[serde(rename = "Forge")]
    Forge,
    #[serde(rename = "Fabric")]
    Fabric,
    #[serde(rename = "NeoForge")]
    NeoForge,
    #[serde(rename = "Quilt")]
    Quilt,
    #[serde(rename = "Rift")]
    Rift,
    #[serde(rename = "Unknown")]
    Unknown,
}

impl From<String> for ModLoader {
    fn from(value: String) -> Self {
        match value.to_lowercase().as_str() {
            "forge" => Self::Forge,
            "fabric" => Self::Fabric,
            "neoforge" => Self::NeoForge,
            "quilt" => Self::Quilt,
            "rift" => Self::Rift,
            _ => Self::Unknown,
        }
    }
}

impl ModLoader {
    /// Returns the CurseForge mod loader identifier used by provider queries.
    pub fn to_curseforge_id(&self) -> i64 {
        match self {
            Self::Forge => 1,
            Self::Fabric => 4,
            Self::NeoForge => 6,
            Self::Rift => 999,
            Self::Quilt => 5,
            Self::Unknown => 0,
        }
    }
}

impl std::fmt::Display for ModLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Forge => "Forge",
            Self::Fabric => "Fabric",
            Self::NeoForge => "NeoForge",
            Self::Quilt => "Quilt",
            Self::Rift => "Rift",
            Self::Unknown => "Unknown",
        };
        f.write_str(label)
    }
}

/// On-disk modpack manifest representation.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModpack {
    /// Human-readable modpack name.
    pub name: String,
    /// Minecraft version the modpack targets.
    pub version: String,
    /// Mod loader the modpack requires.
    pub mod_loader: ModLoader,
    /// Mods currently registered in the modpack manifest.
    pub mods: Vec<InstalledMod>,
}

/// Local modpack model enriched with frontend-oriented state.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocalModpack {
    /// Human-readable modpack name.
    pub name: String,
    /// Minecraft version the modpack targets.
    pub version: String,
    /// Mod loader the modpack requires.
    pub mod_loader: ModLoader,
    /// Mods currently registered in the modpack manifest.
    pub mods: Vec<InstalledMod>,
    /// Whether the modpack directory contains files not tracked by the manifest.
    pub unknown_mods: bool,
    /// Whether this modpack is currently applied as the active `mods` folder.
    pub is_applied: bool,
    /// Last successful sync time in milliseconds since the Unix epoch.
    pub last_synced: i64,
    /// Stable synced modpack identifier when this modpack is linked to Quadrant Sync.
    pub modpack_id: Option<String>,
}

impl From<LocalModpack> for InstalledModpack {
    fn from(modpack: LocalModpack) -> Self {
        Self {
            name: modpack.name,
            version: modpack.version,
            mod_loader: modpack.mod_loader,
            mods: modpack.mods,
        }
    }
}

/// Sync metadata stored alongside a modpack.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SyncInfo {
    /// Last successful sync time in seconds since the Unix epoch.
    pub last_synced: i64,
    /// Stable synced modpack identifier when known.
    #[serde(default)]
    pub modpack_id: Option<String>,
}

impl From<(InstalledModpack, bool, i64)> for LocalModpack {
    fn from(modpack: (InstalledModpack, bool, i64)) -> Self {
        Self {
            name: modpack.0.name,
            version: modpack.0.version,
            mod_loader: modpack.0.mod_loader,
            mods: modpack.0.mods,
            is_applied: modpack.1,
            last_synced: modpack.2,
            modpack_id: None,
            unknown_mods: false,
        }
    }
}

/// RSS article surfaced by the Quadrant news feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Article {
    /// Article title.
    pub title: String,
    /// Canonical article link.
    pub link: String,
    /// Article summary or description.
    pub summary: String,
    /// Publication timestamp.
    pub date: DateTime<Utc>,
    /// Stable feed item GUID.
    pub guid: String,
    /// Whether the article should be treated as recent by the app.
    pub new: bool,
}

/// Returns the canonical filesystem path for a named modpack.
pub fn modpack_path(mc_folder: &Path, modpack_name: &str) -> PathBuf {
    mc_folder.join("modpacks").join(modpack_name)
}
