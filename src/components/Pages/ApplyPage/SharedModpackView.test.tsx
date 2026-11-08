/** @format */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  ContentContext,
  ModLoader,
  ModSource,
  type IMod,
  type InstalledModpack,
} from "../../../intefaces";

const getMod = vi.fn();
const installModpack = vi.fn();
const getModpacks = vi.fn();
const back = vi.fn();

vi.mock("../../../tools", () => ({
  getMod: (...args: unknown[]) => getMod(...args),
  installModpack: (...args: unknown[]) => installModpack(...args),
  getModpacks: (...args: unknown[]) => getModpacks(...args),
}));
vi.mock("../../../desktop", () => ({
  invoke: vi.fn(),
  listen: async () => () => {},
}));
vi.mock("../../shared/Mod", () => ({
  default: ({ mod }: { mod: IMod }) => <div>{mod.name}</div>,
}));

import SharedModpackView from "./SharedModpackView";

const pack: InstalledModpack = {
  name: "Shared pack",
  version: "1.21",
  modLoader: ModLoader.Fabric,
  mods: ["sodium", "iris"].map((id) => ({
    id,
    downloadUrl: "",
    source: ModSource.Modrinth,
  })),
};

function deferredMod() {
  let resolve!: (mod: Partial<IMod>) => void;
  const promise = new Promise<Partial<IMod>>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.resetAllMocks();
  getModpacks.mockResolvedValue([]);
});

describe("SharedModpackView", () => {
  it("shows the pack immediately and reveals each mod while the rest load", async () => {
    const first = deferredMod();
    const second = deferredMod();
    getMod
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    render(<SharedModpackView modpack={pack} />);

    const heading = screen.getByRole("heading", { name: /Shared pack/ });
    expect(heading).toBeVisible();
    expect(screen.getByRole("status")).toBeVisible();
    const download = within(heading.closest("section")!).getByRole("button", {
      name: "Download",
    });
    expect(download).toBeVisible();
    await userEvent.click(download);
    expect(installModpack).toHaveBeenCalledWith(pack);

    await act(async () => first.resolve({ id: "sodium", name: "Sodium" }));
    expect(screen.getByText("Sodium")).toBeVisible();
    expect(screen.getByRole("status")).toBeVisible();

    await act(async () => second.resolve({ id: "iris", name: "Iris" }));
    expect(screen.getByText("Iris")).toBeVisible();
    expect(screen.queryByRole("status")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(installModpack).toHaveBeenCalledWith(pack);
  });

  it("allows cancelling while metadata is still loading", async () => {
    getMod.mockReturnValue(new Promise(() => {}));
    render(
      <ContentContext.Provider
        value={{
          back,
          changeContent: vi.fn(),
          setSnackbar: vi.fn(),
          setSnackbarNoState: vi.fn(),
        }}
      >
        <SharedModpackView modpack={pack} />
      </ContentContext.Provider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(back).toHaveBeenCalledOnce();
  });

  it("stops fetching the old pack and ignores its response after switching packs", async () => {
    const oldRequest = deferredMod();
    getMod.mockReturnValueOnce(oldRequest.promise);
    const { rerender } = render(<SharedModpackView modpack={pack} />);
    rerender(
      <SharedModpackView modpack={{ ...pack, name: "Empty pack", mods: [] }} />,
    );
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    await act(async () => oldRequest.resolve({ id: "sodium", name: "Sodium" }));
    expect(screen.queryByText("Sodium")).toBeNull();
    expect(getMod).toHaveBeenCalledOnce();
  });

  it("finishes loading and keeps successful cards when a lookup fails", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    getMod
      .mockRejectedValueOnce(new Error("Unavailable"))
      .mockResolvedValueOnce({ id: "iris", name: "Iris" });
    render(<SharedModpackView modpack={pack} />);
    expect(await screen.findByText("Iris")).toBeVisible();
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.getByRole("button", { name: "Download" })).toBeVisible();
    error.mockRestore();
  });

  describe("when a local pack already has the name", () => {
    beforeEach(() => {
      getMod.mockReturnValue(new Promise(() => {}));
      getModpacks.mockResolvedValue([
        { name: "shared PACK", version: "1.20.1", mods: [] },
      ]);
    });

    it("asks before replacing it and installs only once confirmed", async () => {
      render(<SharedModpackView modpack={pack} />);
      await userEvent.click(screen.getByRole("button", { name: "Download" }));
      expect(await screen.findByText(/already have a modpack/i)).toBeVisible();
      expect(installModpack).not.toHaveBeenCalled();

      await userEvent.click(screen.getByRole("button", { name: "Replace" }));
      await waitFor(() => expect(installModpack).toHaveBeenCalledWith(pack));
    });

    it("leaves the local pack alone when the question is cancelled", async () => {
      render(<SharedModpackView modpack={pack} />);
      await userEvent.click(screen.getByRole("button", { name: "Download" }));
      await userEvent.click(
        await screen.findByRole("button", { name: "Cancel" }),
      );
      expect(installModpack).not.toHaveBeenCalled();
    });

    it("installs without asking for a pack from the user's own cloud", async () => {
      render(
        <SharedModpackView
          modpack={pack}
          syncTarget={{ syncedAt: 1, modpackId: "id" }}
        />,
      );
      await userEvent.click(screen.getByRole("button", { name: "Download" }));
      await waitFor(() => expect(installModpack).toHaveBeenCalledWith(pack));
    });
  });
});
