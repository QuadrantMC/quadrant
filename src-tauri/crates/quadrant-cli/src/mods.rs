use anyhow::{Result, anyhow, bail};
use clap::{Args, Subcommand};
use futures::future::join_all;
use quadrant_core::{
    error::ErrorCode,
    mc_mod::{GlobalSearchModsArgs, Mod, ModType},
    models::{InstalledMod, LocalModpack, ModLoader, ModSource},
};
use serde::Serialize;
use serde_json::Value;

use crate::{
    Ctx, args, config, i18n,
    modpack::find_modpack,
    output::{Report, table},
    provider,
    search::{self, MergedCategory, SortKey},
};

#[derive(Debug, Subcommand)]
pub enum ModCommand {
    /// Search the enabled providers.
    Search(SearchArgs),
    /// Show a mod's details.
    Info(ModRef),
    /// List a mod's dependencies.
    Deps(ModRef),
    /// List a mod's authors and their profile pages.
    Owners(ModRef),
    /// Install a mod, resource pack or shader pack.
    Install(InstallArgs),
    /// Remove a mod from a modpack.
    Remove { modpack: String, id: String },
    /// Update one mod of a modpack to its newest compatible file.
    Update { modpack: String, id: String },
    /// List the categories `mod search --category` accepts.
    Categories {
        /// Only this provider's categories [default: every enabled provider]
        #[arg(long, value_parser = args::source)]
        source: Option<ModSource>,
        /// mod, resourcepack, shaderpack, modpack or datapack
        #[arg(long = "type", value_parser = args::content_type, default_value = "mod")]
        mod_type: ModType,
    },
}

#[derive(Debug, Args)]
pub struct ModRef {
    /// Project id (or Modrinth slug).
    id: String,
    /// curseforge (cf) or modrinth (mr)
    #[arg(long, value_parser = args::source)]
    source: ModSource,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    query: Option<String>,
    /// Search only this provider; repeat for both [default: the providers
    /// enabled in settings]
    #[arg(long = "source", value_parser = args::source)]
    sources: Vec<ModSource>,
    /// mod, resourcepack, shaderpack, modpack or datapack
    #[arg(long = "type", value_parser = args::content_type, default_value = "mod")]
    mod_type: ModType,
    /// Minecraft version [default: any, or the --modpack's]
    #[arg(long)]
    version: Option<String>,
    /// Mod loader [default: any, or the --modpack's]
    #[arg(long, value_parser = args::loader)]
    loader: Option<ModLoader>,
    /// Category name or id, see `mod categories`; repeat to match any.
    #[arg(long = "category")]
    categories: Vec<String>,
    /// Only open-source projects (Modrinth only).
    #[arg(long)]
    open_source: bool,
    #[arg(long, value_enum, default_value_t = SortKey::Relevance)]
    sort: SortKey,
    /// Skip this many results from each provider.
    #[arg(long, default_value_t = 0)]
    offset: u32,
    #[arg(long, default_value_t = 25)]
    limit: usize,
    /// Search for this modpack: its version and loader become the defaults.
    #[arg(long)]
    modpack: Option<String>,
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Project id (or Modrinth slug).
    pub id: String,
    /// curseforge (cf) or modrinth (mr)
    #[arg(long, value_parser = args::source)]
    pub source: ModSource,
    /// Modpack to install a mod into [default: the last used, the applied or
    /// the first modpack]. Packs only go into a modpack when named.
    #[arg(long)]
    pub modpack: Option<String>,
    /// Minecraft version [default: the modpack's, or the last used]
    #[arg(long)]
    pub version: Option<String>,
    /// Mod loader [default: the modpack's, or the last used]
    #[arg(long, value_parser = args::loader)]
    pub loader: Option<ModLoader>,
    /// A specific CurseForge file id.
    #[arg(long)]
    pub file_id: Option<String>,
    /// Where a resource or shader pack goes, as `content list` names it
    /// [default: the Minecraft folder and Prism instances linked to the modpack]
    #[arg(long)]
    pub location: Option<String>,
    /// Also install the mod's dependencies. Modrinth lists optional
    /// dependencies too, so this can install mods that aren't required.
    #[arg(long)]
    pub with_deps: bool,
}

pub async fn run(command: ModCommand, ctx: &Ctx) -> Result<Report> {
    let host = &ctx.host;
    match command {
        ModCommand::Search(search) => search_mods(ctx, search).await,
        ModCommand::Info(ModRef { id, source }) => {
            let mod_ = provider::get_mod(host, &source, provider::install_lookup(&id)).await?;
            Report::new(&mod_, describe_mod)
        }
        ModCommand::Deps(ModRef { id, source }) => {
            let deps = unique(provider::get_deps(host, &source, id).await?);
            Report::new(&deps, |deps| {
                if deps.is_empty() {
                    "No dependencies.".to_string()
                } else {
                    mod_table(deps)
                }
            })
        }
        ModCommand::Owners(ModRef { id, source }) => {
            let owners: Vec<Owner> = provider::get_owners(host, &source, id)
                .await?
                .into_iter()
                .map(|username| Owner {
                    url: quadrant_host::get_user_url(username.clone(), source.clone()),
                    username,
                })
                .collect();
            Report::new(&owners, |owners| {
                let rows: Vec<[String; 2]> = owners
                    .iter()
                    .map(|owner| [owner.username.clone(), owner.url.clone()])
                    .collect();
                table(&rows)
            })
        }
        ModCommand::Install(install_args) => install(ctx, install_args).await,
        ModCommand::Remove { modpack, id } => {
            host.delete_mod(modpack.clone(), id.clone()).await?;
            Ok(Report::message(format!("Removed {id} from {modpack}.")))
        }
        ModCommand::Update { modpack, id } => update(ctx, &modpack, &id).await,
        ModCommand::Categories { source, mod_type } => {
            let (curseforge, modrinth) = match source {
                Some(source) => (
                    source == ModSource::CurseForge,
                    source == ModSource::Modrinth,
                ),
                None => enabled_providers(ctx)?,
            };
            let categories = categories(ctx, mod_type, curseforge, modrinth).await?;
            Report::new(&categories, |categories| {
                let rows: Vec<[String; 3]> = categories
                    .iter()
                    .map(|category| {
                        let providers = match (&category.cf_id, &category.mr_id) {
                            (Some(_), Some(_)) => "both",
                            (Some(_), None) => "curseforge",
                            _ => "modrinth",
                        };
                        [
                            category.header.clone(),
                            category.name.clone(),
                            providers.to_string(),
                        ]
                    })
                    .collect();
                table(&rows)
            })
        }
    }
}

#[derive(Serialize)]
struct Owner {
    username: String,
    url: String,
}

/// What `mod install` did, including the choices it filled in.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Installed {
    id: String,
    name: String,
    source: ModSource,
    mod_type: ModType,
    modpack: Option<String>,
    minecraft_version: String,
    mod_loader: ModLoader,
    dependencies: Vec<String>,
    failed_dependencies: Vec<String>,
}

/// The config keys the install page and search filters remember choices in.
const LAST_USED_VERSION: &str = "lastUsedVersion";
const LAST_USED_LOADER: &str = "lastUsedAPI";
const LAST_USED_MODPACK: &str = "lastUsedModpack";

fn enabled_providers(ctx: &Ctx) -> Result<(bool, bool)> {
    Ok((
        config::get_bool(&ctx.host, "curseforge")?.unwrap_or(true),
        config::get_bool(&ctx.host, "modrinth")?.unwrap_or(true),
    ))
}

async fn categories(
    ctx: &Ctx,
    mod_type: ModType,
    curseforge: bool,
    modrinth: bool,
) -> Result<Vec<MergedCategory>> {
    let fetch = |enabled: bool, source: ModSource| async move {
        if enabled {
            ctx.host
                .get_categories(source, args::type_wire_name(mod_type))
                .await
        } else {
            Ok(Vec::new())
        }
    };
    let (cf, mr) = futures::join!(
        fetch(curseforge, ModSource::CurseForge),
        fetch(modrinth, ModSource::Modrinth)
    );
    Ok(search::merge_categories(&cf?, &mr?))
}

async fn search_mods(ctx: &Ctx, search: SearchArgs) -> Result<Report> {
    let (curseforge, modrinth) = if search.sources.is_empty() {
        enabled_providers(ctx)?
    } else {
        (
            search.sources.contains(&ModSource::CurseForge),
            search.sources.contains(&ModSource::Modrinth),
        )
    };
    let target = match &search.modpack {
        Some(name) => Some(find_modpack(&ctx.host, name).await?),
        None => None,
    };
    let is_mod = search.mod_type == ModType::Mod;
    let version = search
        .version
        .or_else(|| target.as_ref().map(|modpack| modpack.version.clone()))
        .filter(|version| version != "any")
        .unwrap_or_default();
    let loader = search
        .loader
        .or_else(|| target.as_ref().map(|modpack| modpack.mod_loader))
        .filter(|_| is_mod);

    let selected = if search.categories.is_empty() {
        Vec::new()
    } else {
        let known = categories(ctx, search.mod_type, curseforge, modrinth).await?;
        search
            .categories
            .iter()
            .map(|wanted| {
                search::find_category(&known, wanted)
                    .cloned()
                    .ok_or_else(|| {
                        anyhow!("unknown category {wanted:?}; `mod categories` lists them")
                    })
            })
            .collect::<Result<Vec<_>>>()?
    };

    let providers =
        search::plan_providers(curseforge, modrinth, &selected, search.open_source, loader);
    if providers.is_empty() {
        bail!("no provider can run this search; check --source, --open-source and --loader");
    }

    // Results only count as auto-installable into a modpack they match.
    let filter_on = target.as_ref().is_some_and(|modpack| {
        version == modpack.version && (!is_mod || loader == Some(modpack.mod_loader))
    });
    let query = search.query.unwrap_or_default().trim().to_lowercase();
    let requests = providers.into_iter().map(|provider| {
        ctx.host.search_mods(GlobalSearchModsArgs {
            source: provider.source,
            query: query.clone(),
            mod_type: args::type_wire_name(search.mod_type),
            filter_on,
            game_version: version.clone(),
            mod_loader: loader.map(|loader| loader.to_string()).unwrap_or_default(),
            categories: provider.categories,
            open_source: search.open_source,
            sort_by: search.sort.as_str().to_string(),
            offset: search.offset,
        })
    });

    let mut lists = Vec::new();
    let mut first_error = None;
    for result in join_all(requests).await {
        match result {
            Ok(list) => lists.push(list),
            Err(error) => {
                if first_error.is_some() || !lists.is_empty() {
                    ctx.out
                        .warn(format!("A provider failed: {}", i18n::describe(error)));
                } else {
                    first_error = Some(error);
                }
            }
        }
    }
    if lists.is_empty() {
        return Err(first_error.unwrap_or_else(|| anyhow!("no results")));
    }
    if let Some(error) = first_error {
        ctx.out
            .warn(format!("A provider failed: {}", i18n::describe(error)));
    }

    let mut results = search::order_results(lists, search.sort);
    results.truncate(search.limit);
    Report::new(&results, |results| {
        if results.is_empty() {
            "No results.".to_string()
        } else {
            mod_table(results)
        }
    })
}

pub async fn install(ctx: &Ctx, request: InstallArgs) -> Result<Report> {
    let host = &ctx.host;
    let mod_ =
        provider::get_mod(host, &request.source, provider::install_lookup(&request.id)).await?;
    if matches!(
        mod_.mod_type,
        ModType::Unknown | ModType::Modpack | ModType::DataPack
    ) {
        bail!(
            "Quadrant can't install {} projects; only mods, resource packs and shader packs",
            mod_.mod_type
        );
    }

    let modpacks = host.get_modpacks(true).await?;
    let versions: Vec<String> = host
        .get_versions()
        .await?
        .into_iter()
        .map(|version| version.version)
        .collect();
    let saved = SavedChoices {
        version: config::get_string(host, LAST_USED_VERSION)?,
        loader: config::get_string(host, LAST_USED_LOADER)?,
        modpack: config::get_string(host, LAST_USED_MODPACK)?,
    };
    let requested = Requested {
        modpack: request.modpack.as_deref(),
        version: request.version.as_deref(),
        loader: request.loader,
    };
    let InstallTargets {
        modpack,
        version,
        loader,
    } = resolve_install_targets(mod_.mod_type, &requested, &saved, &modpacks, &versions)?;
    let modpack = modpack.map(|modpack| modpack.name.clone());
    if version.is_empty() {
        bail!("no Minecraft version to install for; pass --version");
    }
    if mod_.mod_type == ModType::Mod && modpack.is_none() {
        bail!("mods are installed into a modpack; create one or pass --modpack");
    }

    remember_choices(ctx, &request, &modpacks)?;
    if mod_.mod_type == ModType::Mod
        && let Some(modpack) = modpacks
            .iter()
            .find(|pack| Some(&pack.name) == modpack.as_ref())
    {
        warn_if_installed(ctx, &mod_, modpack);
    }

    ctx.out.note(match (&modpack, mod_.mod_type) {
        (Some(modpack), ModType::Mod) => {
            format!("Installing into {modpack} ({version}, {loader}).")
        }
        (Some(modpack), _) => format!("Installing into {modpack} ({version})."),
        (None, _) => format!("Installing for Minecraft {version}."),
    });
    let _progress = ctx.out.track_progress(host);
    host.install_mod(
        mod_.id.clone(),
        version.clone(),
        loader,
        mod_.source.clone(),
        modpack.clone(),
        mod_.mod_type,
        request.file_id.clone(),
        request.location.clone(),
    )
    .await?;

    let mut dependencies = Vec::new();
    let mut failed_dependencies = Vec::new();
    if request.with_deps {
        let installed_mods = match &modpack {
            Some(name) => find_modpack(host, name).await?.mods,
            None => Vec::new(),
        };
        for dep in unique(provider::get_deps(host, &mod_.source, mod_.id.clone()).await?) {
            if installed_mods
                .iter()
                .any(|installed| installed.is_same_mod(&as_installed(&dep)))
            {
                continue;
            }
            let result = host
                .install_mod(
                    dep.id.clone(),
                    version.clone(),
                    loader,
                    dep.source.clone(),
                    modpack.clone(),
                    dep.mod_type,
                    None,
                    request.location.clone(),
                )
                .await;
            match result {
                Ok(()) => dependencies.push(dep.name),
                Err(error) => {
                    ctx.out.warn(format!(
                        "Couldn't install the dependency {} ({}): {}",
                        dep.name,
                        dep.id,
                        i18n::describe(error)
                    ));
                    failed_dependencies.push(dep.name);
                }
            }
        }
    }
    let failure = (!failed_dependencies.is_empty()).then(|| {
        anyhow!(
            "couldn't install {} of the dependencies: {}",
            failed_dependencies.len(),
            failed_dependencies.join(", ")
        )
    });

    let installed = Installed {
        id: mod_.id,
        name: mod_.name,
        source: mod_.source,
        mod_type: mod_.mod_type,
        modpack,
        minecraft_version: version,
        mod_loader: loader,
        dependencies,
        failed_dependencies,
    };
    Ok(Report::new(&installed, |installed| {
        let target = match &installed.modpack {
            Some(modpack) => format!(" into {modpack}"),
            None => String::new(),
        };
        let mut text = if installed.mod_type == ModType::Mod {
            format!(
                "Installed {}{target} ({}, {}).",
                installed.name, installed.minecraft_version, installed.mod_loader
            )
        } else {
            format!(
                "Installed {}{target} ({}).",
                installed.name, installed.minecraft_version
            )
        };
        if !installed.dependencies.is_empty() {
            text.push_str(&format!(
                "\nDependencies: {}",
                installed.dependencies.join(", ")
            ));
        }
        text
    })?
    .failing_with(failure))
}

/// Picking a modpack, version or loader on the install page stores it for the
/// next install; naming one on the command line does the same.
fn remember_choices(ctx: &Ctx, request: &InstallArgs, modpacks: &[LocalModpack]) -> Result<()> {
    if let Some(picked) = request
        .modpack
        .as_ref()
        .and_then(|name| modpacks.iter().find(|modpack| &modpack.name == name))
    {
        config::set(
            &ctx.host,
            LAST_USED_VERSION,
            Value::String(picked.version.clone()),
        )?;
        config::set(
            &ctx.host,
            LAST_USED_LOADER,
            Value::String(picked.mod_loader.to_string()),
        )?;
        config::set(
            &ctx.host,
            LAST_USED_MODPACK,
            Value::String(picked.name.clone()),
        )?;
    }
    if let Some(version) = &request.version {
        config::set(&ctx.host, LAST_USED_VERSION, Value::String(version.clone()))?;
    }
    if let Some(loader) = request.loader {
        config::set(
            &ctx.host,
            LAST_USED_LOADER,
            Value::String(loader.to_string()),
        )?;
    }
    Ok(())
}

/// Providers list a dependency once per file that declares it.
fn unique(mut mods: Vec<Mod>) -> Vec<Mod> {
    let mut seen = std::collections::HashSet::new();
    mods.retain(|mod_| seen.insert((args::source_name(&mod_.source), mod_.id.clone())));
    mods
}

fn as_installed(mod_: &Mod) -> InstalledMod {
    InstalledMod {
        slug: mod_.slug.clone(),
        ..InstalledMod::minimal(mod_.id.clone(), mod_.source.clone(), String::new())
    }
}

fn warn_if_installed(ctx: &Ctx, mod_: &Mod, modpack: &LocalModpack) {
    let candidate = as_installed(mod_);
    let Some(existing) = modpack
        .mods
        .iter()
        .find(|installed| installed.is_same_mod(&candidate))
    else {
        return;
    };
    if existing.source == mod_.source {
        ctx.out.note(format!(
            "{} is already in {}; installing it again.",
            mod_.name, modpack.name
        ));
    } else {
        ctx.out.note(format!(
            "{} is in {} from {}; this replaces it with the {} copy.",
            existing.name,
            modpack.name,
            args::source_name(&existing.source),
            args::source_name(&mod_.source)
        ));
    }
}

async fn update(ctx: &Ctx, modpack_name: &str, id: &str) -> Result<Report> {
    let host = &ctx.host;
    let modpack = find_modpack(host, modpack_name).await?;
    let installed = modpack
        .mods
        .iter()
        .find(|mod_| mod_.id == id)
        .ok_or_else(|| anyhow!("{modpack_name} has no mod with id {id}"))?;
    let mod_ = provider::get_mod(
        host,
        &installed.source,
        provider::modpack_entry(id, &modpack),
    )
    .await?;
    let update = host
        .check_mod_updates(
            mod_,
            modpack.version.clone(),
            modpack.mod_loader,
            modpack.name.clone(),
        )
        .await?;
    let Some((update, file)) = update.and_then(|update| {
        let file = update.new_version.clone().filter(|_| update.downloadable)?;
        Some((update, file))
    }) else {
        return Ok(Report::message(format!(
            "{} is up to date.",
            installed.name
        )));
    };

    let _progress = ctx.out.track_progress(host);
    let file_name = file.file_name.clone();
    host.install_remote_file(
        file,
        update.mod_type,
        Some(modpack.name.clone()),
        update.source.clone(),
        update.id.clone(),
    )
    .await?;
    Report::new(&update, |update| {
        format!("Updated {} to {file_name}.", update.name)
    })
}

/// The config the install page starts from.
#[derive(Debug, Default)]
pub struct SavedChoices {
    pub version: Option<String>,
    pub loader: Option<String>,
    pub modpack: Option<String>,
}

/// What the command line named.
#[derive(Debug, Default)]
pub struct Requested<'a> {
    pub modpack: Option<&'a str>,
    pub version: Option<&'a str>,
    pub loader: Option<ModLoader>,
}

#[derive(Debug)]
pub struct InstallTargets<'a> {
    pub modpack: Option<&'a LocalModpack>,
    pub version: String,
    pub loader: ModLoader,
}

/// The modpack, version and loader to install for, starting from what the
/// install page opens with (`ModInstallPage.tsx`). A mod goes into the named
/// modpack, else the last used, the applied or the first one; a pack only
/// into a named modpack. Version and loader come from the command line, else
/// the target modpack, so a file never lands in a modpack it can't run in,
/// else the saved choices while they are still valid.
pub fn resolve_install_targets<'a>(
    mod_type: ModType,
    requested: &Requested,
    saved: &SavedChoices,
    modpacks: &'a [LocalModpack],
    versions: &[String],
) -> Result<InstallTargets<'a>> {
    let named = |name: &str| modpacks.iter().find(|modpack| modpack.name == name);
    let explicit_target = match requested.modpack {
        Some(name) => Some(named(name).ok_or(ErrorCode::ModpackMissing)?),
        None => None,
    };
    let target = if mod_type == ModType::Mod {
        explicit_target
            .or_else(|| saved.modpack.as_deref().and_then(named))
            .or_else(|| modpacks.iter().find(|modpack| modpack.is_applied))
            .or_else(|| modpacks.first())
    } else {
        explicit_target
    };

    let saved_version = saved
        .version
        .as_deref()
        .filter(|wanted| versions.iter().any(|version| version == wanted));
    let version = requested
        .version
        .or_else(|| target.map(|modpack| modpack.version.as_str()))
        .or(saved_version)
        .map(str::to_string)
        .or_else(|| versions.first().cloned())
        .unwrap_or_default();

    let saved_loader = saved
        .loader
        .as_deref()
        .filter(|loader| !loader.is_empty())
        .map(|loader| ModLoader::from(loader.to_string()));
    let loader = requested
        .loader
        .or_else(|| target.map(|modpack| modpack.mod_loader))
        .or(saved_loader)
        .unwrap_or(ModLoader::Unknown);

    Ok(InstallTargets {
        modpack: target,
        version,
        loader,
    })
}

fn mod_table(mods: &[Mod]) -> String {
    let rows: Vec<[String; 4]> = mods
        .iter()
        .map(|mod_| {
            [
                mod_.name.clone(),
                args::source_name(&mod_.source).to_string(),
                mod_.id.clone(),
                format!("{} downloads", mod_.download_count),
            ]
        })
        .collect();
    table(&rows)
}

fn describe_mod(mod_: &Mod) -> String {
    let mut rows = vec![
        ["name".to_string(), mod_.name.clone()],
        ["id".to_string(), mod_.id.clone()],
        [
            "source".to_string(),
            args::source_name(&mod_.source).to_string(),
        ],
        ["type".to_string(), mod_.mod_type.to_string()],
        ["downloads".to_string(), mod_.download_count.to_string()],
        ["license".to_string(), mod_.license.clone()],
        ["updated".to_string(), mod_.date_modified.clone()],
        ["url".to_string(), mod_.url.clone()],
    ];
    rows.retain(|[_, value]| !value.is_empty());
    format!("{}\n\n{}", table(&rows), mod_.description)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modpack(name: &str, version: &str, loader: ModLoader, is_applied: bool) -> LocalModpack {
        LocalModpack {
            name: name.to_string(),
            version: version.to_string(),
            mod_loader: loader,
            mods: Vec::new(),
            unknown_mods: false,
            is_applied,
            last_synced: 0,
            modpack_id: None,
        }
    }

    fn versions() -> Vec<String> {
        ["1.21", "1.20.1"].map(str::to_string).to_vec()
    }

    fn named(modpack: &str) -> Requested<'_> {
        Requested {
            modpack: Some(modpack),
            ..Requested::default()
        }
    }

    fn pack_name<'a>(targets: &InstallTargets<'a>) -> Option<&'a str> {
        targets.modpack.map(|pack| pack.name.as_str())
    }

    #[test]
    fn a_named_modpack_decides_everything() {
        let packs = [
            modpack("a", "1.20.1", ModLoader::Forge, true),
            modpack("b", "1.21", ModLoader::Fabric, false),
        ];
        let saved = SavedChoices {
            version: Some("1.20.1".to_string()),
            loader: Some("Quilt".to_string()),
            modpack: Some("a".to_string()),
        };
        let targets =
            resolve_install_targets(ModType::Mod, &named("b"), &saved, &packs, &versions())
                .unwrap();
        assert_eq!(pack_name(&targets), Some("b"));
        assert_eq!(targets.version, "1.21");
        assert_eq!(targets.loader, ModLoader::Fabric);
    }

    #[test]
    fn a_modpack_picked_by_default_still_beats_saved_choices() {
        let packs = [modpack("fab", "1.20.1", ModLoader::Fabric, false)];
        let saved = SavedChoices {
            version: Some("1.21".to_string()),
            loader: Some("Forge".to_string()),
            modpack: Some("fab".to_string()),
        };
        let targets = resolve_install_targets(
            ModType::Mod,
            &Requested::default(),
            &saved,
            &packs,
            &versions(),
        )
        .unwrap();
        assert_eq!(pack_name(&targets), Some("fab"));
        assert_eq!(targets.version, "1.20.1");
        assert_eq!(targets.loader, ModLoader::Fabric);
    }

    #[test]
    fn the_command_line_beats_the_modpack() {
        let packs = [modpack("fab", "1.20.1", ModLoader::Fabric, true)];
        let requested = Requested {
            modpack: Some("fab"),
            version: Some("1.21"),
            loader: Some(ModLoader::Quilt),
        };
        let targets = resolve_install_targets(
            ModType::Mod,
            &requested,
            &SavedChoices::default(),
            &packs,
            &versions(),
        )
        .unwrap();
        assert_eq!(targets.version, "1.21");
        assert_eq!(targets.loader, ModLoader::Quilt);
    }

    #[test]
    fn mods_fall_back_to_saved_then_applied_then_first_modpack() {
        let packs = [
            modpack("first", "1.20.1", ModLoader::Forge, false),
            modpack("applied", "1.21", ModLoader::Fabric, true),
        ];
        let none = SavedChoices::default();
        let nothing = Requested::default();
        let targets =
            resolve_install_targets(ModType::Mod, &nothing, &none, &packs, &versions()).unwrap();
        assert_eq!(pack_name(&targets), Some("applied"));
        assert_eq!(targets.version, "1.21");
        assert_eq!(targets.loader, ModLoader::Fabric);

        let saved = SavedChoices {
            modpack: Some("first".to_string()),
            ..SavedChoices::default()
        };
        let targets =
            resolve_install_targets(ModType::Mod, &nothing, &saved, &packs, &versions()).unwrap();
        assert_eq!(pack_name(&targets), Some("first"));
        assert_eq!(targets.version, "1.20.1");
        assert_eq!(targets.loader, ModLoader::Forge);

        let only = [modpack("only", "1.20.1", ModLoader::Quilt, false)];
        let gone = SavedChoices {
            modpack: Some("deleted".to_string()),
            ..SavedChoices::default()
        };
        let targets =
            resolve_install_targets(ModType::Mod, &nothing, &gone, &only, &versions()).unwrap();
        assert_eq!(pack_name(&targets), Some("only"));
    }

    #[test]
    fn packs_without_a_modpack_use_saved_choices_then_the_latest_release() {
        let packs = [modpack("applied", "1.20.1", ModLoader::Fabric, true)];
        let nothing = Requested::default();
        let saved = SavedChoices {
            version: Some("1.20.1".to_string()),
            loader: Some("Forge".to_string()),
            ..SavedChoices::default()
        };
        let targets =
            resolve_install_targets(ModType::ShaderPack, &nothing, &saved, &packs, &versions())
                .unwrap();
        assert!(targets.modpack.is_none());
        assert_eq!(targets.version, "1.20.1");
        assert_eq!(targets.loader, ModLoader::Forge);

        let stale = SavedChoices {
            version: Some("9.9".to_string()),
            ..SavedChoices::default()
        };
        let targets =
            resolve_install_targets(ModType::ShaderPack, &nothing, &stale, &packs, &versions())
                .unwrap();
        assert_eq!(
            targets.version, "1.21",
            "an unavailable saved version is dropped"
        );
        assert_eq!(targets.loader, ModLoader::Unknown);
    }

    #[test]
    fn a_missing_named_modpack_is_an_error() {
        let error = resolve_install_targets(
            ModType::Mod,
            &named("nope"),
            &SavedChoices::default(),
            &[],
            &versions(),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "errorModpackMissing");
    }
}
