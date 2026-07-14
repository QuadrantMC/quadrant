/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AccountNotification, SnackbarHistoryItem } from "../../intefaces";

const getAccountInfo = vi.fn();
const getNews = vi.fn();
const answerInvite = vi.fn();
const openIn = vi.fn();
const readNotification = vi.fn();

const storeGet = vi.fn();
const storeOnKeyChange = vi.fn();
const listen = vi.fn();

vi.mock("../../tools", () => ({
  answerInvite: (...a: unknown[]) => answerInvite(...a),
  getAccountInfo: (...a: unknown[]) => getAccountInfo(...a),
  getNews: (...a: unknown[]) => getNews(...a),
  openIn: (...a: unknown[]) => openIn(...a),
  readNotification: (...a: unknown[]) => readNotification(...a),
}));

vi.mock("../../desktop", () => ({
  createDesktopStore: () => ({
    get: (...a: unknown[]) => storeGet(...a),
    onKeyChange: (...a: unknown[]) => storeOnKeyChange(...a),
  }),
  listen: (...a: unknown[]) => listen(...a),
}));

import Notifications from "./Notifications";

// headlessui's anchored PopoverPanel positions via ResizeObserver, which jsdom
// does not implement. A minimal stub keeps the panel from throwing on open.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}

function historyItem(over: Partial<SnackbarHistoryItem>): SnackbarHistoryItem {
  return {
    message: "Something happened",
    className: "bg-emerald-600 rounded-4xl",
    timeout: 5000,
    count: 1,
    id: Math.random().toString(36).slice(2),
    ...over,
  };
}

function notification(
  over: Partial<AccountNotification>,
): AccountNotification {
  return {
    notification_id: "n1",
    user_id: "u1",
    notification_type: "generic",
    resource_id: null,
    message: "A plain notification",
    created_at: "",
    created_at_unix: 0,
    read: false,
    ...over,
  };
}

// The callback registered for the "refreshNotifications" backend event, so a
// test can push notifications the way the host would.
let refreshNotifications: ((event: { payload: unknown }) => void) | undefined;

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("ResizeObserver", ResizeObserverStub);
  refreshNotifications = undefined;
  getAccountInfo.mockResolvedValue({
    id: "1",
    name: "me",
    login: "me",
    email: "",
    quadrant_sync_limit: 0,
    quadrant_share_limit: 0,
    notifications: [],
  });
  getNews.mockResolvedValue([]);
  storeGet.mockResolvedValue(true);
  storeOnKeyChange.mockResolvedValue(() => {});
  listen.mockImplementation(
    async (event: string, cb: (e: { payload: unknown }) => void) => {
      if (event === "refreshNotifications") {
        refreshNotifications = cb;
      }
      return () => {};
    },
  );
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function renderNotifications(history: SnackbarHistoryItem[] = []) {
  const setSnackbarHistory = vi.fn();
  render(
    <Notifications
      snackBarHistory={history}
      setSnackbarHistory={setSnackbarHistory}
    />,
  );
  return { setSnackbarHistory };
}

async function openPanel() {
  // The only button before opening is the bell toggle.
  await userEvent.click(screen.getByRole("button"));
}

describe("Notifications", () => {
  it("renders the snackbar history with per-item repeat counts", async () => {
    renderNotifications([
      historyItem({ id: "a", message: "Repeated event", count: 3 }),
      historyItem({ id: "b", message: "One-off event", count: 1 }),
    ]);
    await waitFor(() => expect(listen).toHaveBeenCalled());

    await openPanel();

    expect(await screen.findByText("Repeated event")).toBeInTheDocument();
    expect(screen.getByText("One-off event")).toBeInTheDocument();
    // Grouped items expose an "Nx" badge; singletons must not.
    expect(screen.getByText("3x")).toBeInTheDocument();
    expect(screen.queryByText("1x")).not.toBeInTheDocument();
  });

  it("renders account notifications pushed via the refresh event", async () => {
    renderNotifications();
    await waitFor(() => expect(refreshNotifications).toBeTypeOf("function"));

    refreshNotifications?.({
      payload: [
        notification({
          notification_id: "n1",
          message: "You were mentioned",
          created_at_unix: 2,
        }),
      ],
    });

    await openPanel();
    expect(await screen.findByText("You were mentioned")).toBeInTheDocument();
  });

  it("sorts pushed notifications newest-first", async () => {
    renderNotifications();
    await waitFor(() => expect(refreshNotifications).toBeTypeOf("function"));

    refreshNotifications?.({
      payload: [
        notification({
          notification_id: "older",
          message: "Older note",
          created_at_unix: 1,
        }),
        notification({
          notification_id: "newer",
          message: "Newer note",
          created_at_unix: 5,
        }),
      ],
    });

    await openPanel();
    await screen.findByText("Newer note");

    const older = screen.getByText("Older note");
    const newer = screen.getByText("Newer note");
    // Newer entry appears earlier in document order.
    expect(
      newer.compareDocumentPosition(older) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("clears the snackbar history when the clear button is pressed", async () => {
    const { setSnackbarHistory } = renderNotifications([
      historyItem({ id: "a", message: "Repeated event", count: 3 }),
    ]);
    await waitFor(() => expect(listen).toHaveBeenCalled());
    await openPanel();

    const clearButton = await screen.findByRole("button", { name: /clear/i });
    await userEvent.click(clearButton);
    expect(setSnackbarHistory).toHaveBeenCalledWith([]);
  });
});
