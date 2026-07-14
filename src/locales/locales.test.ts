/** @format */

import { describe, expect, it } from "vitest";
import en from "./en.json";
import tr from "./tr.json";
import uk from "./uk.json";

const locales = { en, tr, uk } as const;
const reference = "en";

describe("locale files", () => {
  const referenceKeys = Object.keys(locales[reference]).sort();

  it.each(Object.keys(locales).filter((l) => l !== reference))(
    "%s has exactly the same keys as en",
    (locale) => {
      const keys = Object.keys(
        locales[locale as keyof typeof locales],
      ).sort();
      const missing = referenceKeys.filter((k) => !keys.includes(k));
      const extra = keys.filter((k) => !referenceKeys.includes(k));
      expect({ missing, extra }).toEqual({ missing: [], extra: [] });
    },
  );

  it.each(Object.keys(locales))("%s has no empty values", (locale) => {
    const empty = Object.entries(locales[locale as keyof typeof locales])
      .filter(([, value]) => typeof value !== "string" || value.trim() === "")
      .map(([key]) => key);
    expect(empty).toEqual([]);
  });
});
