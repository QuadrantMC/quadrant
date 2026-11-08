/** @format */

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { type ReactNode } from "react";
import { ContentContext, ModLoader } from "../../../intefaces";
import { useModpackInstall } from "./useModpackInstall";

const { clearProgressBar, installModpack, invoke, listeners, setSnackbar } =
  vi.hoisted(() => ({
    clearProgressBar: vi.fn(async () => {}),
    installModpack: vi.fn(),
    invoke: vi.fn(),
    listeners: new Set<(event: { payload: number }) => void>(),
    setSnackbar: vi.fn(),
  }));
vi.mock("../../../tools", () => ({ installModpack }));
vi.mock("../../../desktop", () => ({
  clearProgressBar,
  invoke,
  listen: async (_event: string, cb: (event: { payload: number }) => void) => {
    listeners.add(cb);
    return () => listeners.delete(cb);
  },
}));

const pack = {
  name: "Pack",
  mods: [],
  version: "1.21",
  modLoader: ModLoader.Fabric,
};
const syncTarget = { syncedAt: 42, modpackId: "id" };
function wrapper({ children }: { children: ReactNode }) {
  return (
    <ContentContext.Provider
      value={{
        back: vi.fn(),
        changePage: vi.fn(),
        changeContent: vi.fn(),
        setSnackbar,
        setSnackbarNoState: vi.fn(),
      }}
    >
      {children}
    </ContentContext.Provider>
  );
}
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
beforeEach(() => {
  vi.resetAllMocks();
  listeners.clear();
  installModpack.mockResolvedValue(undefined);
  invoke.mockResolvedValue(undefined);
});

describe("useModpackInstall", () => {
  it("stays busy through completion events until sync metadata is written", async () => {
    const metadata = deferred();
    invoke.mockReturnValue(metadata.promise);
    const { result } = renderHook(() => useModpackInstall(pack, syncTarget), {
      wrapper,
    });
    let operation!: Promise<void>;
    await act(async () => {
      operation = result.current.install();
    });
    await act(async () => {
      listeners.forEach((cb) => cb({ payload: 1 }));
    });
    expect(setSnackbar).not.toHaveBeenCalled();
    expect(result.current.progress).toBeLessThan(1);
    await act(async () => {
      await result.current.install();
    });
    expect(installModpack).toHaveBeenCalledOnce();
    await act(async () => {
      metadata.resolve();
      await operation;
    });
    expect(result.current.progress).toBe(1);
    expect(setSnackbar).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ message: "Mod(s) installed successfully" }),
    );
  });

  it("allows another install when the command succeeds without a progress event", async () => {
    const { result } = renderHook(() => useModpackInstall(pack), { wrapper });
    await act(async () => {
      await result.current.install();
    });
    await act(async () => {
      await result.current.install();
    });
    expect(installModpack).toHaveBeenCalledTimes(2);
  });

  it("does not report success when saving sync metadata fails", async () => {
    invoke.mockRejectedValue("failed");
    const { result } = renderHook(() => useModpackInstall(pack, syncTarget), {
      wrapper,
    });
    await act(async () => {
      await result.current.install();
    });
    expect(setSnackbar).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ className: "bg-red-700" }),
    );
    expect(result.current.progress).toBe(1);
  });

  it("serializes cards and suppresses duplicate installs across page changes", async () => {
    const first = deferred();
    installModpack.mockReturnValueOnce(first.promise);
    const a = renderHook(() => useModpackInstall(pack));
    const b = renderHook(() => useModpackInstall({ ...pack, name: "Other" }));
    const duplicate = renderHook(() => useModpackInstall(pack));
    let aOperation!: Promise<void>;
    let bOperation!: Promise<void>;
    await act(async () => {
      aOperation = a.result.current.install();
      bOperation = b.result.current.install();
      await duplicate.result.current.install();
    });
    expect(installModpack).toHaveBeenCalledOnce();
    a.unmount();
    await act(async () => {
      first.resolve();
      await Promise.all([aOperation, bOperation]);
    });
    expect(installModpack).toHaveBeenCalledTimes(2);
    expect(installModpack).toHaveBeenLastCalledWith(
      expect.objectContaining({ name: "Other" }),
    );
  });
});
