/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";

const getAccountInfo = vi.fn();
const startOAuthServer = vi.fn();
const cancelOAuthServer = vi.fn();
const onOAuthUrl = vi.fn();
const storeSet = vi.fn();
const clearAccountToken = vi.fn();
const emit = vi.fn();
const listen = vi.fn();

vi.mock("../../../tools", () => ({
  clearAccountToken: (...a: unknown[]) => clearAccountToken(...a),
  getAccountInfo: (...a: unknown[]) => getAccountInfo(...a),
  openIn: vi.fn(),
}));

vi.mock("../../../desktop", () => ({
  cancelOAuthServer: (...a: unknown[]) => cancelOAuthServer(...a),
  createDesktopStore: () => ({
    get: vi.fn().mockResolvedValue(null),
    set: (...a: unknown[]) => storeSet(...a),
  }),
  emit: (...a: unknown[]) => emit(...a),
  invoke: vi.fn().mockResolvedValue("client-id"),
  listen: (...a: unknown[]) => listen(...a),
  onOAuthUrl: (...a: unknown[]) => onOAuthUrl(...a),
  startOAuthServer: (...a: unknown[]) => startOAuthServer(...a),
}));

import AccountPage from "./AccountPage";

beforeEach(() => {
  vi.clearAllMocks();
  getAccountInfo.mockResolvedValue(null);
  listen.mockResolvedValue(() => {});
  emit.mockResolvedValue(undefined);
  clearAccountToken.mockResolvedValue(undefined);
  storeSet.mockResolvedValue(undefined);
  cancelOAuthServer.mockResolvedValue(undefined);
  onOAuthUrl.mockResolvedValue(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("AccountPage", () => {
  it("starts a single OAuth callback server when sign-in is clicked twice", async () => {
    let releaseServer!: (port: number) => void;
    startOAuthServer.mockReturnValue(
      new Promise<number>((resolve) => {
        releaseServer = resolve;
      }),
    );
    render(<AccountPage />);
    const signIn = await screen.findByRole("button", {
      name: /sign ?in ?with/i,
    });

    fireEvent.click(signIn);
    fireEvent.click(signIn);
    await waitFor(() => expect(startOAuthServer).toHaveBeenCalledTimes(1));
    expect(signIn).toBeDisabled();

    await act(async () => releaseServer(4000));
    await waitFor(() => expect(onOAuthUrl).toHaveBeenCalledTimes(1));
    expect(startOAuthServer).toHaveBeenCalledTimes(1);
    expect(cancelOAuthServer).not.toHaveBeenCalled();
    await waitFor(() => expect(signIn).not.toBeDisabled());
  });

  it("re-enables sign-in after the callback server fails to start", async () => {
    startOAuthServer.mockRejectedValue(new Error("port busy"));
    render(<AccountPage />);
    const signIn = await screen.findByRole("button", {
      name: /sign ?in ?with/i,
    });

    fireEvent.click(signIn);
    await waitFor(() => expect(startOAuthServer).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(signIn).not.toBeDisabled());

    fireEvent.click(signIn);
    await waitFor(() => expect(startOAuthServer).toHaveBeenCalledTimes(2));
  });

  it("lets go of a listener that resolves after the page is gone", async () => {
    const unlisten = vi.fn();
    let registered!: (unlisten: () => void) => void;
    listen.mockReturnValue(
      new Promise((resolve) => {
        registered = resolve;
      }),
    );
    const { unmount } = render(<AccountPage />);
    await waitFor(() => expect(listen).toHaveBeenCalled());

    unmount();
    await act(async () => registered(unlisten));

    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  describe("leaving the page during sign-in", () => {
    async function startLogin(port: number) {
      startOAuthServer.mockResolvedValue(port);
      const view = render(<AccountPage />);
      const signIn = await screen.findByRole("button", {
        name: /sign ?in ?with/i,
      });
      fireEvent.click(signIn);
      await waitFor(() => expect(onOAuthUrl).toHaveBeenCalled());
      return view;
    }

    it("keeps the callback server running so the login can still finish", async () => {
      const { unmount } = await startLogin(4000);

      unmount();

      expect(cancelOAuthServer).not.toHaveBeenCalled();
    });

    it("shuts the abandoned server down when the page starts the next sign-in", async () => {
      const first = await startLogin(4000);
      first.unmount();
      cancelOAuthServer.mockClear();

      await startLogin(4001);

      expect(cancelOAuthServer).toHaveBeenCalledExactlyOnceWith(4000);
    });
  });

  it("tells the rest of the app when the user signs out", async () => {
    getAccountInfo.mockResolvedValue({
      name: "Me",
      login: "me",
      email: "me@example.com",
      quadrant_sync_limit: 1,
      quadrant_share_limit: 1,
    });
    render(<AccountPage />);

    fireEvent.click(await screen.findByRole("button", { name: /sign ?out/i }));

    await waitFor(() =>
      expect(emit).toHaveBeenCalledWith("recheckAccountToken", null),
    );
    expect(clearAccountToken).toHaveBeenCalled();
  });
});
