/** @format */

import { describe, expect, it } from "vitest";
import { popContent, pushContent, type PageWithScroll } from "./contentHistory";
import type { Page } from "./intefaces";

const page = (name: string): Page => ({
  name,
  title: name,
  content: null,
  icon: null,
  style: "",
  main: false,
});

const only = (name: string): PageWithScroll[] => [
  { page: page(name), scrollPositionX: 0, scrollPositionY: 0 },
];

describe("content history", () => {
  it("seeds the history with the page being left", () => {
    const apply = page("apply");
    const details = page("details");

    const history = pushContent([], apply, details, { x: 3, y: 40 });

    expect(history.map((entry) => entry.page.name)).toEqual([
      "apply",
      "details",
    ]);
    expect(history[0]).toMatchObject({ scrollPositionX: 3, scrollPositionY: 40 });
  });

  it("records the scroll of the page being left, not of the new one", () => {
    const start = only("search");

    const history = pushContent(start, page("ignored"), page("mod"), {
      x: 0,
      y: 120,
    });

    expect(history[0]).toMatchObject({
      page: { name: "search" },
      scrollPositionY: 120,
    });
    expect(history[1]).toMatchObject({ page: { name: "mod" }, scrollPositionY: 0 });
  });

  it("goes back to the page that opened the current one", () => {
    const history = pushContent([], page("apply"), page("details"), {
      x: 0,
      y: 0,
    });

    const popped = popContent(history);

    expect(popped?.entry.page.name).toBe("apply");
    expect(popped?.history).toHaveLength(1);
  });

  it("has nowhere to go back to from the first page", () => {
    expect(popContent([])).toBeNull();
    expect(popContent(only("apply"))).toBeNull();
  });

  // The defect this module fixes: a `back` captured before the push used to
  // read a history that did not contain the pushed view, and did nothing.
  it("a push is visible to a back that was created before it", () => {
    const ref = { current: [] as PageWithScroll[] };
    const back = () => popContent(ref.current);

    ref.current = pushContent(ref.current, page("apply"), page("details"), {
      x: 0,
      y: 0,
    });

    expect(back()?.entry.page.name).toBe("apply");
  });
});
