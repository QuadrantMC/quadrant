/** @format */

import { describe, expect, it } from "vitest";
import { ModSource } from "./intefaces";
import {
  getQuadrantCode,
  getQuadrantModId,
  parseShareCode,
  resolveDeepLink,
} from "./deepLinks";

describe("resolveDeepLink — curseforge scheme", () => {
  it("resolves an install link to a CurseForge mod that keeps iterating", () => {
    const action = resolveDeepLink(
      "curseforge://install?addonId=238222&fileId=99",
    );
    expect(action).toEqual({
      kind: "installMod",
      source: ModSource.CurseForge,
      modId: "238222",
      fileId: "99",
      stopAfter: false,
    });
  });

  it("marks non-install actions as unsupported", () => {
    expect(resolveDeepLink("curseforge://open?addonId=1").kind).toBe(
      "unsupported",
    );
  });

  it.each(["curseforge://install", "curseforge://install?addonId=%20"])(
    "rejects an install link without a project ID: %s",
    (url) => {
      expect(resolveDeepLink(url)).toEqual({ kind: "unsupported" });
    },
  );
});

describe("resolveDeepLink — modrinth scheme", () => {
  it("resolves a mod link using the last path segment as the slug", () => {
    const action = resolveDeepLink("modrinth://mod/sodium");
    expect(action).toEqual({
      kind: "installMod",
      source: ModSource.Modrinth,
      modId: "sodium",
      stopAfter: true,
    });
  });

  it("supports resourcepack and shader content types", () => {
    expect(resolveDeepLink("modrinth://resourcepack/faithful").kind).toBe(
      "installMod",
    );
    expect(resolveDeepLink("modrinth://shader/complementary").kind).toBe(
      "installMod",
    );
  });

  it("marks other modrinth actions as unsupported", () => {
    expect(resolveDeepLink("modrinth://user/jellysquid").kind).toBe(
      "unsupported",
    );
  });

  it.each([
    "modrinth://mod/sodium/",
    "modrinth:///mod/sodium",
    "modrinth://modrinth.com/mod/sodium",
    "modrinth://https://modrinth.com/mod/sodium",
  ])("resolves the project from a supported link shape: %s", (url) => {
    expect(resolveDeepLink(url)).toEqual({
      kind: "installMod",
      source: ModSource.Modrinth,
      modId: "sodium",
      stopAfter: true,
    });
  });

  it.each([
    "modrinth://mod",
    "modrinth://mod/",
    "modrinth:///mod/",
    "modrinth://user/mod-fan",
    "modrinth://unsupported-mod/sodium",
  ])("rejects missing projects and unsupported actions: %s", (url) => {
    expect(resolveDeepLink(url)).toEqual({ kind: "unsupported" });
  });
});

describe("resolveDeepLink — quadrantnext scheme", () => {
  it("resolves an OAuth login callback", () => {
    const action = resolveDeepLink(
      "quadrantnext://login?state=abc&code=xyz#fragment",
    );
    expect(action).toEqual({
      kind: "oauthLogin",
      providedState: "abc",
      code: "xyz",
      redirectUri: "quadrantnext://login",
    });
  });

  it("resolves modrinth and curseforge mod actions", () => {
    expect(resolveDeepLink("quadrantnext://modrinth?modId=sodium")).toEqual({
      kind: "installMod",
      source: ModSource.Modrinth,
      modId: "sodium",
      fileId: undefined,
      stopAfter: true,
    });
    expect(
      resolveDeepLink("quadrantnext://curseforge?addonId=1&fileId=2"),
    ).toEqual({
      kind: "installMod",
      source: ModSource.CurseForge,
      modId: "1",
      fileId: "2",
      stopAfter: true,
    });
  });

  it("flags a mod action with no id as unsupported", () => {
    expect(resolveDeepLink("quadrantnext://modrinth").kind).toBe("unsupported");
  });

  it("resolves a modpack share code", () => {
    expect(resolveDeepLink("quadrantnext://modpack?code=1234567")).toEqual({
      kind: "importModpack",
      code: "1234567",
    });
  });

  it("returns none for unrecognized quadrantnext actions", () => {
    expect(resolveDeepLink("quadrantnext://mystery").kind).toBe("none");
  });
});

describe("resolveDeepLink — https scheme", () => {
  it("imports a 7-digit modpack code from the marketing domain", () => {
    expect(
      resolveDeepLink("https://usequadrant.dev/modpack/1234567"),
    ).toEqual({ kind: "importModpack", code: "1234567" });
    expect(
      resolveDeepLink("https://www.usequadrant.dev/modpack/7654321").kind,
    ).toBe("importModpack");
  });

  it("ignores non-7-digit codes and other hosts", () => {
    expect(resolveDeepLink("https://usequadrant.dev/modpack/12").kind).toBe(
      "none",
    );
    expect(
      resolveDeepLink("https://evil.example.com/modpack/1234567").kind,
    ).toBe("none");
  });
});

describe("resolveDeepLink — invalid input", () => {
  it("throws on a non-URL string (caller catches and shows the error snackbar)", () => {
    expect(() => resolveDeepLink("not a url")).toThrow();
  });

  it("returns none for unrelated schemes", () => {
    expect(resolveDeepLink("mailto:someone@example.com").kind).toBe("none");
  });
});

describe("helper parsers", () => {
  it("getQuadrantModId prefers modId, then addonId, then the path value", () => {
    expect(getQuadrantModId(new URL("quadrantnext://modrinth?modId=a"))).toBe(
      "a",
    );
    expect(
      getQuadrantModId(new URL("quadrantnext://curseforge?addonId=b")),
    ).toBe("b");
  });

  it("getQuadrantCode prefers code, then sharedCode", () => {
    expect(getQuadrantCode(new URL("quadrantnext://modpack?code=111"))).toBe(
      "111",
    );
    expect(
      getQuadrantCode(new URL("quadrantnext://modpack?sharedCode=222")),
    ).toBe("222");
  });
});

describe("parseShareCode", () => {
  it("accepts a bare 7-digit code", () => {
    expect(parseShareCode("1234567")).toBe("1234567");
  });

  it("accepts a share URL", () => {
    expect(parseShareCode("https://usequadrant.dev/modpack/1234567")).toBe(
      "1234567",
    );
  });

  it("accepts the www host", () => {
    expect(parseShareCode("https://www.usequadrant.dev/modpack/7654321")).toBe(
      "7654321",
    );
  });

  it("accepts a trailing slash", () => {
    expect(parseShareCode("https://usequadrant.dev/modpack/1234567/")).toBe(
      "1234567",
    );
  });

  it("trims surrounding whitespace", () => {
    expect(parseShareCode("  1234567  ")).toBe("1234567");
    expect(
      parseShareCode("  https://usequadrant.dev/modpack/1234567  "),
    ).toBe("1234567");
  });

  it("rejects codes that are not exactly 7 digits", () => {
    expect(parseShareCode("123456")).toBeNull();
    expect(parseShareCode("12345678")).toBeNull();
    expect(parseShareCode("https://usequadrant.dev/modpack/123456")).toBeNull();
  });

  it("rejects other hosts", () => {
    expect(
      parseShareCode("https://evil.example.com/modpack/1234567"),
    ).toBeNull();
  });

  it("rejects non-URL junk", () => {
    expect(parseShareCode("not a url")).toBeNull();
  });
});

describe("resolveDeepLink — provider ids", () => {
  it.each([
    "curseforge://install?addonId=123/files/456",
    "curseforge://install?addonId=1%3Fx%3D",
    "curseforge://install?addonId=abc",
    "curseforge://install?addonId=1&fileId=2/3",
    "curseforge://install?addonId=1&fileId=latest",
    "quadrantnext://curseforge?addonId=1&fileId=..%2F2",
    "quadrantnext://modrinth?modId=sodium%2F..%2Fother",
    "quadrantnext://modrinth?modId=sodium%3Fq%3D1",
    "quadrantnext://modrinth?modId=..",
    "quadrantnext://modrinth?modId=sodium&fileId=a/b",
    "modrinth://mod/sodium%2Fversions",
  ])("rejects %s", (link) => {
    expect(resolveDeepLink(link)).toEqual({ kind: "unsupported" });
  });

  it("keeps the ids the providers really use", () => {
    expect(resolveDeepLink("modrinth://mod/AANobbMI")).toMatchObject({
      modId: "AANobbMI",
    });
    expect(resolveDeepLink("modrinth://mod/fabric-api")).toMatchObject({
      modId: "fabric-api",
    });
    expect(
      resolveDeepLink("quadrantnext://modrinth?modId=sodium&fileId=u1sQ4GfL"),
    ).toMatchObject({ modId: "sodium", fileId: "u1sQ4GfL" });
  });

  it("treats an empty file id as none", () => {
    expect(
      resolveDeepLink("curseforge://install?addonId=1&fileId="),
    ).toMatchObject({ kind: "installMod", fileId: undefined });
  });
});
