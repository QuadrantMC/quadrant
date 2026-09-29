//! Cross-provider search logic ported from the search page
//! (`SearchPage.tsx` and `searchLogic.ts`): which providers a search runs
//! on, how their categories line up, and how their results merge.

use std::collections::HashMap;

use clap::ValueEnum;
use quadrant_core::{
    mc_mod::{Mod, SearchCategory},
    models::{ModLoader, ModSource},
};
use serde::Serialize;

/// A category both providers may offer under different ids, so one choice
/// filters each provider by its own id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergedCategory {
    pub key: String,
    pub name: String,
    pub header: String,
    pub cf_id: Option<String>,
    pub mr_id: Option<String>,
}

impl MergedCategory {
    fn id_for(&self, source: &ModSource) -> Option<&str> {
        match source {
            ModSource::CurseForge => self.cf_id.as_deref(),
            ModSource::Modrinth => self.mr_id.as_deref(),
            ModSource::Online => None,
        }
    }

    /// The only provider that has this category, when just one has it.
    fn only_on(&self) -> Option<ModSource> {
        match (&self.cf_id, &self.mr_id) {
            (Some(_), None) => Some(ModSource::CurseForge),
            (None, Some(_)) => Some(ModSource::Modrinth),
            _ => None,
        }
    }
}

/// CurseForge uses long editorial category names and Modrinth short slugs;
/// without these the two never collapse into one choice.
const CATEGORY_ALIASES: &[(&str, &str)] = &[
    ("worldgeneration", "worldgen"),
    ("adventureandrpg", "adventure"),
    ("apiandlibrary", "library"),
    ("libraryandapi", "library"),
    ("armortoolsandweapons", "equipment"),
    ("playertransport", "transportation"),
    ("utilityqol", "utility"),
    ("utilityandqol", "utility"),
];

pub fn canonical_name(name: &str) -> String {
    let compact: String = name
        .to_lowercase()
        .replace('&', "and")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    CATEGORY_ALIASES
        .iter()
        .find(|(alias, _)| *alias == compact)
        .map(|(_, canonical)| canonical.to_string())
        .unwrap_or(compact)
}

/// Modrinth's categories go first so its shorter label names a merged one.
pub fn merge_categories(cf: &[SearchCategory], mr: &[SearchCategory]) -> Vec<MergedCategory> {
    let mut merged: Vec<MergedCategory> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let tagged = mr
        .iter()
        .map(|category| (category, ModSource::Modrinth))
        .chain(cf.iter().map(|category| (category, ModSource::CurseForge)));
    for (category, source) in tagged {
        let key = format!("{}:{}", category.header, canonical_name(&category.name));
        let position = *index.entry(key.clone()).or_insert_with(|| {
            merged.push(MergedCategory {
                key,
                name: category.name.clone(),
                header: category.header.clone(),
                cf_id: None,
                mr_id: None,
            });
            merged.len() - 1
        });
        let entry = &mut merged[position];
        match source {
            ModSource::CurseForge => entry.cf_id = Some(category.id.clone()),
            _ => entry.mr_id = Some(category.id.clone()),
        }
    }
    merged
}

/// Finds a category by its name (as `canonical_name` sees it), its merged
/// key, or either provider's id.
pub fn find_category<'a>(merged: &'a [MergedCategory], wanted: &str) -> Option<&'a MergedCategory> {
    let canonical = canonical_name(wanted);
    merged.iter().find(|category| {
        category.key == wanted
            || canonical_name(&category.name) == canonical
            || category.cf_id.as_deref() == Some(wanted)
            || category.mr_id.as_deref() == Some(wanted)
    })
}

/// One provider a search runs on, with the category ids it understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderQuery {
    pub source: ModSource,
    pub categories: Vec<String>,
}

/// The providers a search runs on, in the page's stable CurseForge-then-
/// Modrinth order. A category only one provider has locks the search to it,
/// open-source only rules out CurseForge, and a loader a provider can't
/// filter by rules that provider out.
pub fn plan_providers(
    curseforge: bool,
    modrinth: bool,
    selected: &[MergedCategory],
    open_source: bool,
    loader: Option<ModLoader>,
) -> Vec<ProviderQuery> {
    let lock = selected.iter().find_map(MergedCategory::only_on);
    let supports_loader = |source: &ModSource| match (loader, source) {
        (None | Some(ModLoader::Unknown), _) => true,
        (Some(loader), ModSource::CurseForge) => loader.curseforge_id().is_some(),
        (Some(loader), _) => loader.modrinth_slug().is_some(),
    };
    let candidates = [
        (
            ModSource::CurseForge,
            curseforge && lock != Some(ModSource::Modrinth) && !open_source,
        ),
        (
            ModSource::Modrinth,
            modrinth && lock != Some(ModSource::CurseForge),
        ),
    ];
    candidates
        .into_iter()
        .filter(|(source, enabled)| *enabled && supports_loader(source))
        .map(|(source, _)| ProviderQuery {
            categories: selected
                .iter()
                .filter_map(|category| category.id_for(&source).map(str::to_string))
                .collect(),
            source,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SortKey {
    Relevance,
    Downloads,
    Name,
    Updated,
}

impl SortKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Downloads => "downloads",
            Self::Name => "name",
            Self::Updated => "updated",
        }
    }
}

/// Merges per-provider result lists into one. Relevance keeps each
/// provider's own ranking by interleaving; the other keys sort the union.
pub fn order_results(lists: Vec<Vec<Mod>>, sort: SortKey) -> Vec<Mod> {
    if sort == SortKey::Relevance {
        let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
        let mut iterators: Vec<_> = lists.into_iter().map(Vec::into_iter).collect();
        let mut merged = Vec::new();
        for _ in 0..longest {
            merged.extend(iterators.iter_mut().filter_map(Iterator::next));
        }
        return merged;
    }

    let mut merged: Vec<Mod> = lists.into_iter().flatten().collect();
    match sort {
        SortKey::Name => merged.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        }),
        SortKey::Updated => {
            merged.sort_by_key(|mod_| std::cmp::Reverse(date_value(&mod_.date_modified)))
        }
        _ => merged.sort_by_key(|mod_| std::cmp::Reverse(mod_.download_count)),
    }
    merged
}

/// Milliseconds of an RFC 3339 timestamp; unknown dates sort oldest.
fn date_value(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.timestamp_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quadrant_core::mc_mod::ModType;

    fn category(id: &str, name: &str, source: ModSource) -> SearchCategory {
        SearchCategory {
            id: id.to_string(),
            name: name.to_string(),
            header: "categories".to_string(),
            source,
        }
    }

    fn found(name: &str, source: ModSource, downloads: i64, date: &str) -> Mod {
        Mod {
            name: name.to_string(),
            id: name.to_lowercase(),
            download_count: downloads,
            version: String::new(),
            date_modified: date.to_string(),
            mod_type: ModType::Mod,
            source,
            slug: name.to_lowercase(),
            thumbnail_urls: Vec::new(),
            url: String::new(),
            description: String::new(),
            license: String::new(),
            mod_icon_url: String::new(),
            downloadable: true,
            show_previous_version: false,
            new_version: None,
            deleteable: false,
            autoinstallable: false,
            selectable: false,
            modpack: None,
            select_url: None,
        }
    }

    fn names(mods: &[Mod]) -> Vec<&str> {
        mods.iter().map(|mod_| mod_.name.as_str()).collect()
    }

    #[test]
    fn canonical_names_collapse_provider_spellings() {
        assert_eq!(canonical_name("World Generation"), "worldgen");
        assert_eq!(canonical_name("Adventure & RPG"), "adventure");
        assert_eq!(canonical_name("API and Library"), "library");
        assert_eq!(canonical_name("Utility & QoL"), "utility");
        assert_eq!(canonical_name("Magic"), "magic");
    }

    #[test]
    fn merged_categories_carry_both_ids_and_the_modrinth_label() {
        let merged = merge_categories(
            &[
                category("406", "World Generation", ModSource::CurseForge),
                category("419", "Magic", ModSource::CurseForge),
            ],
            &[
                category("worldgen", "Worldgen", ModSource::Modrinth),
                category("optimization", "Optimization", ModSource::Modrinth),
            ],
        );
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].name, "Worldgen");
        assert_eq!(merged[0].cf_id.as_deref(), Some("406"));
        assert_eq!(merged[0].mr_id.as_deref(), Some("worldgen"));
        assert_eq!(merged[2].key, "categories:magic");
        assert_eq!(merged[2].mr_id, None);
        assert_eq!(find_category(&merged, "world generation"), Some(&merged[0]));
        assert_eq!(find_category(&merged, "419"), Some(&merged[2]));
        assert_eq!(find_category(&merged, "optimization"), Some(&merged[1]));
        assert_eq!(find_category(&merged, "storage"), None);
    }

    #[test]
    fn providers_follow_settings_categories_open_source_and_loader() {
        let both = plan_providers(true, true, &[], false, None);
        assert_eq!(
            both.iter()
                .map(|query| query.source.clone())
                .collect::<Vec<_>>(),
            [ModSource::CurseForge, ModSource::Modrinth]
        );

        let magic = MergedCategory {
            key: "categories:magic".to_string(),
            name: "Magic".to_string(),
            header: "categories".to_string(),
            cf_id: Some("419".to_string()),
            mr_id: None,
        };
        assert_eq!(
            plan_providers(true, true, std::slice::from_ref(&magic), false, None),
            [ProviderQuery {
                source: ModSource::CurseForge,
                categories: vec!["419".to_string()],
            }]
        );
        assert!(plan_providers(true, true, &[magic], true, None).is_empty());

        let open_source = plan_providers(true, true, &[], true, None);
        assert_eq!(open_source.len(), 1);
        assert_eq!(open_source[0].source, ModSource::Modrinth);

        let babric = plan_providers(true, true, &[], false, Some(ModLoader::Babric));
        assert_eq!(babric.len(), 1);
        assert_eq!(babric[0].source, ModSource::Modrinth);

        assert!(plan_providers(false, false, &[], false, None).is_empty());
    }

    #[test]
    fn relevance_interleaves_provider_rankings() {
        let cf = vec![
            found("A", ModSource::CurseForge, 1, ""),
            found("B", ModSource::CurseForge, 1, ""),
            found("C", ModSource::CurseForge, 1, ""),
        ];
        let mr = vec![found("x", ModSource::Modrinth, 1, "")];
        assert_eq!(
            names(&order_results(vec![cf, mr], SortKey::Relevance)),
            ["A", "x", "B", "C"]
        );
    }

    #[test]
    fn other_keys_sort_the_union() {
        let lists = || {
            vec![
                vec![
                    found("beta", ModSource::CurseForge, 10, "2024-01-01T00:00:00Z"),
                    found("Alpha", ModSource::CurseForge, 5, "not a date"),
                ],
                vec![found(
                    "gamma",
                    ModSource::Modrinth,
                    50,
                    "2025-06-01T00:00:00Z",
                )],
            ]
        };
        assert_eq!(
            names(&order_results(lists(), SortKey::Downloads)),
            ["gamma", "beta", "Alpha"]
        );
        assert_eq!(
            names(&order_results(lists(), SortKey::Name)),
            ["Alpha", "beta", "gamma"]
        );
        assert_eq!(
            names(&order_results(lists(), SortKey::Updated)),
            ["gamma", "beta", "Alpha"]
        );
    }
}
