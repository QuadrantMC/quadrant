/** @format */

import { describe, expect, it } from "vitest";
import { ModSource, type LocalMod } from "./intefaces";
import { hasWarnings, reviewMods } from "./modpackReview";

const hash = "a".repeat(128);
function mod(over: Partial<LocalMod>): LocalMod {
  return {
    id: "sodium",
    downloadUrl: "https://cdn.modrinth.com/data/sodium.jar",
    source: ModSource.Modrinth,
    sha512: hash,
    ...over,
  };
}

describe("reviewMods", () => {
  it("has nothing to say about a pack of hashed provider mods", () => {
    const review = reviewMods([mod({}), mod({ id: "iris" })]);

    expect(review).toEqual({ online: [], unverified: [] });
    expect(hasWarnings(review)).toBe(false);
  });

  it("lists the mods without a hash", () => {
    const review = reviewMods([
      mod({}),
      mod({ id: "lithium", name: "Lithium", sha512: undefined }),
      mod({ id: "iris", sha512: "" }),
    ]);

    expect(review.unverified.map((m) => m.name)).toEqual(["Lithium", "iris"]);
    expect(review.online).toEqual([]);
    expect(hasWarnings(review)).toBe(true);
  });

  it("lists Online mods with the host they download from", () => {
    const review = reviewMods([
      mod({
        id: "custom",
        slug: "custom-mod",
        source: ModSource.Online,
        downloadUrl: "https://files.example.org/a/custom.jar",
      }),
    ]);

    expect(review.online).toEqual([
      { name: "custom-mod", host: "files.example.org" },
    ]);
    expect(review.unverified).toHaveLength(0);
  });

  it("puts an unhashed Online mod in both lists and survives a bad link", () => {
    const review = reviewMods([
      mod({
        id: "odd",
        source: ModSource.Online,
        downloadUrl: "not a link",
        sha512: undefined,
      }),
    ]);

    expect(review.online).toEqual([{ name: "odd", host: "" }]);
    expect(review.unverified).toEqual([{ name: "odd", host: "" }]);
  });
});
