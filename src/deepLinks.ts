/** @format */

import { ModSource } from "./intefaces";

export function getQuadrantPathParts(url: URL): string[] {
  return url.pathname.split("/").filter((part) => part.trim().length > 0);
}

export function getQuadrantAction(url: URL): string {
  const pathAction = getQuadrantPathParts(url)[0];
  return (url.host || pathAction || "").toLowerCase();
}

export function getQuadrantPathValue(url: URL): string | undefined {
  const pathParts = getQuadrantPathParts(url);
  return url.host ? pathParts[0] : pathParts[1];
}

export function getQuadrantModId(url: URL): string {
  return (
    url.searchParams.get("modId") ??
    url.searchParams.get("addonId") ??
    getQuadrantPathValue(url) ??
    ""
  ).trim();
}

export function getQuadrantCode(url: URL): string {
  return (
    url.searchParams.get("code") ??
    url.searchParams.get("sharedCode") ??
    getQuadrantPathValue(url) ??
    ""
  ).trim();
}

/** Parses a bare 7-digit Quadrant Share code or a usequadrant.dev/modpack/<code> URL. */
export function parseShareCode(input: string): string | null {
  const trimmed = input.trim();
  if (/^\d{7}$/.test(trimmed)) {
    return trimmed;
  }
  try {
    const url = new URL(trimmed);
    if (
      url.hostname === "usequadrant.dev" ||
      url.hostname === "www.usequadrant.dev"
    ) {
      const match = url.pathname.match(/^\/modpack\/(\d{7})\/?$/);
      if (match) {
        return match[1];
      }
    }
  } catch {
    // not a URL
  }
  return null;
}

export type DeepLinkAction =
  /**
   * Open the install page for a mod. `stopAfter` mirrors the historical
   * handler: most schemes stop processing further URLs after an install,
   * but the bare `curseforge:` scheme kept iterating.
   */
  | {
      kind: "installMod";
      source: ModSource;
      modId: string;
      fileId?: string;
      stopAfter: boolean;
    }
  /** OAuth redirect; the caller still validates `providedState`. */
  | {
      kind: "oauthLogin";
      providedState: string | null;
      code: string | null;
      redirectUri: string;
    }
  /** Open the shared-modpack import page. */
  | { kind: "importModpack"; code: string }
  /** Recognized scheme but unusable payload: show the error snackbar and stop. */
  | { kind: "unsupported" }
  /** Not a Quadrant link: skip silently and keep processing. */
  | { kind: "none" };

// Ids are spliced into provider API paths by the backend, so a link must not
// be able to smuggle a path segment or a query in through them.
const CURSEFORGE_ID = /^\d{1,12}$/;
const MODRINTH_PROJECT = /^[\w!@$()`.+,"\-']{1,64}$/;
const MODRINTH_VERSION = /^[A-Za-z0-9]{1,64}$/;

function installLink(
  source: ModSource,
  modId: string,
  rawFileId: string | null,
  stopAfter: boolean,
): DeepLinkAction {
  const fileId = rawFileId?.trim() || null;
  const curseForge = source === ModSource.CurseForge;
  const validMod = curseForge
    ? CURSEFORGE_ID.test(modId)
    : MODRINTH_PROJECT.test(modId) && !/^\.+$/.test(modId);
  const validFile =
    fileId === null ||
    (curseForge ? CURSEFORGE_ID : MODRINTH_VERSION).test(fileId);
  if (!validMod || !validFile) {
    return { kind: "unsupported" };
  }
  return {
    kind: "installMod",
    source,
    modId,
    fileId: fileId ?? undefined,
    stopAfter,
  };
}

/**
 * Classifies a deep-link URL into the action the app should take.
 * Throws if `rawUrl` is not a valid URL (matching `new URL`).
 */
export function resolveDeepLink(rawUrl: string): DeepLinkAction {
  const url = new URL(rawUrl);

  switch (url.protocol) {
    case "curseforge:": {
      const action = (url.host || getQuadrantPathParts(url)[0] || "")
        .toLowerCase();
      const modId = (url.searchParams.get("addonId") ?? "").trim();
      if (action !== "install" || !modId) {
        return { kind: "unsupported" };
      }
      return installLink(
        ModSource.CurseForge,
        modId,
        url.searchParams.get("fileId"),
        false,
      );
    }

    case "modrinth:": {
      const parts = [url.host, ...getQuadrantPathParts(url)].filter(Boolean);
      // Preserve links wrapping a website URL, such as
      // modrinth://https://modrinth.com/mod/sodium.
      if (parts[0]?.toLowerCase() === "https") {
        parts.shift();
      }
      if (parts[0]?.toLowerCase() === "modrinth.com") {
        parts.shift();
      }
      const [action, modId] = parts;
      if (
        !["mod", "resourcepack", "shader"].includes(action?.toLowerCase()) ||
        !modId
      ) {
        return { kind: "unsupported" };
      }
      return installLink(ModSource.Modrinth, modId, null, true);
    }

    case "quadrantnext:": {
      const actions = url.pathname.split("/");
      const quadrantAction = getQuadrantAction(url);

      if (actions.includes("login") || quadrantAction === "login") {
        return {
          kind: "oauthLogin",
          providedState: url.searchParams.get("state"),
          code: url.searchParams.get("code"),
          redirectUri: rawUrl.split("#")[0].split("?")[0],
        };
      }

      if (quadrantAction === "modrinth" || quadrantAction === "curseforge") {
        const modId = getQuadrantModId(url);
        if (!modId) {
          return { kind: "unsupported" };
        }
        return installLink(
          quadrantAction === "modrinth"
            ? ModSource.Modrinth
            : ModSource.CurseForge,
          modId,
          url.searchParams.get("fileId"),
          true,
        );
      }

      if (quadrantAction === "modpack") {
        const code = getQuadrantCode(url);
        if (!code) {
          return { kind: "unsupported" };
        }
        return { kind: "importModpack", code };
      }

      return { kind: "none" };
    }

    case "https:": {
      const code = parseShareCode(rawUrl);
      return code ? { kind: "importModpack", code } : { kind: "none" };
    }

    default:
      return { kind: "none" };
  }
}
