/** @format */

import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import { ContentLocation, ModType } from "./intefaces";
import {
  isInstallable,
  isPackType,
  locationOptionLabel,
  locationTitle,
  sectionTitle,
} from "./contentLocations";

/** Answers with the key itself, so a translated string is visible as its key. */
const t = ((key: string, values?: Record<string, unknown>) =>
  values === undefined ? key : `${key}:${JSON.stringify(values)}`) as TFunction;

function location(over: Partial<ContentLocation>): ContentLocation {
  return {
    id: "minecraft",
    kind: "minecraft",
    name: "",
    path: "/home/me/.minecraft",
    sections: [],
    ...over,
  };
}

const minecraft = location({});
const survival = location({ id: "prism:1", kind: "prism", name: "Survival" });

describe("isInstallable", () => {
  it("accepts what the install flow can write to disk", () => {
    expect(isInstallable(ModType.Mod)).toBe(true);
    expect(isInstallable(ModType.ResourcePack)).toBe(true);
    expect(isInstallable(ModType.ShaderPack)).toBe(true);
  });

  it("rejects what can only be browsed", () => {
    expect(isInstallable(ModType.Modpack)).toBe(false);
    expect(isInstallable(ModType.DataPack)).toBe(false);
    expect(isInstallable(ModType.Unknown)).toBe(false);
  });
});

describe("isPackType", () => {
  it("accepts the kinds that live in a folder of their own", () => {
    expect(isPackType(ModType.ResourcePack)).toBe(true);
    expect(isPackType(ModType.ShaderPack)).toBe(true);
  });

  it("rejects the kinds that follow their modpack instead", () => {
    expect(isPackType(ModType.Mod)).toBe(false);
    expect(isPackType(ModType.Modpack)).toBe(false);
    expect(isPackType(ModType.DataPack)).toBe(false);
    expect(isPackType(ModType.Unknown)).toBe(false);
  });
});

describe("sectionTitle", () => {
  it("names each kind of pack by its own translated title", () => {
    expect(sectionTitle(ModType.ResourcePack, t)).toBe("contentResourcePacks");
    expect(sectionTitle(ModType.ShaderPack, t)).toBe("contentShaders");
  });

  it("falls back to the raw kind for a section it has no title for", () => {
    // A host that starts listing another kind still renders something usable.
    expect(sectionTitle(ModType.DataPack, t)).toBe("DataPack");
  });
});

describe("locationTitle", () => {
  it("calls the Minecraft folder by its translated name", () => {
    expect(locationTitle(minecraft, t)).toBe("installedContentMinecraft");
  });

  it("calls a Prism instance by the name the launcher gave it", () => {
    expect(locationTitle(survival, t)).toBe("Survival");
  });
});

describe("locationOptionLabel", () => {
  it("says which launcher owns a Prism instance", () => {
    expect(locationOptionLabel(survival, t)).toBe(
      'installedContentPrismOption:{"name":"Survival"}',
    );
  });

  it("leaves the Minecraft folder's own name alone", () => {
    expect(locationOptionLabel(minecraft, t)).toBe("installedContentMinecraft");
  });
});
