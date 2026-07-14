/** @format */

import type { IMod, SearchCategory } from "../../../intefaces";

/** A category/facet unified across providers so a single row can carry both a
 *  CurseForge numeric id and a Modrinth slug. */
export interface MergedCategory {
  key: string;
  name: string;
  header: string;
  cfId?: string;
  mrId?: string;
}

// Known CurseForge↔Modrinth category-name equivalences, keyed by the normalized
// (lowercased, punctuation-stripped) name. CurseForge uses long editorial names
// while Modrinth uses short slugs, so without these aliases the two providers'
// facets never collapse into one "both" row and picking any category would
// source-lock the search to a single provider.
export const CATEGORY_ALIASES: Record<string, string> = {
  worldgeneration: "worldgen",
  adventureandrpg: "adventure",
  apiandlibrary: "library",
  libraryandapi: "library",
  armortoolsandweapons: "equipment",
  playertransport: "transportation",
  utilityqol: "utility",
  utilityandqol: "utility",
};

/** Canonical merge token for a category name: lowercased, punctuation-stripped,
 *  then mapped through {@link CATEGORY_ALIASES}. */
export function canonicalName(name: string): string {
  const compact = name
    .toLowerCase()
    .replace(/&/g, "and")
    .replace(/[^a-z0-9]+/g, "");
  return CATEGORY_ALIASES[compact] ?? compact;
}

export function mergeCategories(
  cf: SearchCategory[],
  mr: SearchCategory[],
): MergedCategory[] {
  const map = new Map<string, MergedCategory>();
  const add = (category: SearchCategory, which: "cf" | "mr") => {
    const key = `${category.header}:${canonicalName(category.name)}`;
    let merged = map.get(key);
    if (!merged) {
      merged = { key, name: category.name, header: category.header };
      map.set(key, merged);
    }
    if (which === "cf") {
      merged.cfId = category.id;
    } else {
      merged.mrId = category.id;
    }
  };
  // Modrinth first so its shorter, cleaner label wins for merged rows; Map
  // preserves insertion order, so no separate ordering array is needed.
  mr.forEach((category) => add(category, "mr"));
  cf.forEach((category) => add(category, "cf"));
  return [...map.values()];
}

export function orderResults(
  lists: IMod[][],
  key: string,
  locale: string,
): IMod[] {
  // Relevance: each provider already returns its own ranked list, so preserve
  // those rankings by round-robin interleaving rather than re-sorting.
  if (key === "relevance") {
    const out: IMod[] = [];
    const longest = lists.reduce((max, list) => Math.max(max, list.length), 0);
    for (let i = 0; i < longest; i++) {
      for (const list of lists) {
        if (i < list.length) out.push(list[i]);
      }
    }
    return out;
  }

  const flat = lists.flat();
  if (key === "name") {
    return flat.sort((a, b) => a.name.localeCompare(b.name, locale));
  }
  if (key === "updated") {
    return flat.sort(
      (a, b) => dateValue(b.dateModified) - dateValue(a.dateModified),
    );
  }
  // downloads (default)
  return flat.sort((a, b) => b.downloadCount - a.downloadCount);
}

/** Parses an RFC 3339 timestamp to millis; unknown/blank sorts oldest. */
export function dateValue(value: string): number {
  if (!value) return 0;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? 0 : parsed;
}
