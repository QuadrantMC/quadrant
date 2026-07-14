//! Shared serialization-friendly data models used across core services.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, de::Error as DeError};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static QUADRANT_VERSION: OnceLock<String> = OnceLock::new();

pub fn set_quadrant_version(version: String) {
    let _ = QUADRANT_VERSION.set(version);
}

pub fn quadrant_version() -> String {
    QUADRANT_VERSION
        .get()
        .cloned()
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

fn default_mod_config_version() -> String {
    "1".to_string()
}

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

/// Persisted representation of an installed mod entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    /// Human-readable mod name.
    #[serde(default)]
    pub name: String,
    /// Provider-specific mod identifier.
    pub id: String,
    /// Aggregate download count from the upstream provider.
    #[serde(default)]
    pub download_count: i64,
    /// Mod version label (e.g. "1.2.3").
    #[serde(default)]
    pub version: String,
    /// Broad content type ("Mod", "ResourcePack", "ShaderPack", "Unknown").
    #[serde(default)]
    pub mod_type: String,
    /// Source provider of the mod.
    pub source: ModSource,
    /// Provider slug used to build URLs.
    #[serde(default)]
    pub slug: String,
    /// Preview image URLs exposed by the provider.
    #[serde(default)]
    pub thumbnail_urls: Vec<String>,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// License label, if known.
    #[serde(default)]
    pub license: String,
    /// Primary icon URL.
    #[serde(default)]
    pub mod_icon_url: String,
    /// URL of the installed file that was selected for this mod.
    pub download_url: String,
}

impl InstalledMod {
    /// Creates an `InstalledMod` with only the required fields populated;
    /// all optional fields default to empty/zero values.
    pub fn minimal(id: String, source: ModSource, download_url: String) -> Self {
        Self {
            id,
            source,
            download_url,
            name: String::new(),
            download_count: 0,
            version: String::new(),
            mod_type: String::new(),
            slug: String::new(),
            thumbnail_urls: Vec::new(),
            description: String::new(),
            license: String::new(),
            mod_icon_url: String::new(),
        }
    }
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
    #[serde(rename = "Babric")]
    Babric,
    #[serde(rename = "BTA (Babric)")]
    BtaBabric,
    #[serde(rename = "Java Agent")]
    JavaAgent,
    #[serde(rename = "Legacy Fabric")]
    LegacyFabric,
    #[serde(rename = "LiteLoader")]
    LiteLoader,
    #[serde(rename = "Risugami's ModLoader")]
    RisugamisModLoader,
    #[serde(rename = "NilLoader")]
    NilLoader,
    #[serde(rename = "Ornithe")]
    Ornithe,
    #[serde(rename = "Quilt")]
    Quilt,
    #[serde(rename = "Rift")]
    Rift,
    #[serde(rename = "Unknown")]
    Unknown,
}

impl From<String> for ModLoader {
    fn from(value: String) -> Self {
        match value.trim().to_lowercase().as_str() {
            "forge" => Self::Forge,
            "fabric" => Self::Fabric,
            "neoforge" => Self::NeoForge,
            "babric" => Self::Babric,
            "bta (babric)" | "bta-babric" | "bta babric" => Self::BtaBabric,
            "java agent" | "java-agent" => Self::JavaAgent,
            "legacy fabric" | "legacy-fabric" => Self::LegacyFabric,
            "liteloader" | "lite loader" | "lite-loader" => Self::LiteLoader,
            "risugami's modloader" | "risugamis modloader" | "risugami modloader" | "modloader" => {
                Self::RisugamisModLoader
            }
            "nilloader" | "nil loader" | "nil-loader" => Self::NilLoader,
            "ornithe" => Self::Ornithe,
            "quilt" => Self::Quilt,
            "rift" => Self::Rift,
            _ => Self::Unknown,
        }
    }
}

impl ModLoader {
    /// Returns the Modrinth loader/category slug used by provider queries.
    /// Display labels intentionally differ from API slugs for hyphenated loaders.
    pub fn modrinth_slug(&self) -> Option<&'static str> {
        match self {
            Self::Forge => Some("forge"),
            Self::Fabric => Some("fabric"),
            Self::NeoForge => Some("neoforge"),
            Self::Babric => Some("babric"),
            Self::BtaBabric => Some("bta-babric"),
            Self::JavaAgent => Some("java-agent"),
            Self::LegacyFabric => Some("legacy-fabric"),
            Self::LiteLoader => Some("liteloader"),
            Self::RisugamisModLoader => Some("modloader"),
            Self::NilLoader => Some("nilloader"),
            Self::Ornithe => Some("ornithe"),
            Self::Quilt => Some("quilt"),
            Self::Rift => Some("rift"),
            Self::Unknown => None,
        }
    }

    /// Returns the CurseForge mod loader identifier used by provider queries.
    pub fn curseforge_id(&self) -> Option<i64> {
        match self {
            Self::Forge => Some(1),
            Self::LiteLoader => Some(3),
            Self::Fabric => Some(4),
            Self::Quilt => Some(5),
            Self::NeoForge => Some(6),
            Self::Babric
            | Self::BtaBabric
            | Self::JavaAgent
            | Self::LegacyFabric
            | Self::RisugamisModLoader
            | Self::NilLoader
            | Self::Ornithe
            | Self::Rift
            | Self::Unknown => None,
        }
    }

    /// Returns the CurseForge mod loader identifier used by legacy call sites.
    pub fn to_curseforge_id(&self) -> i64 {
        self.curseforge_id().unwrap_or(0)
    }
}

impl std::fmt::Display for ModLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Forge => "Forge",
            Self::Fabric => "Fabric",
            Self::NeoForge => "NeoForge",
            Self::Babric => "Babric",
            Self::BtaBabric => "BTA (Babric)",
            Self::JavaAgent => "Java Agent",
            Self::LegacyFabric => "Legacy Fabric",
            Self::LiteLoader => "LiteLoader",
            Self::RisugamisModLoader => "Risugami's ModLoader",
            Self::NilLoader => "NilLoader",
            Self::Ornithe => "Ornithe",
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
    /// Schema version of the mod config file.
    #[serde(default = "default_mod_config_version")]
    pub mod_config_version: String,
    /// Quadrant version under which this file was last saved.
    #[serde(default)]
    pub quadrant_version: String,
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
    #[serde(deserialize_with = "deserialize_integral_i64")]
    pub last_synced: i64,
    /// Stable synced modpack identifier when this modpack is linked to Quadrant Sync.
    pub modpack_id: Option<String>,
}

impl From<LocalModpack> for InstalledModpack {
    fn from(modpack: LocalModpack) -> Self {
        Self {
            mod_config_version: "2".to_string(),
            quadrant_version: quadrant_version(),
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

fn deserialize_integral_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                return Ok(value);
            }
            if let Some(value) = number.as_u64() {
                return i64::try_from(value)
                    .map_err(|_| D::Error::custom("integer is out of range for i64"));
            }
            if let Some(value) = number.as_f64()
                && value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value <= i64::MAX as f64
            {
                return Ok(value as i64);
            }
            Err(D::Error::custom("expected an integer-valued number"))
        }
        other => Err(D::Error::custom(format!(
            "expected a number for integer deserialization, got {other}"
        ))),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ALL_LOADERS: [ModLoader; 14] = [
        ModLoader::Forge,
        ModLoader::Fabric,
        ModLoader::NeoForge,
        ModLoader::Babric,
        ModLoader::BtaBabric,
        ModLoader::JavaAgent,
        ModLoader::LegacyFabric,
        ModLoader::LiteLoader,
        ModLoader::RisugamisModLoader,
        ModLoader::NilLoader,
        ModLoader::Ornithe,
        ModLoader::Quilt,
        ModLoader::Rift,
        ModLoader::Unknown,
    ];

    #[test]
    fn mod_loader_from_string_covers_all_variants_and_aliases() {
        let cases = [
            ("forge", ModLoader::Forge),
            ("Forge", ModLoader::Forge),
            ("  fabric  ", ModLoader::Fabric),
            ("neoforge", ModLoader::NeoForge),
            ("babric", ModLoader::Babric),
            ("BTA (Babric)", ModLoader::BtaBabric),
            ("bta-babric", ModLoader::BtaBabric),
            ("bta babric", ModLoader::BtaBabric),
            ("Java Agent", ModLoader::JavaAgent),
            ("java-agent", ModLoader::JavaAgent),
            ("Legacy Fabric", ModLoader::LegacyFabric),
            ("legacy-fabric", ModLoader::LegacyFabric),
            ("LiteLoader", ModLoader::LiteLoader),
            ("lite loader", ModLoader::LiteLoader),
            ("lite-loader", ModLoader::LiteLoader),
            ("Risugami's ModLoader", ModLoader::RisugamisModLoader),
            ("risugamis modloader", ModLoader::RisugamisModLoader),
            ("risugami modloader", ModLoader::RisugamisModLoader),
            ("modloader", ModLoader::RisugamisModLoader),
            ("NilLoader", ModLoader::NilLoader),
            ("nil loader", ModLoader::NilLoader),
            ("nil-loader", ModLoader::NilLoader),
            ("ornithe", ModLoader::Ornithe),
            ("quilt", ModLoader::Quilt),
            ("rift", ModLoader::Rift),
            ("Unknown", ModLoader::Unknown),
            ("", ModLoader::Unknown),
            ("paper", ModLoader::Unknown),
        ];
        for (input, expected) in cases {
            assert_eq!(ModLoader::from(input.to_string()), expected, "{input:?}");
        }
    }

    #[test]
    fn mod_loader_display_round_trips_through_from() {
        for loader in ALL_LOADERS {
            assert_eq!(ModLoader::from(loader.to_string()), loader);
        }
    }

    #[test]
    fn mod_loader_modrinth_slugs() {
        let cases = [
            (ModLoader::Forge, Some("forge")),
            (ModLoader::Fabric, Some("fabric")),
            (ModLoader::NeoForge, Some("neoforge")),
            (ModLoader::Babric, Some("babric")),
            (ModLoader::BtaBabric, Some("bta-babric")),
            (ModLoader::JavaAgent, Some("java-agent")),
            (ModLoader::LegacyFabric, Some("legacy-fabric")),
            (ModLoader::LiteLoader, Some("liteloader")),
            (ModLoader::RisugamisModLoader, Some("modloader")),
            (ModLoader::NilLoader, Some("nilloader")),
            (ModLoader::Ornithe, Some("ornithe")),
            (ModLoader::Quilt, Some("quilt")),
            (ModLoader::Rift, Some("rift")),
            (ModLoader::Unknown, None),
        ];
        for (loader, expected) in cases {
            assert_eq!(loader.modrinth_slug(), expected, "{loader}");
        }
    }

    #[test]
    fn mod_loader_curseforge_ids() {
        assert_eq!(ModLoader::Forge.curseforge_id(), Some(1));
        assert_eq!(ModLoader::LiteLoader.curseforge_id(), Some(3));
        assert_eq!(ModLoader::Fabric.curseforge_id(), Some(4));
        assert_eq!(ModLoader::Quilt.curseforge_id(), Some(5));
        assert_eq!(ModLoader::NeoForge.curseforge_id(), Some(6));
        for loader in [
            ModLoader::Babric,
            ModLoader::BtaBabric,
            ModLoader::JavaAgent,
            ModLoader::LegacyFabric,
            ModLoader::RisugamisModLoader,
            ModLoader::NilLoader,
            ModLoader::Ornithe,
            ModLoader::Rift,
            ModLoader::Unknown,
        ] {
            assert_eq!(loader.curseforge_id(), None, "{loader}");
        }
        assert_eq!(ModLoader::Forge.to_curseforge_id(), 1);
        assert_eq!(ModLoader::Unknown.to_curseforge_id(), 0);
    }

    #[test]
    fn mod_loader_serde_uses_display_labels() {
        for loader in ALL_LOADERS {
            let serialized = serde_json::to_value(loader).unwrap();
            assert_eq!(serialized, json!(loader.to_string()));
            let round_tripped: ModLoader = serde_json::from_value(serialized).unwrap();
            assert_eq!(round_tripped, loader);
        }
    }

    #[test]
    fn mod_source_serde_uses_dart_style_names() {
        assert_eq!(
            serde_json::to_value(ModSource::CurseForge).unwrap(),
            json!("ModSource.curseForge")
        );
        assert_eq!(
            serde_json::to_value(ModSource::Modrinth).unwrap(),
            json!("ModSource.modRinth")
        );
        assert_eq!(
            serde_json::to_value(ModSource::Online).unwrap(),
            json!("ModSource.online")
        );
        assert_eq!(
            serde_json::from_value::<ModSource>(json!("ModSource.curseForge")).unwrap(),
            ModSource::CurseForge
        );
        assert!(serde_json::from_value::<ModSource>(json!("curseForge")).is_err());
    }

    #[test]
    fn installed_mod_serializes_camel_case_fields() {
        let mod_ = InstalledMod {
            name: "Sodium".to_string(),
            id: "AANobbMI".to_string(),
            download_count: 7,
            version: "0.5.8".to_string(),
            mod_type: "Mod".to_string(),
            source: ModSource::Modrinth,
            slug: "sodium".to_string(),
            thumbnail_urls: vec!["https://example.invalid/thumb.png".to_string()],
            description: "desc".to_string(),
            license: "LGPL-3.0".to_string(),
            mod_icon_url: "https://example.invalid/icon.png".to_string(),
            download_url: "https://cdn.modrinth.com/sodium.jar".to_string(),
        };
        let value = serde_json::to_value(&mod_).unwrap();
        assert_eq!(value["downloadCount"], json!(7));
        assert_eq!(value["modType"], json!("Mod"));
        assert_eq!(
            value["thumbnailUrls"],
            json!(["https://example.invalid/thumb.png"])
        );
        assert_eq!(
            value["modIconUrl"],
            json!("https://example.invalid/icon.png")
        );
        assert_eq!(
            value["downloadUrl"],
            json!("https://cdn.modrinth.com/sodium.jar")
        );
        assert_eq!(value["source"], json!("ModSource.modRinth"));

        let round_tripped: InstalledMod = serde_json::from_value(value).unwrap();
        assert_eq!(round_tripped.download_count, 7);
        assert_eq!(round_tripped.mod_icon_url, mod_.mod_icon_url);
    }

    #[test]
    fn installed_mod_deserializes_with_minimal_fields() {
        let mod_: InstalledMod = serde_json::from_value(json!({
            "id": "abc",
            "source": "ModSource.curseForge",
            "downloadUrl": "https://forgecdn.net/mod.jar"
        }))
        .unwrap();
        assert_eq!(mod_.id, "abc");
        assert_eq!(mod_.source, ModSource::CurseForge);
        assert_eq!(mod_.name, "");
        assert_eq!(mod_.download_count, 0);
        assert!(mod_.thumbnail_urls.is_empty());
    }

    #[test]
    fn installed_modpack_serde_pins_field_names_and_defaults() {
        let modpack = InstalledModpack {
            mod_config_version: "2".to_string(),
            quadrant_version: "26.7.6".to_string(),
            name: "alpha".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::BtaBabric,
            mods: Vec::new(),
        };
        let value = serde_json::to_value(&modpack).unwrap();
        assert_eq!(value["modConfigVersion"], json!("2"));
        assert_eq!(value["quadrantVersion"], json!("26.7.6"));
        assert_eq!(value["modLoader"], json!("BTA (Babric)"));

        let legacy: InstalledModpack = serde_json::from_value(json!({
            "name": "legacy",
            "version": "1.12.2",
            "modLoader": "Forge",
            "mods": []
        }))
        .unwrap();
        assert_eq!(legacy.mod_config_version, "1");
        assert_eq!(legacy.quadrant_version, "");
    }

    #[test]
    fn local_modpack_serde_pins_field_names() {
        let modpack: LocalModpack = serde_json::from_value(json!({
            "name": "alpha",
            "version": "1.20.1",
            "modLoader": "Fabric",
            "mods": [],
            "unknownMods": true,
            "isApplied": false,
            "lastSynced": 42000,
            "modpackId": "pack-1"
        }))
        .unwrap();
        assert!(modpack.unknown_mods);
        assert!(!modpack.is_applied);
        assert_eq!(modpack.last_synced, 42_000);
        assert_eq!(modpack.modpack_id.as_deref(), Some("pack-1"));

        let value = serde_json::to_value(&modpack).unwrap();
        assert_eq!(value["unknownMods"], json!(true));
        assert_eq!(value["isApplied"], json!(false));
        assert_eq!(value["lastSynced"], json!(42_000));
        assert_eq!(value["modpackId"], json!("pack-1"));
    }

    #[test]
    fn local_modpack_last_synced_accepts_integral_numbers_only() {
        let base = |last_synced: Value| {
            json!({
                "name": "alpha",
                "version": "1.20.1",
                "modLoader": "Fabric",
                "mods": [],
                "unknownMods": false,
                "isApplied": false,
                "lastSynced": last_synced,
                "modpackId": null
            })
        };

        let from_int: LocalModpack = serde_json::from_value(base(json!(1234))).unwrap();
        assert_eq!(from_int.last_synced, 1234);
        let from_float: LocalModpack = serde_json::from_value(base(json!(1234.0))).unwrap();
        assert_eq!(from_float.last_synced, 1234);
        let from_negative: LocalModpack = serde_json::from_value(base(json!(-5.0))).unwrap();
        assert_eq!(from_negative.last_synced, -5);
        let from_u64: LocalModpack =
            serde_json::from_value(base(json!(u64::from(u32::MAX)))).unwrap();
        assert_eq!(from_u64.last_synced, i64::from(u32::MAX));

        assert!(serde_json::from_value::<LocalModpack>(base(json!(12.5))).is_err());
        assert!(serde_json::from_value::<LocalModpack>(base(json!(u64::MAX))).is_err());
        assert!(serde_json::from_value::<LocalModpack>(base(json!("42"))).is_err());
        assert!(serde_json::from_value::<LocalModpack>(base(json!(null))).is_err());
    }

    #[test]
    fn installed_modpack_from_local_modpack_stamps_versions() {
        let local = LocalModpack {
            name: "alpha".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Quilt,
            mods: vec![InstalledMod::minimal(
                "mod-1".to_string(),
                ModSource::Online,
                "https://example.invalid/mod.jar".to_string(),
            )],
            unknown_mods: true,
            is_applied: true,
            last_synced: 99,
            modpack_id: Some("pack-1".to_string()),
        };
        let installed = InstalledModpack::from(local);
        assert_eq!(installed.mod_config_version, "2");
        assert_eq!(installed.quadrant_version, quadrant_version());
        assert_eq!(installed.name, "alpha");
        assert_eq!(installed.version, "1.20.1");
        assert_eq!(installed.mod_loader, ModLoader::Quilt);
        assert_eq!(installed.mods.len(), 1);
    }

    #[test]
    fn local_modpack_from_installed_tuple_defaults_extras() {
        let installed = InstalledModpack {
            mod_config_version: "2".to_string(),
            quadrant_version: "x".to_string(),
            name: "alpha".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Forge,
            mods: Vec::new(),
        };
        let local = LocalModpack::from((installed, true, 7));
        assert!(local.is_applied);
        assert_eq!(local.last_synced, 7);
        assert_eq!(local.modpack_id, None);
        assert!(!local.unknown_mods);
    }

    #[test]
    fn modpack_path_appends_modpacks_folder() {
        assert_eq!(
            modpack_path(Path::new("/mc"), "alpha"),
            PathBuf::from("/mc/modpacks/alpha")
        );
    }
}
