/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";

const getAccountInfo = vi.fn();
const startOAuthServer = vi.fn();
const cancelOAuthServer = vi.fn();
const onOAuthUrl = vi.fn();
const storeSet = vi.fn();
const clearAccountToken = vi.fn();
const emit = vi.fn();
const listen = vi.fn();
const invoke = vi.fn();
const openIn = vi.fn();
// What the config store holds; sign-in keeps its state and verifier here.
const store = new Map<string, unknown>();

vi.mock("../../../tools", () => ({
  clearAccountToken: (...a: unknown[]) => clearAccountToken(...a),
  getAccountInfo: (...a: unknown[]) => getAccountInfo(...a),
  openIn: (...a: unknown[]) => openIn(...a),
}));

vi.mock("../../../desktop", () => ({
  cancelOAuthServer: (...a: unknown[]) => cancelOAuthServer(...a),
  createDesktopStore: () => ({
    get: async (key: string) => store.get(key) ?? null,
    set: async (key: string, value: unknown) => {
      store.set(key, value);
      await storeSet(key, value);
    },
    delete: async (key: string) => {
      store.delete(key);
    },
  }),
  emit: (...a: unknown[]) => emit(...a),
  invoke: (...a: unknown[]) => invoke(...a),
  listen: (...a: unknown[]) => listen(...a),
  onOAuthUrl: (...a: unknown[]) => onOAuthUrl(...a),
  startOAuthServer: (...a: unknown[]) => startOAuthServer(...a),
}));

import AccountPage from "./AccountPage";
import { codeChallengeS256 } from "../../../pkce";

beforeEach(() => {
  vi.clearAllMocks();
  store.clear();
  invoke.mockResolvedValue("client-id");
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

  describe("the OAuth redirect", () => {
    async function startLogin() {
      startOAuthServer.mockResolvedValue(4000);
      render(<AccountPage />);
      fireEvent.click(
        await screen.findByRole("button", { name: /sign ?in ?with/i }),
      );
      await waitFor(() => expect(onOAuthUrl).toHaveBeenCalled());
      const authUrl = new URL(openIn.mock.calls[0][0] as string);
      const redirect = onOAuthUrl.mock.calls[0][0] as (
        url: string,
      ) => Promise<void>;
      return { authUrl, redirect };
    }

    it("asks for S256 and sends the matching verifier when the code is exchanged", async () => {
      const { authUrl, redirect } = await startLogin();
      const verifier = store.get("oauthVerifier") as string;
      const state = authUrl.searchParams.get("state");

      expect(authUrl.searchParams.get("code_challenge_method")).toBe("S256");
      expect(authUrl.searchParams.get("code_challenge")).toBe(
        await codeChallengeS256(verifier),
      );
      expect(state).toBe(store.get("oauthState"));

      await act(async () =>
        redirect(`http://127.0.0.1:4000/?code=abc&state=${state}`),
      );

      expect(invoke).toHaveBeenCalledWith("oauth2_login", {
        code: "abc",
        redirectUri: "http://127.0.0.1:4000",
        codeVerifier: verifier,
      });
    });

    it("accepts a state once, so a replayed redirect is rejected", async () => {
      vi.spyOn(console, "error").mockImplementation(() => {});
      const { authUrl, redirect } = await startLogin();
      const url = `http://127.0.0.1:4000/?code=abc&state=${authUrl.searchParams.get("state")}`;

      await act(async () => redirect(url));
      expect(store.has("oauthState")).toBe(false);
      expect(store.has("oauthVerifier")).toBe(false);
      await act(async () => redirect(url));

      expect(
        invoke.mock.calls.filter(([command]) => command === "oauth2_login"),
      ).toHaveLength(1);
    });

    it("rejects a redirect carrying another state", async () => {
      vi.spyOn(console, "error").mockImplementation(() => {});
      const { redirect } = await startLogin();

      await act(async () =>
        redirect("http://127.0.0.1:4000/?code=abc&state=forged"),
      );

      expect(invoke).not.toHaveBeenCalledWith(
        "oauth2_login",
        expect.anything(),
      );
    });
  });

  it("shows the email only when the account info carries one", async () => {
    const account = {
      name: "Me",
      login: "me",
      quadrant_sync_limit: 1,
      quadrant_share_limit: 1,
    };
    getAccountInfo.mockResolvedValue({ ...account, email: "" });
    const { unmount } = render(<AccountPage />);
    // The test renders translation keys.
    expect(await screen.findByText("username: me")).toBeTruthy();
    expect(screen.queryByText(/^email:/)).toBeNull();
    unmount();

    getAccountInfo.mockResolvedValue({ ...account, email: "me@example.com" });
    render(<AccountPage />);
    expect(await screen.findByText("email: me@example.com")).toBeTruthy();
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
