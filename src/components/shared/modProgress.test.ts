/** @format */
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ listen: vi.fn() }));
vi.mock("../../desktop", () => ({ listen: mocks.listen }));

import {
  subscribeModDownloadProgress,
  subscribeModInstallProgress,
} from "./modProgress";

describe("modProgress", () => {
  let handlers: Record<string, (event: unknown) => void>;
  let unlisten: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    handlers = {};
    unlisten = vi.fn();
    mocks.listen.mockReset();
    mocks.listen.mockImplementation(async (name: string, cb: never) => {
      handlers[name] = cb;
      return unlisten;
    });
  });

  it("registers one listener for any number of subscribers and routes by mod id", async () => {
    const a = vi.fn();
    const b = vi.fn();
    const stopA = subscribeModDownloadProgress("a", a);
    const stopB = subscribeModDownloadProgress("b", b);
    expect(mocks.listen).toHaveBeenCalledTimes(1);
    await Promise.resolve();

    handlers.modDownloadProgress({ payload: { modId: "a", progress: 10 } });
    expect(a).toHaveBeenCalledWith(10);
    expect(b).not.toHaveBeenCalled();

    stopA();
    handlers.modDownloadProgress({ payload: { modId: "a", progress: 20 } });
    expect(a).toHaveBeenCalledTimes(1);
    expect(unlisten).not.toHaveBeenCalled();

    stopB();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
  });

  it("keeps the two events apart and listens again after everyone left", async () => {
    const stop = subscribeModInstallProgress("a", vi.fn());
    expect(mocks.listen).toHaveBeenCalledWith(
      "modInstallProgress",
      expect.any(Function),
    );
    stop();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
    subscribeModInstallProgress("a", vi.fn())();
    expect(mocks.listen).toHaveBeenCalledTimes(2);
  });
});
