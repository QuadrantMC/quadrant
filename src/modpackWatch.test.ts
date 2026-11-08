/** @format */

import { beforeEach, describe, expect, it, vi } from "vitest";

type Listener = () => void | Promise<void>;
interface Watch {
  path: string;
  listener: Listener;
  options?: { recursive?: boolean };
  unwatch: ReturnType<typeof vi.fn>;
}

const hosts = vi.hoisted(() => ({
  watches: [] as Watch[],
  failTree: 0,
  /** Holds the next recursive watch until it is called. */
  holdTree: null as null | (() => void),
  holdTreeOn: false,
}));

vi.mock("./desktop", () => ({
  joinPath: async (...segments: string[]) => segments.join("/"),
  watch: async (
    path: string,
    listener: Listener,
    options?: { recursive?: boolean },
  ) => {
    if (options?.recursive && hosts.failTree > 0) {
      hosts.failTree--;
      throw new Error("modpacks folder does not exist");
    }
    if (options?.recursive && hosts.holdTreeOn) {
      hosts.holdTreeOn = false;
      await new Promise<void>((resolve) => {
        hosts.holdTree = resolve;
      });
    }
    const unwatch = vi.fn();
    hosts.watches.push({ path, listener, options, unwatch });
    return unwatch;
  },
}));

import { watchModpacks } from "./modpackWatch";

const tree = () => hosts.watches.find((w) => w.options?.recursive);
const folder = () => hosts.watches.find((w) => !w.options?.recursive);

beforeEach(() => {
  hosts.watches.length = 0;
  hosts.failTree = 0;
  hosts.holdTree = null;
  hosts.holdTreeOn = false;
  vi.spyOn(console, "error").mockImplementation(() => {});
});

describe("watchModpacks", () => {
  it("watches the Minecraft folder flat and the modpacks tree recursively", async () => {
    await watchModpacks("mc", vi.fn());

    expect(folder()?.path).toBe("mc");
    expect(tree()?.path).toBe("mc/modpacks");
    expect(hosts.watches).toHaveLength(2);
  });

  it("reports changes from both watchers", async () => {
    const onChange = vi.fn();
    await watchModpacks("mc", onChange);

    await tree()!.listener();
    expect(onChange).toHaveBeenCalledTimes(1);
    await folder()!.listener();
    expect(onChange).toHaveBeenCalledTimes(2);
  });

  it("attaches the tree watch once the first pack creates the folder", async () => {
    hosts.failTree = 1;
    const onChange = vi.fn();
    await watchModpacks("mc", onChange);
    expect(tree()).toBeUndefined();

    await folder()!.listener();

    expect(tree()?.path).toBe("mc/modpacks");
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("keeps retrying while the tree is missing and attaches it only once", async () => {
    hosts.failTree = 2;
    await watchModpacks("mc", vi.fn());

    await folder()!.listener();
    expect(tree()).toBeUndefined();
    await folder()!.listener();
    expect(tree()).toBeDefined();
    await folder()!.listener();

    expect(hosts.watches.filter((w) => w.options?.recursive)).toHaveLength(1);
  });

  it("stops both watchers on dispose", async () => {
    const dispose = await watchModpacks("mc", vi.fn());

    dispose();

    expect(folder()!.unwatch).toHaveBeenCalledTimes(1);
    expect(tree()!.unwatch).toHaveBeenCalledTimes(1);
  });

  it("ignores events and attaches nothing after dispose", async () => {
    hosts.failTree = 1;
    const onChange = vi.fn();
    const dispose = await watchModpacks("mc", onChange);

    dispose();
    await folder()!.listener();

    expect(onChange).not.toHaveBeenCalled();
    expect(tree()).toBeUndefined();
  });

  it("stops a tree watch that finishes attaching after dispose", async () => {
    hosts.failTree = 1;
    const onChange = vi.fn();
    const dispose = await watchModpacks("mc", onChange);

    hosts.holdTreeOn = true;
    const event = folder()!.listener();
    await vi.waitFor(() => expect(hosts.holdTree).not.toBeNull());
    dispose();
    hosts.holdTree!();
    await event;

    expect(tree()!.unwatch).toHaveBeenCalledTimes(1);
    expect(onChange).not.toHaveBeenCalled();
  });
});
