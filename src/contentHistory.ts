/** @format */

import type { Page } from "./intefaces";

export interface PageWithScroll {
  page: Page;
  scrollPositionX: number;
  scrollPositionY: number;
}

interface Scroll {
  x: number;
  y: number;
}

/** The history after opening `next` on top of `current`, remembering where
 *  `current` was scrolled so going back can restore it. */
export function pushContent(
  history: PageWithScroll[],
  current: Page,
  next: Page,
  scroll: Scroll,
): PageWithScroll[] {
  const leaving = history[history.length - 1]?.page ?? current;
  return [
    ...history.slice(0, -1),
    { page: leaving, scrollPositionX: scroll.x, scrollPositionY: scroll.y },
    { page: next, scrollPositionX: 0, scrollPositionY: 0 },
  ];
}

/** The history after going back, or `null` when there is nowhere to go. */
export function popContent(
  history: PageWithScroll[],
): { history: PageWithScroll[]; entry: PageWithScroll } | null {
  if (history.length < 2) return null;
  const remaining = history.slice(0, -1);
  return { history: remaining, entry: remaining[remaining.length - 1] };
}
