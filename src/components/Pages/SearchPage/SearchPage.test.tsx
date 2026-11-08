/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  ModLoader,
  ModSource,
  ModType,
  type ContentLocation,
  type IMod,
  type LocalModpack,
} from "../../../intefaces";

const getVersions = vi.fn();
const getModpacks = vi.fn();
const getCategories = vi.fn();
const searchMods = vi.fn();
const getInstalledContent = vi.fn();

const storeGet = vi.fn();
const storeSet = vi.fn();
const storeSave = vi.fn();

vi.mock("../../../tools", () => ({
  getVersions: (...a: unknown[]) => getVersions(...a),
  getModpacks: (...a: unknown[]) => getModpacks(...a),
  getCategories: (...a: unknown[]) => getCategories(...a),
  searchMods: (...a: unknown[]) => searchMods(...a),
  getInstalledContent: (...a: unknown[]) => getInstalledContent(...a),
}));

vi.mock("../../../desktop", () => ({
  createDesktopStore: () => ({
    get: (...a: unknown[]) => storeGet(...a),
    set: (...a: unknown[]) => storeSet(...a),
    save: (...a: unknown[]) => storeSave(...a),
  }),
}));

// The Mod card is heavy and independently tested; stub it to render just the
// mod name so result ordering/identity is observable. Clicking it stands in
// for a finished install.
vi.mock("../../shared/Mod", () => ({
  default: ({
    mod,
    installed,
    installLocation,
    onInstalled,
  }: {
    mod: IMod;
    installed?: boolean;
    installLocation?: string;
    onInstalled?: () => void;
  }) => (
    <div
      data-testid="mod-card"
      data-installed={String(!!installed)}
      data-install-location={installLocation ?? ""}
      onClick={() => onInstalled?.()}
    >
      {mod.name}
    </div>
  ),
}));

import SearchPage from "./SearchPage";

function mod(over: Partial<IMod>): IMod {
  return {
    name: "Mod",
    id: "0",
    downloadCount: 0,
    version: "",
    dateModified: "",
    modType: ModType.Mod,
    source: ModSource.Modrinth,
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
  };
}

/** A promise whose resolution is controlled by the test. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.clearAllMocks();
  // jsdom does not implement Element.scrollTo; the results pane scrolls on
  // page change.
  Element.prototype.scrollTo = vi.fn();
  getVersions.mockResolvedValue([
    { version: "1.20.1", versionType: "release" },
  ]);
  getModpacks.mockResolvedValue([]);
  getCategories.mockResolvedValue([]);
  searchMods.mockResolvedValue([]);
  getInstalledContent.mockResolvedValue([]);
  // Only Modrinth enabled → one provider request per search, keeping the
  // race scenario unambiguous.
  storeGet.mockImplementation(async (key: string) => {
    if (key === "curseforge") return false;
    if (key === "modrinth") return true;
    return undefined;
  });
  storeSet.mockResolvedValue(undefined);
  storeSave.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("SearchPage", () => {
  it("boots and runs an initial search against the enabled provider", async () => {
    render(<SearchPage />);

    // The single text field is the query input.
    await waitFor(() =>
      expect(screen.getByRole("textbox")).toBeInTheDocument(),
    );
    // The mount effect auto-searches; with only Modrinth enabled that provider
    // must be queried.
    await waitFor(() => expect(searchMods).toHaveBeenCalled());
    expect(searchMods).toHaveBeenCalledWith(
      expect.objectContaining({ source: ModSource.Modrinth }),
    );
  });

  it("restores the saved filters when the version list cannot load", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    getVersions.mockRejectedValue(new Error("offline"));
    getModpacks.mockResolvedValue([{ name: "Local Pack", mods: [] }]);
    const saved = {
      contentType: ModType.ResourcePack,
      targetModpack: "Local Pack",
    };
    storeGet.mockImplementation(async (key: string) =>
      key === "searchFilters"
        ? saved
        : key === "modrinth"
          ? true
          : key === "curseforge"
            ? false
            : undefined,
    );
    render(<SearchPage />);

    // Persistence only switches on once hydration has finished, so a write of
    // the restored filters proves the boot did not die on the version request.
    await waitFor(() =>
      expect(storeSet).toHaveBeenCalledWith(
        "searchFilters",
        expect.objectContaining(saved),
      ),
    );
  });

  it("submitting the form searches with the typed query", async () => {
    render(<SearchPage />);
    await waitFor(() => expect(searchMods).toHaveBeenCalled());
    searchMods.mockClear();

    await userEvent.type(screen.getByRole("textbox"), "sodium");
    await userEvent.keyboard("{Enter}");

    await waitFor(() =>
      expect(searchMods).toHaveBeenCalledWith(
        expect.objectContaining({
          query: "sodium",
          source: ModSource.Modrinth,
        }),
      ),
    );
  });

  it("a stale in-flight response never overwrites a newer one", async () => {
    // Controlled promises for the two racing requests, keyed by query so the
    // empty-query mount searches don't interfere.
    const pending: { resolve: (v: IMod[]) => void }[] = [];
    searchMods.mockImplementation((args: { query: string }) => {
      if (args.query === "race") {
        const d = deferred<IMod[]>();
        pending.push({ resolve: d.resolve });
        return d.promise;
      }
      return Promise.resolve<IMod[]>([]);
    });

    render(<SearchPage />);
    await waitFor(() =>
      expect(screen.getByRole("textbox")).toBeInTheDocument(),
    );

    const input = screen.getByRole("textbox");
    await userEvent.type(input, "race");

    // Request A (stale).
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(pending).toHaveLength(1));

    // Request B (newer) — supersedes A.
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(pending).toHaveLength(2));

    // Newer request resolves first and should win.
    pending[1].resolve([mod({ name: "Newer result", id: "new" })]);
    await waitFor(() =>
      expect(screen.getByText("Newer result")).toBeInTheDocument(),
    );

    // Stale request resolves LAST; the race guard must discard it.
    pending[0].resolve([mod({ name: "Stale result", id: "old" })]);

    // Give any (incorrect) state update a chance to flush, then assert the
    // stale payload never replaced the newer results.
    await Promise.resolve();
    await waitFor(() =>
      expect(screen.getByText("Newer result")).toBeInTheDocument(),
    );
    expect(screen.queryByText("Stale result")).not.toBeInTheDocument();
  });

  it("appends results for the submitted query when the search input has an unsubmitted edit", async () => {
    searchMods.mockResolvedValue([mod({ name: "Initial result" })]);
    render(<SearchPage />);
    await screen.findByText("Initial result");
    await userEvent.type(screen.getByRole("textbox"), "unsubmitted");
    searchMods.mockClear();
    await userEvent.click(
      screen.getByRole("button", { name: "Search further" }),
    );
    await waitFor(() =>
      expect(searchMods).toHaveBeenCalledWith(
        expect.objectContaining({ query: "", offset: 1 }),
      ),
    );
  });

  it("keeps pagination usable when a new search supersedes an in-flight append", async () => {
    const pending = deferred<IMod[]>();
    searchMods.mockImplementation((args: { query: string; offset: number }) => {
      if (args.offset > 0) return pending.promise;
      return Promise.resolve([
        mod({ name: args.query === "new" ? "New result" : "Initial result" }),
      ]);
    });
    render(<SearchPage />);
    await screen.findByText("Initial result");
    await userEvent.click(
      screen.getByRole("button", { name: "Search further" }),
    );
    await waitFor(() =>
      expect(searchMods).toHaveBeenCalledWith(
        expect.objectContaining({ offset: 1 }),
      ),
    );
    await userEvent.type(screen.getByRole("textbox"), "new{Enter}");
    await screen.findByText("New result");
    expect(screen.getByRole("button", { name: "Search further" })).toBeEnabled();
    await act(async () =>
      pending.resolve([mod({ name: "Stale appended result" })]),
    );
    expect(screen.queryByText("Stale appended result")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Search further" })).toBeEnabled();
  });

  it("does not mark results auto-installable when their saved target no longer exists", async () => {
    storeGet.mockImplementation(async (key: string) => {
      if (key === "curseforge") return false;
      if (key === "modrinth") return true;
      if (key === "searchFilters")
        return { targetModpack: "Deleted", version: "1.20.1" };
      return undefined;
    });
    render(<SearchPage />);
    await waitFor(() =>
      expect(searchMods).toHaveBeenCalledWith(
        expect.objectContaining({ gameVersion: "1.20.1" }),
      ),
    );
    const restoredRequests = searchMods.mock.calls.filter(
      ([args]) => args.gameVersion === "1.20.1",
    );
    expect(restoredRequests.every(([args]) => args.filterOn === false)).toBe(
      true,
    );
  });

  const alpha = (modIds: string[]): LocalModpack => ({
    name: "alpha",
    version: "1.20.1",
    modLoader: ModLoader.Fabric,
    isApplied: false,
    lastSynced: 0,
    unknownMods: false,
    mods: modIds.map((id) => ({
      id,
      downloadUrl: `https://example.com/${id}.jar`,
      source: ModSource.Modrinth,
      slug: id,
    })),
  });

  const targetAlpha = () =>
    storeGet.mockImplementation(async (key: string) => {
      if (key === "curseforge") return false;
      if (key === "modrinth") return true;
      if (key === "searchFilters")
        return { targetModpack: "alpha", version: "1.20.1" };
      return undefined;
    });

  it("flips a card to installed once its install reports back", async () => {
    getModpacks.mockResolvedValue([alpha([])]);
    targetAlpha();
    searchMods.mockResolvedValue([
      mod({ name: "Iris", id: "iris", slug: "iris" }),
    ]);

    render(<SearchPage />);
    const card = await screen.findByText("Iris");
    expect(card).toHaveAttribute("data-installed", "false");

    getModpacks.mockResolvedValue([alpha(["iris"])]);
    await userEvent.click(card);

    await waitFor(() =>
      expect(screen.getByText("Iris")).toHaveAttribute(
        "data-installed",
        "true",
      ),
    );
  });

  it("keeps the newest reload when two installs answer out of order", async () => {
    getModpacks.mockResolvedValue([alpha([])]);
    targetAlpha();
    searchMods.mockResolvedValue([
      mod({ name: "Iris", id: "iris", slug: "iris" }),
      mod({ name: "Sodium", id: "sodium", slug: "sodium" }),
    ]);

    render(<SearchPage />);
    const iris = await screen.findByText("Iris");

    const afterFirst = deferred<LocalModpack[]>();
    const afterSecond = deferred<LocalModpack[]>();
    getModpacks
      .mockReturnValueOnce(afterFirst.promise)
      .mockReturnValueOnce(afterSecond.promise);
    await userEvent.click(iris);
    await userEvent.click(screen.getByText("Sodium"));

    afterSecond.resolve([alpha(["iris", "sodium"])]);
    await waitFor(() =>
      expect(screen.getByText("Sodium")).toHaveAttribute(
        "data-installed",
        "true",
      ),
    );

    afterFirst.resolve([alpha(["iris"])]);
    await act(async () => {
      await afterFirst.promise;
    });
    expect(screen.getByText("Sodium")).toHaveAttribute(
      "data-installed",
      "true",
    );
  });

  it("marks only the results already present in the target modpack", async () => {
    getModpacks.mockResolvedValue([alpha(["sodium"])]);
    targetAlpha();
    searchMods.mockResolvedValue([
      mod({ name: "Sodium", id: "sodium", slug: "sodium" }),
      mod({ name: "Iris", id: "iris", slug: "iris" }),
    ]);

    render(<SearchPage />);

    await waitFor(() =>
      expect(screen.getByText("Sodium")).toHaveAttribute(
        "data-installed",
        "true",
      ),
    );
    expect(screen.getByText("Iris")).toHaveAttribute("data-installed", "false");
  });

  it("marks nothing installed while no modpack is targeted", async () => {
    searchMods.mockResolvedValue([
      mod({ name: "Sodium", id: "sodium", slug: "sodium" }),
    ]);

    render(<SearchPage />);

    expect(await screen.findByText("Sodium")).toHaveAttribute(
      "data-installed",
      "false",
    );
  });

  describe("install location", () => {
    const folder = (over: Partial<ContentLocation>): ContentLocation => ({
      id: "minecraft",
      kind: "minecraft",
      name: "",
      path: "/home/me/.minecraft",
      sections: [],
      ...over,
    });
    const folders = [
      folder({}),
      folder({ id: "prism:1", kind: "prism", name: "Survival" }),
    ];

    /** The content-type pills are the only way to leave the mod type. */
    const chooseType = async (label: string) =>
      userEvent.click(screen.getByRole("button", { name: label }));

    const picker = () =>
      screen.queryByRole("combobox", { name: "Install to" });

    it("never offers a mod a folder to install into", async () => {
      getInstalledContent.mockResolvedValue(folders);
      render(<SearchPage />);
      await waitFor(() => expect(searchMods).toHaveBeenCalled());

      expect(picker()).toBeNull();
      expect(getInstalledContent).not.toHaveBeenCalled();
    });

    it("offers nothing when the Minecraft folder is the only place", async () => {
      getInstalledContent.mockResolvedValue([folders[0]]);
      render(<SearchPage />);
      await waitFor(() => expect(searchMods).toHaveBeenCalled());

      await chooseType("Shaders");
      // The picker only needs the names, so the pack lists are left out.
      await waitFor(() => expect(getInstalledContent).toHaveBeenCalledWith(false));
      expect(picker()).toBeNull();
    });

    it("offers every folder once a pack type is being searched", async () => {
      getInstalledContent.mockResolvedValue(folders);
      searchMods.mockResolvedValue([mod({ name: "Faithful", id: "faithful" })]);
      render(<SearchPage />);
      await waitFor(() => expect(searchMods).toHaveBeenCalled());

      await chooseType("Resource Packs");
      const select = await screen.findByRole("combobox", {
        name: "Install to",
      });
      expect(select).toHaveValue("");
      expect(
        screen.getByRole("option", { name: "Automatic" }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("option", { name: "Survival (Prism Launcher)" }),
      ).toBeInTheDocument();
    });

    it("hands the chosen folder to the cards that quick install", async () => {
      getInstalledContent.mockResolvedValue(folders);
      searchMods.mockResolvedValue([mod({ name: "Faithful", id: "faithful" })]);
      render(<SearchPage />);
      await waitFor(() => expect(searchMods).toHaveBeenCalled());

      await chooseType("Resource Packs");
      await userEvent.selectOptions(
        await screen.findByRole("combobox", { name: "Install to" }),
        "prism:1",
      );

      await waitFor(() =>
        expect(screen.getByText("Faithful")).toHaveAttribute(
          "data-install-location",
          "prism:1",
        ),
      );
    });

    it("forgets the folder when the content type changes", async () => {
      getInstalledContent.mockResolvedValue(folders);
      render(<SearchPage />);
      await waitFor(() => expect(searchMods).toHaveBeenCalled());

      await chooseType("Resource Packs");
      await userEvent.selectOptions(
        await screen.findByRole("combobox", { name: "Install to" }),
        "prism:1",
      );
      await chooseType("Shaders");

      await waitFor(() => expect(picker()).toHaveValue(""));
      // A choice this transient has no business outliving the session.
      expect(
        storeSet.mock.calls.filter(([key]) => key === "searchFilters"),
      ).not.toContainEqual(
        expect.arrayContaining([
          expect.objectContaining({ installLocation: expect.anything() }),
        ]),
      );
    });

    it("costs the user the picker, not the search, when listing fails", async () => {
      getInstalledContent.mockRejectedValue(new Error("boom"));
      searchMods.mockResolvedValue([mod({ name: "Faithful", id: "faithful" })]);
      render(<SearchPage />);
      await waitFor(() => expect(searchMods).toHaveBeenCalled());

      await chooseType("Resource Packs");
      await waitFor(() => expect(getInstalledContent).toHaveBeenCalled());

      expect(picker()).toBeNull();
      expect(await screen.findByText("Faithful")).toBeInTheDocument();
    });
  });

  it("translates the error code a failed search comes back with", async () => {
    searchMods.mockRejectedValue("errorNetwork");

    render(<SearchPage />);

    expect(
      await screen.findByText(
        "Couldn't reach the server. Check your internet connection and try again.",
      ),
    ).toBeInTheDocument();
  });
});
