/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";

const getModpacks = vi.fn();
const getMinecraftFolder = vi.fn();
const joinPath = vi.fn();
const watch = vi.fn();

vi.mock("../../../tools", () => ({
  getModpacks: (...a: unknown[]) => getModpacks(...a),
  getMinecraftFolder: (...a: unknown[]) => getMinecraftFolder(...a),
}));
vi.mock("../../../desktop", () => ({
  joinPath: (...a: unknown[]) => joinPath(...a),
  watch: (...a: unknown[]) => watch(...a),
}));
// ModpackView is heavy and independently tested; stub it to a marker.
vi.mock("../../shared/Pages/ModpackView", () => ({
  default: ({ name }: { name: string }) => (
    <div data-testid="modpack-view">{name}</div>
  ),
}));

import CurrentModpackPage from "./CurrentModpackPage";

function pack(over: Record<string, unknown>) {
  return {
    name: "Applied Pack",
    version: "1.20.1",
    modLoader: "fabric",
    isApplied: true,
    lastSynced: 0,
    mods: [],
    unknownMods: [],
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  getMinecraftFolder.mockResolvedValue("/mc");
  joinPath.mockImplementation(async (...parts: string[]) => parts.join("/"));
  watch.mockResolvedValue(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("CurrentModpackPage", () => {
  it("renders the applied modpack once loaded", async () => {
    getModpacks.mockResolvedValue([
      pack({ name: "Not Applied", isApplied: false }),
      pack({ name: "Applied Pack", isApplied: true }),
    ]);

    render(<CurrentModpackPage />);
    await waitFor(() =>
      expect(screen.getByTestId("modpack-view")).toHaveTextContent(
        "Applied Pack",
      ),
    );
  });

  it("shows the placeholder when no modpack is applied", async () => {
    getModpacks.mockResolvedValue([pack({ isApplied: false })]);
    render(<CurrentModpackPage />);
    // watch is still set up; the view marker must never appear.
    await waitFor(() => expect(watch).toHaveBeenCalled());
    expect(screen.queryByTestId("modpack-view")).not.toBeInTheDocument();
  });

  // The mods link is replaced on apply and absent until a pack was ever
  // applied, so it is never watched itself.
  it("watches the Minecraft folder and the modpacks tree, not the mods link", async () => {
    getModpacks.mockResolvedValue([pack({})]);
    render(<CurrentModpackPage />);
    await waitFor(() => expect(watch).toHaveBeenCalledTimes(2));
    expect(watch).toHaveBeenCalledWith("/mc", expect.any(Function), {
      delayMs: 50,
    });
    expect(watch).toHaveBeenCalledWith("/mc/modpacks", expect.any(Function), {
      delayMs: 50,
      recursive: true,
    });
    expect(watch).not.toHaveBeenCalledWith(
      "/mc/mods",
      expect.anything(),
      expect.anything(),
    );
  });

  it("tears down the watch on unmount", async () => {
    const unwatch = vi.fn();
    watch.mockResolvedValue(unwatch);
    getModpacks.mockResolvedValue([pack({})]);

    const { unmount } = render(<CurrentModpackPage />);
    await waitFor(() => expect(watch).toHaveBeenCalledTimes(2));
    unmount();
    await waitFor(() => expect(unwatch).toHaveBeenCalledTimes(2));
  });

  it("keeps the latest applied pack when watch refreshes resolve out of order", async () => {
    getModpacks.mockResolvedValue([pack({})]);
    render(<CurrentModpackPage />);
    await waitFor(() => expect(watch).toHaveBeenCalledTimes(2));
    const refresh = watch.mock.calls.find(
      ([path]) => path === "/mc/modpacks",
    )![1];
    let first!: (value: ReturnType<typeof pack>[]) => void;
    let second!: (value: ReturnType<typeof pack>[]) => void;
    getModpacks
      .mockReturnValueOnce(
        new Promise((resolve) => {
          first = resolve;
        }),
      )
      .mockReturnValueOnce(
        new Promise((resolve) => {
          second = resolve;
        }),
      );
    refresh();
    refresh();
    await act(async () => second([pack({ name: "Latest Pack" })]));
    await act(async () => first([pack({ name: "Stale Pack" })]));
    expect(screen.getByTestId("modpack-view")).toHaveTextContent("Latest Pack");
  });

  it("does not resurrect an obsolete effect or leak its watch during StrictMode setup", async () => {
    let first!: (value: ReturnType<typeof pack>[]) => void;
    getModpacks
      .mockReturnValueOnce(
        new Promise((resolve) => {
          first = resolve;
        }),
      )
      .mockResolvedValue([pack({ name: "Current Pack" })]);
    const unwatch = vi.fn();
    watch.mockResolvedValue(unwatch);
    const { unmount } = render(
      <StrictMode>
        <CurrentModpackPage />
      </StrictMode>,
    );
    await waitFor(() => expect(watch).toHaveBeenCalledTimes(2));
    await act(async () => first([pack({ name: "Obsolete Pack" })]));
    expect(screen.getByTestId("modpack-view")).toHaveTextContent(
      "Current Pack",
    );
    expect(watch).toHaveBeenCalledTimes(2);
    unmount();
    expect(unwatch).toHaveBeenCalledTimes(2);
  });
});
