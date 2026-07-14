/** @format */

import { describe, expect, it } from "vitest";
import { ModLoader, ModSource } from "./intefaces";
import {
  MOD_LOADER_OPTIONS,
  getModLoaderOptions,
  loaderProvidersForSource,
  loaderProvidersFromSettings,
  loaderSupportsProvider,
} from "./modLoaders";

describe("getModLoaderOptions", () => {
  it("returns every option when no provider filter is given", () => {
    expect(getModLoaderOptions()).toEqual(MOD_LOADER_OPTIONS);
  });

  it("treats an empty provider list as 'all providers'", () => {
    expect(getModLoaderOptions([])).toEqual(MOD_LOADER_OPTIONS);
  });

  it("only returns CurseForge-capable loaders for a CurseForge-only filter", () => {
    const options = getModLoaderOptions([ModSource.CurseForge]);
    expect(options.map((o) => o.value)).toEqual([
      ModLoader.Fabric,
      ModLoader.Forge,
      ModLoader.NeoForge,
      ModLoader.LiteLoader,
      ModLoader.Quilt,
    ]);
  });

  it("returns all options for a Modrinth-only filter (Modrinth supports every loader)", () => {
    expect(getModLoaderOptions([ModSource.Modrinth])).toEqual(
      MOD_LOADER_OPTIONS,
    );
  });
});

describe("loaderProvidersFromSettings", () => {
  it("maps enabled settings to their providers", () => {
    expect(loaderProvidersFromSettings(true, false)).toEqual([
      ModSource.CurseForge,
    ]);
    expect(loaderProvidersFromSettings(false, true)).toEqual([
      ModSource.Modrinth,
    ]);
    expect(loaderProvidersFromSettings(true, true)).toEqual([
      ModSource.CurseForge,
      ModSource.Modrinth,
    ]);
  });

  it("falls back to all providers when nothing is enabled", () => {
    expect(loaderProvidersFromSettings(false, false)).toEqual([
      ModSource.CurseForge,
      ModSource.Modrinth,
    ]);
    expect(loaderProvidersFromSettings(null, undefined)).toEqual([
      ModSource.CurseForge,
      ModSource.Modrinth,
    ]);
  });
});

describe("loaderProvidersForSource", () => {
  it("returns the single matching provider for a provider source", () => {
    expect(loaderProvidersForSource(ModSource.CurseForge)).toEqual([
      ModSource.CurseForge,
    ]);
    expect(loaderProvidersForSource(ModSource.Modrinth)).toEqual([
      ModSource.Modrinth,
    ]);
  });

  it("returns all providers for non-provider sources", () => {
    expect(loaderProvidersForSource(ModSource.Online)).toEqual([
      ModSource.CurseForge,
      ModSource.Modrinth,
    ]);
  });
});

describe("loaderSupportsProvider", () => {
  it("always matches when the loader is unknown or empty", () => {
    expect(loaderSupportsProvider(ModLoader.Unknown, ModSource.CurseForge)).toBe(
      true,
    );
    expect(loaderSupportsProvider("", ModSource.Modrinth)).toBe(true);
  });

  it("checks the provider list of the loader", () => {
    expect(loaderSupportsProvider(ModLoader.Rift, ModSource.Modrinth)).toBe(
      true,
    );
    expect(loaderSupportsProvider(ModLoader.Rift, ModSource.CurseForge)).toBe(
      false,
    );
    expect(loaderSupportsProvider(ModLoader.Quilt, ModSource.CurseForge)).toBe(
      true,
    );
  });

  it("rejects loaders that are not in the option table", () => {
    expect(loaderSupportsProvider("not-a-loader", ModSource.Modrinth)).toBe(
      false,
    );
  });
});
