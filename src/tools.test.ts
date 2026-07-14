/** @format */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { ModSource } from "./intefaces";

const invoke = vi.fn();
const platform = vi.fn();
const openExternal = vi.fn();
const openDialog = vi.fn();
const writeClipboardText = vi.fn();
const storeGet = vi.fn();

vi.mock("./desktop", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  platform: (...args: unknown[]) => platform(...args),
  openExternal: (...args: unknown[]) => openExternal(...args),
  openDialog: (...args: unknown[]) => openDialog(...args),
  writeClipboardText: (...args: unknown[]) => writeClipboardText(...args),
  requestCheckForUpdates: vi.fn(),
  createDesktopStore: () => ({ get: (...args: unknown[]) => storeGet(...args) }),
}));

// Imported after the mock is registered.
import {
  applyModpack,
  exportModpack,
  getMod,
  getModDependencies,
  getModOwners,
  getModpacks,
  shareModpack,
  shuffle,
} from "./tools";

beforeEach(() => {
  vi.clearAllMocks();
});

function modpack(over: Partial<Record<string, unknown>>) {
  return {
    name: "pack",
    version: "1.20.1",
    modLoader: "fabric",
    lastSynced: 0,
    isApplied: false,
    ...over,
  };
}

describe("getModpacks", () => {
  it("sorts by lastSynced descending", async () => {
    invoke.mockResolvedValue([
      modpack({ name: "old", lastSynced: 1 }),
      modpack({ name: "new", lastSynced: 3 }),
      modpack({ name: "mid", lastSynced: 2 }),
    ]);
    const res = await getModpacks();
    expect(res.map((m) => m.name)).toEqual(["new", "mid", "old"]);
    expect(invoke).toHaveBeenCalledWith("get_modpacks", { hideFree: true });
  });

  it("floats the applied modpack to the front regardless of sync time", async () => {
    invoke.mockResolvedValue([
      modpack({ name: "a", lastSynced: 3 }),
      modpack({ name: "applied", lastSynced: 1, isApplied: true }),
      modpack({ name: "b", lastSynced: 2 }),
    ]);
    const res = await getModpacks();
    expect(res[0].name).toBe("applied");
  });

  it("filters by a case-insensitive substring across name, version and loader", async () => {
    invoke.mockResolvedValue([
      modpack({ name: "Fancy", version: "1.20.1", modLoader: "fabric" }),
      modpack({ name: "Other", version: "1.19.2", modLoader: "forge" }),
    ]);
    const byLoader = await getModpacks(true, "fabric");
    expect(byLoader.map((m) => m.name)).toEqual(["Fancy"]);

    const byVersion = await getModpacks(true, "1.19");
    expect(byVersion.map((m) => m.name)).toEqual(["Other"]);
  });

  it("passes hideFree through to the backend", async () => {
    invoke.mockResolvedValue([]);
    await getModpacks(false);
    expect(invoke).toHaveBeenCalledWith("get_modpacks", { hideFree: false });
  });
});

describe("source dispatch", () => {
  it("getMod routes to the provider-specific command", async () => {
    invoke.mockResolvedValue({ name: "m" });
    await getMod({ id: "x" } as never, ModSource.CurseForge);
    expect(invoke).toHaveBeenCalledWith("get_mod_curseforge", {
      args: { id: "x" },
    });

    await getMod({ id: "y" } as never, ModSource.Modrinth);
    expect(invoke).toHaveBeenCalledWith("get_mod_modrinth", {
      args: { id: "y" },
    });
  });

  it("getMod throws for an unknown source", async () => {
    await expect(getMod({ id: "x" } as never, ModSource.Online)).rejects.toThrow();
  });

  it("getModOwners returns [] for a non-provider source without invoking", async () => {
    const owners = await getModOwners(ModSource.Online, "1");
    expect(owners).toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("getModDependencies routes per provider", async () => {
    invoke.mockResolvedValue([]);
    await getModDependencies(ModSource.Modrinth, "42");
    expect(invoke).toHaveBeenCalledWith("get_mod_deps_modrinth", { id: "42" });
  });
});

describe("applyModpack", () => {
  it("resolves on success without touching the browser", async () => {
    invoke.mockResolvedValue(undefined);
    await applyModpack("pack");
    expect(openExternal).not.toHaveBeenCalled();
  });

  it("opens the Windows help page and rethrows on failure", async () => {
    invoke.mockRejectedValue(new Error("boom"));
    platform.mockResolvedValue("windows");
    await expect(applyModpack("pack")).rejects.toThrow("boom");
    expect(openExternal).toHaveBeenCalledWith(
      expect.stringContaining("Fixing-Windows-issues"),
    );
  });

  it("does not open the help page on non-Windows platforms", async () => {
    invoke.mockRejectedValue(new Error("boom"));
    platform.mockResolvedValue("linux");
    await expect(applyModpack("pack")).rejects.toThrow("boom");
    expect(openExternal).not.toHaveBeenCalled();
  });
});

describe("shareModpack", () => {
  it("builds the share URL from the returned code and copies it", async () => {
    invoke.mockResolvedValue({ code: 1234567 });
    await shareModpack("pack");
    expect(writeClipboardText).toHaveBeenCalledWith(
      "https://usequadrant.dev/modpack/1234567",
    );
  });
});

describe("exportModpack", () => {
  it("aborts when the save dialog is cancelled", async () => {
    openDialog.mockResolvedValue(null);
    await exportModpack("pack");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("invokes export with the chosen destination", async () => {
    openDialog.mockResolvedValue("/tmp/pack.zip");
    invoke.mockResolvedValue(undefined);
    await exportModpack("pack");
    expect(invoke).toHaveBeenCalledWith("export_modpack_to", {
      modpack: "pack",
      destination: "/tmp/pack.zip",
    });
  });
});

describe("shuffle", () => {
  it("permutes in place, preserving the multiset of elements", () => {
    const arr = Array.from({ length: 50 }, (_, i) => i);
    const copy = [...arr];
    shuffle(arr);
    expect(arr).toHaveLength(copy.length);
    expect([...arr].sort((a, b) => a - b)).toEqual(copy);
  });
});
