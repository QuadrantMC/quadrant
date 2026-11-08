/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  ContentContext,
  IMod,
  ModLoader,
  ModSource,
  ModType,
} from "../../../intefaces";

const getMod = vi.fn();
const identifyUnknownMods = vi.fn();
const getModUpdate = vi.fn();

vi.mock("../../../tools", () => ({
  getMod: (...a: unknown[]) => getMod(...a),
  getModUpdate: (...a: unknown[]) => getModUpdate(...a),
  identifyUnknownMods: (...a: unknown[]) => identifyUnknownMods(...a),
}));

// The Mod card pulls in the desktop store and install flow; a marker is enough
// to prove which mods were resolved.
vi.mock("../Mod", () => ({
  default: ({ mod }: { mod: IMod }) => <div data-testid="mod">{mod.name}</div>,
}));

import ModpackView from "./ModpackView";

function fetched(id: string): IMod {
  return {
    name: id,
    id,
    downloadCount: 0,
    version: "1.0",
    dateModified: "",
    modType: ModType.Mod,
    source: ModSource.Modrinth,
    slug: id,
    thumbnailUrls: [],
    url: "",
    description: "",
    license: "",
    modIconUrl: "",
    downloadable: false,
    showPreviousVersion: false,
    newVersion: null,
    deleteable: true,
    autoinstallable: false,
    selectable: false,
    modpack: "Pack",
    selectUrl: null,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  getMod.mockImplementation(async (args: { id: string }) => fetched(args.id));
  identifyUnknownMods.mockResolvedValue([]);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("ModpackView", () => {
  it("resolves each installed mod exactly once on mount", async () => {
    render(
      <ModpackView
        name="Pack"
        version="1.20.1"
        modLoader={ModLoader.Fabric}
        isApplied={false}
        lastSynced={0}
        unknownMods={false}
        mods={[
          { id: "sodium", downloadUrl: "", source: ModSource.Modrinth },
          { id: "lithium", downloadUrl: "", source: ModSource.Modrinth },
        ]}
      />,
    );

    await waitFor(() => expect(getMod).toHaveBeenCalledTimes(2));
    // Let any duplicate mount effect settle before asserting the final count.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(getMod).toHaveBeenCalledTimes(2);
    expect(getMod.mock.calls.map((call) => call[0].id).sort()).toEqual([
      "lithium",
      "sodium",
    ]);
  });
  it("caps concurrent provider requests and does not refetch when Identify is toggled", async () => {
    let running = 0;
    let peak = 0;
    getMod.mockImplementation(async (args: { id: string }) => {
      running++;
      peak = Math.max(peak, running);
      await new Promise((resolve) => setTimeout(resolve, 5));
      running--;
      return fetched(args.id);
    });
    const mods = Array.from({ length: 20 }, (_, i) => ({
      id: `mod${i}`,
      downloadUrl: "",
      source: ModSource.Modrinth,
    }));
    render(
      <ModpackView
        name="Pack"
        version="1.20.1"
        modLoader={ModLoader.Fabric}
        isApplied={false}
        lastSynced={0}
        unknownMods={true}
        mods={mods}
      />,
    );
    await waitFor(() => expect(screen.getAllByTestId("mod")).toHaveLength(20));
    expect(peak).toBeLessThanOrEqual(6);
    expect(getMod).toHaveBeenCalledTimes(20);

    await userEvent.click(screen.getByRole("button", { name: /identify/i }));
    await userEvent.click(screen.getByRole("button", { name: /identify/i }));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(getMod).toHaveBeenCalledTimes(20);
    expect(screen.getAllByTestId("mod")).toHaveLength(20);
  });

  it("lets the user hide the update view while the checks still run", async () => {
    let finishCheck!: () => void;
    getModUpdate.mockReturnValue(
      new Promise((resolve) => {
        finishCheck = () => resolve(null);
      }),
    );
    render(
      <ModpackView
        name="Pack"
        version="1.20.1"
        modLoader={ModLoader.Fabric}
        isApplied={false}
        lastSynced={0}
        unknownMods={false}
        mods={[{ id: "sodium", downloadUrl: "", source: ModSource.Modrinth }]}
      />,
    );
    await screen.findByTestId("mod");

    const button = screen.getByRole("button", { name: /update/i });
    await userEvent.click(button);
    await waitFor(() => expect(getModUpdate).toHaveBeenCalled());

    expect(button).toBeEnabled();
    finishCheck();
  });

  it("offers a back button that uses the live content context", async () => {
    const staleBack = vi.fn();
    const back = vi.fn();
    const pack = {
      name: "Pack",
      version: "1.20.1",
      modLoader: ModLoader.Fabric,
      isApplied: false,
      lastSynced: 0,
      unknownMods: false,
      mods: [],
    };
    const withContext = (view: React.ReactNode, goBack = back) => (
      <ContentContext.Provider
        value={{
          back: goBack,
          changeContent: vi.fn(),
          setSnackbar: vi.fn(),
          setSnackbarNoState: vi.fn(),
        }}
      >
        {view}
      </ContentContext.Provider>
    );
    const { rerender } = render(
      withContext(<ModpackView {...pack} />, staleBack),
    );
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();

    rerender(withContext(<ModpackView {...pack} showBack />));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(back).toHaveBeenCalledTimes(1);
    expect(staleBack).not.toHaveBeenCalled();
  });

  describe("when the pack it shows changes", () => {
    const base = {
      version: "1.20.1",
      modLoader: ModLoader.Fabric,
      isApplied: false,
      lastSynced: 0,
      unknownMods: false,
    };
    const mod = (id: string) => ({
      id,
      downloadUrl: "",
      source: ModSource.Modrinth,
    });

    it("loads the cards of the new pack, not the old one", async () => {
      const { rerender } = render(
        <ModpackView {...base} name="Old" mods={[mod("sodium")]} />,
      );
      expect(await screen.findByText("sodium")).toBeInTheDocument();

      rerender(<ModpackView {...base} name="New" mods={[mod("iris")]} />);

      expect(await screen.findByText("iris")).toBeInTheDocument();
      await waitFor(() =>
        expect(screen.queryByText("sodium")).not.toBeInTheDocument(),
      );
      expect(getMod.mock.calls[getMod.mock.calls.length - 1][0]).toMatchObject({
        id: "iris",
        modpack: "New",
      });
    });

    it("refetches when a mod leaves the pack but not when the list is re-read unchanged", async () => {
      const { rerender } = render(
        <ModpackView
          {...base}
          name="Pack"
          mods={[mod("sodium"), mod("iris")]}
        />,
      );
      await waitFor(() => expect(getMod).toHaveBeenCalledTimes(2));

      rerender(
        <ModpackView
          {...base}
          name="Pack"
          mods={[mod("sodium"), mod("iris")]}
        />,
      );
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 20));
      });
      expect(getMod).toHaveBeenCalledTimes(2);

      rerender(<ModpackView {...base} name="Pack" mods={[mod("sodium")]} />);
      await waitFor(() => expect(getMod).toHaveBeenCalledTimes(3));
      await waitFor(() =>
        expect(screen.queryByText("iris")).not.toBeInTheDocument(),
      );
    });

    it("ignores a slow response for the pack it was showing before", async () => {
      let resolveOld!: (value: IMod) => void;
      getMod.mockImplementationOnce(
        () =>
          new Promise<IMod>((resolve) => {
            resolveOld = resolve;
          }),
      );
      const { rerender } = render(
        <ModpackView {...base} name="Old" mods={[mod("sodium")]} />,
      );
      await waitFor(() => expect(getMod).toHaveBeenCalledTimes(1));

      rerender(<ModpackView {...base} name="New" mods={[mod("iris")]} />);
      expect(await screen.findByText("iris")).toBeInTheDocument();
      await act(async () => resolveOld(fetched("sodium")));

      expect(screen.queryByText("sodium")).not.toBeInTheDocument();
    });
  });
});
