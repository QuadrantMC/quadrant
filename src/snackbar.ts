/** @format */

import type { SnackbarHistoryItem, SnackbarState } from "./intefaces";

export const SNACKBAR_HISTORY_LIMIT = 5;

function messageKeyOf(state: {
  message: SnackbarState["message"];
}): string {
  return typeof state.message === "string"
    ? state.message
    : JSON.stringify(state.message);
}

/**
 * Groups a new snackbar into the history: repeats of the same
 * message+className bump the count and move to the end; the history is
 * capped at the most recent SNACKBAR_HISTORY_LIMIT groups.
 */
export function appendSnackbarHistory(
  prevHistory: SnackbarHistoryItem[],
  newState: SnackbarState,
  id: string,
): SnackbarHistoryItem[] {
  const messageKey = messageKeyOf(newState);
  const existingIndex = prevHistory.findIndex(
    (item) =>
      messageKeyOf(item) === messageKey &&
      item.className === newState.className,
  );

  let newHistory: SnackbarHistoryItem[];
  if (existingIndex !== -1) {
    const existingItem = prevHistory[existingIndex];
    newHistory = [
      ...prevHistory.slice(0, existingIndex),
      ...prevHistory.slice(existingIndex + 1),
      { ...existingItem, count: existingItem.count + 1 },
    ];
  } else {
    newHistory = [...prevHistory, { ...newState, count: 1, id }];
  }

  if (newHistory.length > SNACKBAR_HISTORY_LIMIT) {
    newHistory = newHistory.slice(-SNACKBAR_HISTORY_LIMIT);
  }
  return newHistory;
}
