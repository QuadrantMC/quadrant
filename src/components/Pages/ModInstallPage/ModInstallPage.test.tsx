/** @format */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  ContentContext,
  ModLoader,
  ModSource,
  ModType,
  type IMod,
} from "../../../intefaces";

const getVersions = vi.fn();
const getModpacks = vi.fn();
const getModOwners = vi.fn();
const getModDependencies = vi.fn();
const getUserURL = vi.fn();
const installMod = vi.fn();
const getInstalledContent = vi.fn();
const openIn = vi.fn();
const storeGet = vi.fn();
const storeSet = vi.fn();
const storeSave = vi.fn();

vi.mock("../../../tools", () => ({
  getVersions: (...a: unknown[]) => getVersions(...a),
  getModpacks: (...a: unknown[]) => getModpacks(...a),
  getModOwners: (...a: unknown[]) => getModOwners(...a),
  getModDependencies: (...a: unknown[]) => getModDependencies(...a),
  getUserURL: (...a: unknown[]) => getUserURL(...a),
  installMod: (...a: unknown[]) => installMod(...a),
  getInstalledContent: (...a: unknown[]) => getInstalledContent(...a),
  openIn: (...a: unknown[]) => openIn(...a),
}));

vi.mock("../../../desktop", () => ({
  createDesktopStore: () => ({
    get: (...a: unknown[]) => storeGet(...a),
    set: (...a: unknown[]) => storeSet(...a),
    save: (...a: unknown[]) => storeSave(...a),
  }),
  listen: async () => () => {},
}));

vi.mock("../../shared/Mod", () => ({
  default: ({ mod, installed }: { mod: IMod; installed?: boolean }) => (
    <div data-testid="mod-card" data-installed={String(!!installed)}>
      {mod.name}
    </div>
  ),
}));

import ModInstallPage from "./ModInstallPage";

function mod(over: Partial<IMod>): IMod {
  return {
    name: "Sodium",
    id: "sodium",
    downloadCount: 12_400_000,
    version: "",
    dateModified: "",
    modType: ModType.Mod,
    source: ModSource.Modrinth,
    slug: "",
    thumbnailUrls: [],
    url: "https://modrinth.com/mod/sodium",
    description: "A rendering engine",
    license: "LGPL-3.0",
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
  };
}

const back = vi.fn();
const setSnackbar = vi.fn();

function renderPage(props: {
  mod: IMod;
  fileId?: string;
  installTarget?: { name: string };
  installLocation?: string;
}) {
  return render(
    <ContentContext.Provider
      value={{
        back,
        setSnackbar,
        changeContent: () => {},
        changePage: () => {},
        setSnackbarNoState: () => {},
      }}
    >
      <ModInstallPage {...props} />
    </ContentContext.Provider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  getVersions.mockResolvedValue([{ version: "1.21" }, { version: "1.20.1" }]);
  getModpacks.mockResolvedValue([
    { name: "Pack", version: "1.20.1", modLoader: ModLoader.Fabric, mods: [] },
  ]);
  getModOwners.mockResolvedValue(["jellysquid"]);
  getUserURL.mockResolvedValue("https://modrinth.com/user/jellysquid");
  getModDependencies.mockResolvedValue([]);
  installMod.mockResolvedValue(undefined);
  getInstalledContent.mockResolvedValue([]);
  storeGet.mockImplementation(async (key: string) =>
    key === "lastUsedVersion"
      ? "1.21"
      : key === "lastUsedAPI"
        ? ModLoader.Fabric
        : key === "lastUsedModpack"
          ? "Pack"
          : undefined,
  );
});

describe("ModInstallPage", () => {
  it("shows header metadata and owner chips, and no dependency panel when empty", async () => {
    renderPage({ mod: mod({}) });
    expect(screen.getByRole("heading", { name: "Sodium" })).toBeTruthy();
    expect(screen.getByText("12.4M")).toBeTruthy();
    expect(screen.getByText(/Licensed under/)).toBeTruthy();
    expect(await screen.findByText("jellysquid")).toBeTruthy();
    expect(screen.queryByText("dependencies")).toBeNull();
  });

  it("opens the owner page from the owner chip", async () => {
    renderPage({ mod: mod({}) });
    await userEvent.click(await screen.findByText("jellysquid"));
    expect(openIn).toHaveBeenCalledWith("https://modrinth.com/user/jellysquid");
  });

  it("lists dependencies as mod cards with a count badge", async () => {
    getModDependencies.mockResolvedValue([
      mod({ id: "fabric-api", name: "Fabric API" }),
    ]);
    renderPage({ mod: mod({}) });
    expect(await screen.findByTestId("mod-card")).toHaveTextContent(
      "Fabric API",
    );
    expect(screen.getByText("1")).toBeTruthy();
  });

  it("installs with the remembered version, loader and modpack", async () => {
    renderPage({ mod: mod({}) });
    await waitFor(() =>
      expect(
        screen.getByRole("combobox", { name: /Choose a Minecraft version/ }),
      ).toHaveValue("1.21"),
    );
    await userEvent.click(
      screen.getAllByRole("button", { name: /Download/ })[0],
    );
    expect(installMod).toHaveBeenCalledWith({
      id: "sodium",
      minecraftVersion: "1.21",
      loader: ModLoader.Fabric,
      source: ModSource.Modrinth,
      modType: ModType.Mod,
      modpack: "Pack",
      fileId: undefined,
      contentLocation: undefined,
    });
  });

  it("stays installable when the version list cannot load", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    getVersions.mockRejectedValue(new Error("offline"));
    renderPage({ mod: mod({}) });
    await waitFor(() =>
      expect(
        screen.getAllByRole("button", { name: /Download/ })[0],
      ).toBeEnabled(),
    );
  });

  it("hides version and loader pickers when a file is preselected", async () => {
    renderPage({ mod: mod({}), fileId: "abc" });
    await screen.findByText("jellysquid");
    expect(
      screen.queryByRole("combobox", { name: /Choose a Minecraft version/ }),
    ).toBeNull();
    expect(
      screen.queryByRole("combobox", { name: /choosePreferredAPI/ }),
    ).toBeNull();
    expect(
      screen.getByRole("combobox", { name: /Choose a modpack/ }),
    ).toBeTruthy();
  });

  it("picking a modpack adopts its version and loader", async () => {
    renderPage({ mod: mod({}) });
    const picker = await screen.findByRole("combobox", {
      name: /Choose a modpack/,
    });
    await userEvent.selectOptions(picker, "Pack");
    await waitFor(() =>
      expect(storeSet).toHaveBeenCalledWith("lastUsedVersion", "1.20.1"),
    );
    expect(
      screen.getByRole("combobox", { name: /Choose a Minecraft version/ }),
    ).toHaveValue("1.20.1");
  });

  it("goes back on cancel", async () => {
    renderPage({ mod: mod({}) });
    await userEvent.click(screen.getByRole("button", { name: /Cancel/ }));
    expect(back).toHaveBeenCalled();
  });

  it("falls back to the target pack when the saved version is gone", async () => {
    // The remembered version is no longer offered by the provider.
    storeGet.mockImplementation(async (key: string) =>
      key === "lastUsedVersion"
        ? "1.16.5"
        : key === "lastUsedModpack"
          ? "Pack"
          : undefined,
    );
    renderPage({ mod: mod({}) });

    const picker = await screen.findByRole("combobox", {
      name: /Choose a Minecraft version/,
    });
    await waitFor(() => expect(picker).toHaveValue("1.20.1"));
    await userEvent.click(
      screen.getAllByRole("button", { name: /Download/ })[0],
    );
    expect(installMod).toHaveBeenCalledWith({
      id: "sodium",
      minecraftVersion: "1.20.1",
      loader: ModLoader.Fabric,
      source: ModSource.Modrinth,
      modType: ModType.Mod,
      modpack: "Pack",
      fileId: undefined,
      contentLocation: undefined,
    });
  });

  it("replaces a saved pack that no longer exists with an available one", async () => {
    getModpacks.mockResolvedValue([
      {
        name: "Applied Pack",
        version: "1.20.1",
        modLoader: ModLoader.Fabric,
        isApplied: true,
        mods: [],
      },
    ]);
    storeGet.mockImplementation(async (key: string) =>
      key === "lastUsedModpack" ? "Deleted Pack" : undefined,
    );
    renderPage({ mod: mod({}) });

    const picker = await screen.findByRole("combobox", {
      name: /Choose a modpack/,
    });
    await waitFor(() => expect(picker).toHaveValue("Applied Pack"));
    await userEvent.click(
      screen.getAllByRole("button", { name: /Download/ })[0],
    );
    expect(installMod).toHaveBeenCalledWith({
      id: "sodium",
      minecraftVersion: "1.20.1",
      loader: ModLoader.Fabric,
      source: ModSource.Modrinth,
      modType: ModType.Mod,
      modpack: "Applied Pack",
      fileId: undefined,
      contentLocation: undefined,
    });
  });

  it("blocks downloads until the install choices have loaded", async () => {
    let releaseModpacks!: (packs: unknown[]) => void;
    getModpacks.mockReturnValue(
      new Promise((resolve) => {
        releaseModpacks = resolve;
      }),
    );
    renderPage({ mod: mod({}) });

    const download = screen.getAllByRole("button", { name: /Download/ })[0];
    expect(download).toBeDisabled();
    await userEvent.click(download);
    expect(installMod).not.toHaveBeenCalled();

    releaseModpacks([
      {
        name: "Pack",
        version: "1.20.1",
        modLoader: ModLoader.Fabric,
        mods: [],
      },
    ]);
    await waitFor(() => expect(download).not.toBeDisabled());
  });

  it("keeps the download disabled when a mod has no modpack to install into", async () => {
    getModpacks.mockResolvedValue([]);
    storeGet.mockResolvedValue(undefined);
    renderPage({ mod: mod({}) });

    await screen.findByText("jellysquid");
    const download = screen.getAllByRole("button", { name: /Download/ })[0];
    expect(download).toBeDisabled();
    await userEvent.click(download);
    expect(installMod).not.toHaveBeenCalled();
  });

  it("targets the opener's modpack over the last used one", async () => {
    getModpacks.mockResolvedValue([
      { name: "Pack", version: "1.21", modLoader: ModLoader.Fabric, mods: [] },
      { name: "Other", version: "1.20.1", modLoader: ModLoader.Forge, mods: [] },
    ]);
    getVersions.mockResolvedValue([
      { version: "1.21", versionType: "release" },
      { version: "1.20.1", versionType: "release" },
    ]);
    renderPage({ mod: mod({}), installTarget: { name: "Other" } });

    await waitFor(() =>
      expect(
        screen.getByRole("combobox", { name: /Choose a modpack/ }),
      ).toHaveValue("Other"),
    );
    expect(
      screen.getByRole("combobox", { name: /Choose a Minecraft version/ }),
    ).toHaveValue("1.20.1");
  });

  it("routes a resource pack to the opener's modpack", async () => {
    renderPage({
      mod: mod({ modType: ModType.ResourcePack }),
      installTarget: { name: "Pack" },
    });

    await screen.findByText("jellysquid");
    await userEvent.click(
      screen.getAllByRole("button", { name: /Download/ })[0],
    );

    expect(installMod).toHaveBeenCalledWith({
      id: "sodium",
      minecraftVersion: "1.20.1",
      loader: ModLoader.Fabric,
      source: ModSource.Modrinth,
      modType: ModType.ResourcePack,
      modpack: "Pack",
      fileId: undefined,
      contentLocation: undefined,
    });
  });

  it("sends no modpack for a resource pack opened without a target", async () => {
    // The saved lastUsedModpack must never route a resource pack; the backend
    // decides where an untargeted one lands.
    renderPage({ mod: mod({ modType: ModType.ResourcePack }) });

    await screen.findByText("jellysquid");
    await userEvent.click(
      screen.getAllByRole("button", { name: /Download/ })[0],
    );

    expect(installMod).toHaveBeenCalledWith({
      id: "sodium",
      minecraftVersion: "1.21",
      loader: ModLoader.Fabric,
      source: ModSource.Modrinth,
      modType: ModType.ResourcePack,
      modpack: "",
      fileId: undefined,
      contentLocation: undefined,
    });
  });

  describe("install location", () => {
    // The picker asks for the names alone, so every section comes back empty.
    const sections = [
      { modType: ModType.ResourcePack, files: [] },
      { modType: ModType.ShaderPack, files: [] },
    ];
    const folders = [
      {
        id: "minecraft",
        kind: "minecraft",
        name: "",
        path: "/home/me/.minecraft",
        sections,
      },
      {
        id: "prism:1",
        kind: "prism",
        name: "Survival",
        path: "/home/me/prism/Survival/.minecraft",
        sections,
      },
    ];

    it("never offers a mod a folder to install into", async () => {
      getInstalledContent.mockResolvedValue(folders);
      renderPage({ mod: mod({}), installTarget: { name: "Pack" } });

      await screen.findByText("jellysquid");
      expect(
        screen.queryByRole("combobox", { name: /Install to/ }),
      ).toBeNull();
      expect(getInstalledContent).not.toHaveBeenCalled();
    });

    it("offers nothing when the Minecraft folder is the only place", async () => {
      getInstalledContent.mockResolvedValue([folders[0]]);
      renderPage({
        mod: mod({ modType: ModType.ResourcePack }),
        installTarget: { name: "Pack" },
      });

      await screen.findByText("jellysquid");
      // The picker only needs the names, so the pack lists are left out.
      await waitFor(() =>
        expect(getInstalledContent).toHaveBeenCalledWith(false),
      );
      expect(
        screen.queryByRole("combobox", { name: /Install to/ }),
      ).toBeNull();
      await userEvent.click(
        screen.getAllByRole("button", { name: /Download/ })[0],
      );
      expect(installMod).toHaveBeenCalledWith({
        id: "sodium",
        minecraftVersion: "1.20.1",
        loader: ModLoader.Fabric,
        source: ModSource.Modrinth,
        modType: ModType.ResourcePack,
        modpack: "Pack",
        fileId: undefined,
        contentLocation: undefined,
      });
    });

    it("follows the target pack until the user picks a folder", async () => {
      getInstalledContent.mockResolvedValue(folders);
      renderPage({
        mod: mod({ modType: ModType.ShaderPack }),
        installTarget: { name: "Pack" },
      });

      const picker = await screen.findByRole("combobox", {
        name: /Install to/,
      });
      expect(picker).toHaveValue("");
      expect(
        screen.getByRole("option", { name: "Automatic (where Pack is applied)" }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("option", { name: "Survival (Prism Launcher)" }),
      ).toBeInTheDocument();

      await userEvent.selectOptions(picker, "prism:1");
      await userEvent.click(
        screen.getAllByRole("button", { name: /Download/ })[0],
      );
      expect(installMod).toHaveBeenCalledWith({
        id: "sodium",
        minecraftVersion: "1.20.1",
        loader: ModLoader.Fabric,
        source: ModSource.Modrinth,
        modType: ModType.ShaderPack,
        modpack: "Pack",
        fileId: undefined,
        contentLocation: "prism:1",
      });
    });

    it("defaults to the Minecraft folder when there is no target pack", async () => {
      getInstalledContent.mockResolvedValue(folders);
      renderPage({ mod: mod({ modType: ModType.ResourcePack }) });

      const picker = await screen.findByRole("combobox", {
        name: /Install to/,
      });
      expect(picker).toHaveValue("minecraft");
      expect(screen.queryByRole("option", { name: /Automatic/ })).toBeNull();

      await userEvent.click(
        screen.getAllByRole("button", { name: /Download/ })[0],
      );
      expect(installMod).toHaveBeenCalledWith({
        id: "sodium",
        minecraftVersion: "1.21",
        loader: ModLoader.Fabric,
        source: ModSource.Modrinth,
        modType: ModType.ResourcePack,
        modpack: "",
        fileId: undefined,
        contentLocation: "minecraft",
      });
    });

    it("installs as usual when the folders cannot be listed", async () => {
      getInstalledContent.mockRejectedValue(new Error("boom"));
      renderPage({ mod: mod({ modType: ModType.ResourcePack }) });

      await screen.findByText("jellysquid");
      expect(
        screen.queryByRole("combobox", { name: /Install to/ }),
      ).toBeNull();
      await userEvent.click(
        screen.getAllByRole("button", { name: /Download/ })[0],
      );
      expect(installMod).toHaveBeenCalledWith({
        id: "sodium",
        minecraftVersion: "1.21",
        loader: ModLoader.Fabric,
        source: ModSource.Modrinth,
        modType: ModType.ResourcePack,
        modpack: "",
        fileId: undefined,
        contentLocation: undefined,
      });
    });

    it("starts on the folder the opener already chose", async () => {
      getInstalledContent.mockResolvedValue(folders);
      renderPage({
        mod: mod({ modType: ModType.ResourcePack }),
        installLocation: "prism:1",
      });

      const picker = await screen.findByRole("combobox", {
        name: /Install to/,
      });
      expect(picker).toHaveValue("prism:1");

      await userEvent.click(
        screen.getAllByRole("button", { name: /Download/ })[0],
      );
      expect(installMod).toHaveBeenCalledWith({
        id: "sodium",
        minecraftVersion: "1.21",
        loader: ModLoader.Fabric,
        source: ModSource.Modrinth,
        modType: ModType.ResourcePack,
        modpack: "",
        fileId: undefined,
        contentLocation: "prism:1",
      });
    });

    it("ignores a chosen folder this host no longer lists", async () => {
      getInstalledContent.mockResolvedValue(folders);
      renderPage({
        mod: mod({ modType: ModType.ResourcePack }),
        installLocation: "prism:9",
      });

      const picker = await screen.findByRole("combobox", {
        name: /Install to/,
      });
      // Falls back to the default rather than submitting a folder that is gone.
      expect(picker).toHaveValue("minecraft");
    });
  });

  it("warns and offers a reinstall when the pack already has the mod", async () => {
    getModpacks.mockResolvedValue([
      {
        name: "Pack",
        version: "1.20.1",
        modLoader: ModLoader.Fabric,
        mods: [
          {
            id: "sodium",
            downloadUrl: "https://example.com/sodium.jar",
            source: ModSource.Modrinth,
          },
        ],
      },
    ]);
    renderPage({ mod: mod({}) });

    expect(await screen.findByText(/Already installed in/)).toBeTruthy();
    expect(screen.getByRole("button", { name: /Reinstall/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  });

  it("says whose copy is removed when the pack has the mod from the other provider", async () => {
    getModpacks.mockResolvedValue([
      {
        name: "Pack",
        version: "1.20.1",
        modLoader: ModLoader.Fabric,
        mods: [
          {
            id: "394468",
            downloadUrl: "https://example.com/sodium.jar",
            source: ModSource.CurseForge,
            slug: "sodium",
          },
        ],
      },
    ]);
    renderPage({ mod: mod({ slug: "sodium" }) });

    expect(
      await screen.findByText(
        "Pack already has this mod from CurseForge. Installing it here removes that copy.",
      ),
    ).toBeTruthy();
  });

  it("marks a dependency the pack already has", async () => {
    getModpacks.mockResolvedValue([
      {
        name: "Pack",
        version: "1.20.1",
        modLoader: ModLoader.Fabric,
        mods: [
          {
            id: "fabric-api",
            downloadUrl: "https://example.com/fabric-api.jar",
            source: ModSource.Modrinth,
          },
        ],
      },
    ]);
    getModDependencies.mockResolvedValue([
      mod({ id: "fabric-api", name: "Fabric API" }),
      mod({ id: "cloth", name: "Cloth Config" }),
    ]);
    renderPage({ mod: mod({}) });

    await waitFor(() =>
      expect(screen.getByText("Fabric API")).toHaveAttribute(
        "data-installed",
        "true",
      ),
    );
    expect(screen.getByText("Cloth Config")).toHaveAttribute(
      "data-installed",
      "false",
    );
  });

  it("ignores dependencies from a previously viewed mod", async () => {
    let releaseFirst!: (deps: IMod[]) => void;
    getModDependencies
      .mockReturnValueOnce(
        new Promise((resolve) => {
          releaseFirst = resolve;
        }),
      )
      .mockResolvedValue([mod({ id: "sodium-extra", name: "Sodium Extra" })]);

    const { rerender } = renderPage({ mod: mod({}) });
    rerender(
      <ContentContext.Provider
        value={{
          back,
          setSnackbar,
          changeContent: () => {},
          changePage: () => {},
          setSnackbarNoState: () => {},
        }}
      >
        <ModInstallPage mod={mod({ id: "iris", name: "Iris" })} />
      </ContentContext.Provider>,
    );
    expect(await screen.findByTestId("mod-card")).toHaveTextContent(
      "Sodium Extra",
    );

    // The first mod's response arriving late must not replace the current one.
    releaseFirst([mod({ id: "fabric-api", name: "Fabric API" })]);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.queryByText("Fabric API")).toBeNull();
    expect(screen.getByTestId("mod-card")).toHaveTextContent("Sodium Extra");
  });
});
