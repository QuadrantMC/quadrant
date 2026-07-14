/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ModSource, ModType, type IMod } from "../../../intefaces";

const getVersions = vi.fn();
const getModpacks = vi.fn();
const getCategories = vi.fn();
const searchMods = vi.fn();

const storeGet = vi.fn();
const storeSet = vi.fn();
const storeSave = vi.fn();

vi.mock("../../../tools", () => ({
  getVersions: (...a: unknown[]) => getVersions(...a),
  getModpacks: (...a: unknown[]) => getModpacks(...a),
  getCategories: (...a: unknown[]) => getCategories(...a),
  searchMods: (...a: unknown[]) => searchMods(...a),
}));

vi.mock("../../../desktop", () => ({
  createDesktopStore: () => ({
    get: (...a: unknown[]) => storeGet(...a),
    set: (...a: unknown[]) => storeSet(...a),
    save: (...a: unknown[]) => storeSave(...a),
  }),
}));

// The Mod card is heavy and independently tested; stub it to render just the
// mod name so result ordering/identity is observable.
vi.mock("../../shared/Mod", () => ({
  default: ({ mod }: { mod: IMod }) => (
    <div data-testid="mod-card">{mod.name}</div>
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

  it("submitting the form searches with the typed query", async () => {
    render(<SearchPage />);
    await waitFor(() => expect(searchMods).toHaveBeenCalled());
    searchMods.mockClear();

    await userEvent.type(screen.getByRole("textbox"), "sodium");
    await userEvent.keyboard("{Enter}");

    await waitFor(() =>
      expect(searchMods).toHaveBeenCalledWith(
        expect.objectContaining({ query: "sodium", source: ModSource.Modrinth }),
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
    await waitFor(() => expect(screen.getByRole("textbox")).toBeInTheDocument());

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
});
