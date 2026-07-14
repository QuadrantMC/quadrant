/** @format */

import { afterEach, describe, expect, it } from "vitest";
import {
  COMPACT_UI_SCALE,
  MAX_UI_SCALE,
  MIN_UI_SCALE,
  applyUiScale,
  clampUiScale,
  getAppliedUiScale,
} from "./uiScale";

afterEach(() => {
  document.documentElement.style.fontSize = "";
});

describe("clampUiScale", () => {
  it("returns the compact default for non-numeric input", () => {
    expect(clampUiScale(null)).toBe(COMPACT_UI_SCALE);
    expect(clampUiScale(undefined)).toBe(COMPACT_UI_SCALE);
    expect(clampUiScale(Number.NaN)).toBe(COMPACT_UI_SCALE);
    expect(clampUiScale(Number.POSITIVE_INFINITY)).toBe(COMPACT_UI_SCALE);
    expect(clampUiScale(Number.NEGATIVE_INFINITY)).toBe(COMPACT_UI_SCALE);
  });

  it("clamps to the allowed range", () => {
    expect(clampUiScale(0)).toBe(MIN_UI_SCALE);
    expect(clampUiScale(-500)).toBe(MIN_UI_SCALE);
    expect(clampUiScale(10_000)).toBe(MAX_UI_SCALE);
    expect(clampUiScale(MIN_UI_SCALE)).toBe(MIN_UI_SCALE);
    expect(clampUiScale(MAX_UI_SCALE)).toBe(MAX_UI_SCALE);
  });

  it("rounds fractional scales", () => {
    expect(clampUiScale(99.4)).toBe(99);
    expect(clampUiScale(99.5)).toBe(100);
  });
});

describe("applyUiScale / getAppliedUiScale", () => {
  it("round-trips a scale through the root font size", () => {
    applyUiScale(150);
    expect(document.documentElement.style.fontSize).toBe("21px");
    expect(getAppliedUiScale()).toBe(150);
  });

  it("applies the clamped value for out-of-range input", () => {
    applyUiScale(9999);
    expect(getAppliedUiScale()).toBe(MAX_UI_SCALE);
  });

  it("falls back to the compact default when nothing is applied", () => {
    expect(getAppliedUiScale()).toBe(COMPACT_UI_SCALE);
  });
});
