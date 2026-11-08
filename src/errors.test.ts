/** @format */

import { describe, expect, it } from "vitest";
import {
  describeError,
  isCloudSyncNewerError,
  isModpackMissingError,
} from "./errors";
import quadrantLocale from "./i18n";
import en from "./locales/en.json";

const t = quadrantLocale.t;

function describeRaw(raw: unknown): string {
  return describeError(raw, t);
}

describe("describeError normalization", () => {
  it("takes a string error as-is", () => {
    expect(describeRaw("thirdPartyDownloadDisabled")).toBe(
      en.thirdPartyDownloadDisabled,
    );
  });

  it("reads the message of an Error", () => {
    expect(describeRaw(new Error("thirdPartyDownloadDisabled"))).toBe(
      en.thirdPartyDownloadDisabled,
    );
  });

  it("reads a string message off a plain object", () => {
    expect(describeRaw({ message: "noVersion" })).toBe(en.noVersion);
  });

  it("stringifies anything else", () => {
    expect(describeRaw(42)).toBe(
      en.errorUnknown.replace("{{details}}", "42"),
    );
  });

  it("ignores surrounding whitespace", () => {
    expect(describeRaw("  noVersion  ")).toBe(en.noVersion);
  });

  it("falls back without details when there is nothing to show", () => {
    expect(describeRaw("   ")).toBe(en.errorUnknownNoDetails);
  });

  it("does not treat a non-string message as the message", () => {
    expect(describeRaw({ message: 7 })).toBe(
      en.errorUnknown.replace("{{details}}", "[object Object]"),
    );
  });
});

describe("describeError keys and fallback", () => {
  it.each([
    "thirdPartyDownloadDisabled",
    "noVersion",
    "unsupportedDownload",
    "modpackRequired",
    "enableDataSharing",
    "errorNetwork",
    "errorModpackMissing",
  ])("translates the backend key %s", (key) => {
    expect(describeRaw(key)).toBe(en[key as keyof typeof en]);
  });

  it("does not treat an unknown single word as a key", () => {
    expect(describeRaw("kaboom")).toBe(
      en.errorUnknown.replace("{{details}}", "kaboom"),
    );
  });

  it("keeps the raw text of an English sentence", () => {
    expect(describeRaw("The flux capacitor overheated")).toBe(
      en.errorUnknown.replace("{{details}}", "The flux capacitor overheated"),
    );
  });
});

describe("isCloudSyncNewerError", () => {
  it.each(["errorCloudSyncNewer", "  errorCloudSyncNewer  "])(
    "recognizes %s",
    (raw) => {
      expect(isCloudSyncNewerError(raw)).toBe(true);
      expect(isCloudSyncNewerError(new Error(raw))).toBe(true);
    },
  );

  it.each([
    "errorNetwork",
    "errorModpackMissing",
    "Current settings are newer",
    // The backend classifies the server's sentence before it gets here.
    "Cloud sync is newer",
    "Upload refused: cloud sync is newer than allowed",
    // What the browser runtime throws when there is no desktop host at all.
    "Quadrant desktop runtime is unavailable in this environment. Tried to use: invoke",
    "",
    null,
    undefined,
    42,
  ])("does not mistake %o for the conflict", (raw) => {
    expect(isCloudSyncNewerError(raw)).toBe(false);
  });
});

describe("isModpackMissingError", () => {
  it("recognizes the backend's missing-modpack code only", () => {
    expect(isModpackMissingError("errorModpackMissing")).toBe(true);
    expect(isModpackMissingError(new Error("errorModpackMissing"))).toBe(true);
    expect(isModpackMissingError("errorFileAccess")).toBe(false);
    expect(isModpackMissingError("the errorModpackMissing happened")).toBe(
      false,
    );
  });
});
