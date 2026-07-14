/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

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
  default: ({ name }: { name: string }) => <div data-testid="modpack-view">{name}</div>,
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
  joinPath.mockResolvedValue("/mc/mods");
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

  it("registers a filesystem watch on the mods folder", async () => {
    getModpacks.mockResolvedValue([pack({})]);
    render(<CurrentModpackPage />);
    await waitFor(() => expect(watch).toHaveBeenCalledTimes(1));
    expect(watch).toHaveBeenCalledWith(
      "/mc/mods",
      expect.any(Function),
      { delayMs: 500 },
    );
  });

  it("tears down the watch on unmount", async () => {
    const unwatch = vi.fn();
    watch.mockResolvedValue(unwatch);
    getModpacks.mockResolvedValue([pack({})]);

    const { unmount } = render(<CurrentModpackPage />);
    await waitFor(() => expect(watch).toHaveBeenCalled());
    unmount();
    await waitFor(() => expect(unwatch).toHaveBeenCalled());
  });
});
