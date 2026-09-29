//! Parsers that turn command-line strings into core types, so handlers only
//! ever see a `ModSource`, `ModLoader` or `ModType`.

use quadrant_core::{
    mc_mod::ModType,
    models::{ModLoader, ModSource},
};

pub const SOURCE_HELP: &str = "curseforge (cf) or modrinth (mr)";
pub const LOADER_HELP: &str = "fabric, forge, neoforge, quilt, liteloader, babric, bta-babric, \
     java-agent, legacy-fabric, modloader, nilloader, ornithe, rift";
pub const TYPE_HELP: &str = "mod, resourcepack, shaderpack, modpack, datapack";
pub const PACK_TYPE_HELP: &str = "resourcepack or shaderpack";

pub fn source(raw: &str) -> Result<ModSource, String> {
    match raw.trim().to_lowercase().as_str() {
        "curseforge" | "cf" => Ok(ModSource::CurseForge),
        "modrinth" | "mr" => Ok(ModSource::Modrinth),
        _ => Err(format!("expected {SOURCE_HELP}")),
    }
}

pub fn loader(raw: &str) -> Result<ModLoader, String> {
    match ModLoader::from(raw.to_string()) {
        ModLoader::Unknown => Err(format!("expected one of: {LOADER_HELP}")),
        loader => Ok(loader),
    }
}

pub fn content_type(raw: &str) -> Result<ModType, String> {
    match ModType::from(raw.trim().to_string()) {
        ModType::Unknown => Err(format!("expected one of: {TYPE_HELP}")),
        mod_type => Ok(mod_type),
    }
}

/// A content type that can live in a content location: only packs can.
pub fn pack_type(raw: &str) -> Result<ModType, String> {
    match content_type(raw) {
        Ok(mod_type @ (ModType::ResourcePack | ModType::ShaderPack)) => Ok(mod_type),
        _ => Err(format!("expected {PACK_TYPE_HELP}")),
    }
}

/// The spelling the desktop frontend sends a content type in.
pub fn type_wire_name(mod_type: ModType) -> String {
    serde_json::to_value(mod_type)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The lowercase name a person types for a source.
pub fn source_name(source: &ModSource) -> &'static str {
    match source {
        ModSource::CurseForge => "curseforge",
        ModSource::Modrinth => "modrinth",
        ModSource::Online => "online",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_accept_names_and_short_aliases() {
        assert_eq!(source("CurseForge"), Ok(ModSource::CurseForge));
        assert_eq!(source("cf"), Ok(ModSource::CurseForge));
        assert_eq!(source("mr"), Ok(ModSource::Modrinth));
        assert!(source("online").is_err());
    }

    #[test]
    fn loaders_reject_unknown_values() {
        assert_eq!(loader("NeoForge"), Ok(ModLoader::NeoForge));
        assert_eq!(loader("bta-babric"), Ok(ModLoader::BtaBabric));
        assert_eq!(loader("legacy-fabric"), Ok(ModLoader::LegacyFabric));
        assert!(loader("unknown").is_err());
        assert!(loader("spigot").is_err());
    }

    #[test]
    fn every_loader_named_in_help_parses() {
        for name in LOADER_HELP.split(',') {
            assert!(loader(name.trim()).is_ok(), "{name} should parse");
        }
    }

    #[test]
    fn content_types_reject_unknown_values() {
        assert_eq!(content_type("datapack"), Ok(ModType::DataPack));
        assert_eq!(content_type("ShaderPack"), Ok(ModType::ShaderPack));
        assert!(content_type("plugin").is_err());
    }

    #[test]
    fn pack_types_only_accept_packs() {
        assert_eq!(pack_type("shaderpack"), Ok(ModType::ShaderPack));
        assert_eq!(pack_type("resourcepack"), Ok(ModType::ResourcePack));
        assert!(pack_type("mod").is_err());
    }
}
