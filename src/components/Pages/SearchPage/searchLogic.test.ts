/** @format */

import { describe, expect, it } from "vitest";
import { ModSource, type IMod, type SearchCategory } from "../../../intefaces";
import {
  canonicalName,
  dateValue,
  mergeCategories,
  orderResults,
} from "./searchLogic";

function cat(
  id: string,
  name: string,
  source: ModSource,
  header = "categories",
): SearchCategory {
  return { id, name, header, source };
}

function mod(over: Partial<IMod>): IMod {
  return {
    name: "",
    id: "",
    downloadCount: 0,
    version: "",
    dateModified: "",
    slug: "",
    thumbnailUrls: [],
    url: "",
    description: "",
    license: "",
    modIconUrl: "",
    downloadable: true,
    showPreviousVersion: false,
    newVersion: null,
    deleteable: false,
    autoinstallable: false,
    selectable: false,
    modpack: null,
    selectUrl: null,
    ...over,
  } as IMod;
}

describe("canonicalName", () => {
  it("lowercases, strips punctuation and maps & to and", () => {
    expect(canonicalName("World Generation")).toBe("worldgen");
    expect(canonicalName("Adventure & RPG")).toBe("adventure");
    expect(canonicalName("API and Library")).toBe("library");
  });

  it("passes through names with no alias", () => {
    expect(canonicalName("Magic")).toBe("magic");
  });
});

describe("mergeCategories", () => {
  it("collapses equivalent CurseForge and Modrinth facets into one row", () => {
    const merged = mergeCategories(
      [cat("6", "World Generation", ModSource.CurseForge)],
      [cat("worldgen", "Worldgen", ModSource.Modrinth)],
    );
    expect(merged).toHaveLength(1);
    expect(merged[0].cfId).toBe("6");
    expect(merged[0].mrId).toBe("worldgen");
    // Modrinth is added first, so its cleaner label wins.
    expect(merged[0].name).toBe("Worldgen");
  });

  it("keeps provider-only facets separate", () => {
    const merged = mergeCategories(
      [cat("1", "Only CF", ModSource.CurseForge)],
      [cat("only-mr", "Only MR", ModSource.Modrinth)],
    );
    expect(merged).toHaveLength(2);
    expect(merged.map((m) => m.name)).toEqual(["Only MR", "Only CF"]);
  });

  it("does not merge across different headers", () => {
    const merged = mergeCategories(
      [cat("1", "Fast", ModSource.CurseForge, "performance impact")],
      [cat("fast", "Fast", ModSource.Modrinth, "features")],
    );
    expect(merged).toHaveLength(2);
  });
});

describe("orderResults", () => {
  const a = mod({ name: "Alpha", downloadCount: 10, dateModified: "2024-01-01" });
  const b = mod({ name: "beta", downloadCount: 30, dateModified: "2026-06-01" });
  const c = mod({ name: "Gamma", downloadCount: 20, dateModified: "2025-01-01" });

  it("interleaves provider lists round-robin for relevance", () => {
    const result = orderResults([[a, c], [b]], "relevance", "en");
    expect(result.map((m) => m.name)).toEqual(["Alpha", "beta", "Gamma"]);
  });

  it("sorts by download count descending", () => {
    const result = orderResults([[a, b, c]], "downloads", "en");
    expect(result.map((m) => m.downloadCount)).toEqual([30, 20, 10]);
  });

  it("sorts by name using the given locale (case-insensitive)", () => {
    const result = orderResults([[a, b, c]], "name", "en");
    expect(result.map((m) => m.name)).toEqual(["Alpha", "beta", "Gamma"]);
  });

  it("sorts by modification date descending", () => {
    const result = orderResults([[a, b, c]], "updated", "en");
    expect(result.map((m) => m.name)).toEqual(["beta", "Gamma", "Alpha"]);
  });
});

describe("dateValue", () => {
  it("returns 0 for blank or unparseable input", () => {
    expect(dateValue("")).toBe(0);
    expect(dateValue("not a date")).toBe(0);
  });

  it("parses ISO timestamps to epoch millis", () => {
    expect(dateValue("2024-01-01T00:00:00Z")).toBe(Date.parse("2024-01-01T00:00:00Z"));
  });
});
