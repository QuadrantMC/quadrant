/** @format */

import { describe, expect, it } from "vitest";
import type { SnackbarHistoryItem, SnackbarState } from "./intefaces";
import { SNACKBAR_HISTORY_LIMIT, appendSnackbarHistory } from "./snackbar";

function state(message: string, className = "bg-red-700"): SnackbarState {
  return { message, className, timeout: 5000 };
}

describe("appendSnackbarHistory", () => {
  it("adds a new entry with count 1 and the given id", () => {
    const history = appendSnackbarHistory([], state("hello"), "id-1");
    expect(history).toHaveLength(1);
    expect(history[0].count).toBe(1);
    expect(history[0].id).toBe("id-1");
  });

  it("increments the count and moves a repeated message to the end", () => {
    const existing: SnackbarHistoryItem[] = [
      { ...state("a"), count: 1, id: "a" },
      { ...state("b"), count: 1, id: "b" },
    ];
    const next = appendSnackbarHistory(existing, state("a"), "a2");
    expect(next.map((i) => i.id)).toEqual(["b", "a"]);
    expect(next[1].count).toBe(2);
    // A repeat must not mint a new id.
    expect(next.find((i) => i.id === "a2")).toBeUndefined();
  });

  it("treats different classNames as distinct notifications", () => {
    const existing: SnackbarHistoryItem[] = [
      { ...state("a", "bg-red-700"), count: 1, id: "a" },
    ];
    const next = appendSnackbarHistory(existing, state("a", "bg-green-700"), "b");
    expect(next).toHaveLength(2);
  });

  it("caps history at the most recent SNACKBAR_HISTORY_LIMIT groups", () => {
    let history: SnackbarHistoryItem[] = [];
    for (let i = 0; i < SNACKBAR_HISTORY_LIMIT + 3; i++) {
      history = appendSnackbarHistory(history, state(`m${i}`), `id-${i}`);
    }
    expect(history).toHaveLength(SNACKBAR_HISTORY_LIMIT);
    expect(history[history.length - 1].id).toBe(
      `id-${SNACKBAR_HISTORY_LIMIT + 2}`,
    );
  });
});
