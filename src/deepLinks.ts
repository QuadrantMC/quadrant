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
      if (action !== "install") {
        return { kind: "unsupported" };
      }
      return {
        kind: "installMod",
        source: ModSource.CurseForge,
        modId: url.searchParams.get("addonId") ?? "",
        fileId: url.searchParams.get("fileId") ?? undefined,
        stopAfter: false,
      };
    }

    case "modrinth:": {
      const action = url.pathname.split("/")[2] ?? url.host ?? "";
      if (
        action.includes("mod") ||
        action.includes("resourcepack") ||
        action.includes("shader")
      ) {
        // This gets the slug, not the ID, but it doesn't matter for Modrinth
        let modId = "";
        url.pathname.split("/").forEach((val) => {
          if (val !== "") {
            modId = val;
          }
        });
        return {
          kind: "installMod",
          source: ModSource.Modrinth,
          modId,
          stopAfter: true,
        };
      }
      return { kind: "unsupported" };
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
        return {
          kind: "installMod",
          source:
            quadrantAction === "modrinth"
              ? ModSource.Modrinth
              : ModSource.CurseForge,
          modId,
          fileId: url.searchParams.get("fileId") ?? undefined,
          stopAfter: true,
        };
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
      const host = url.host.toLowerCase();
      if (host === "usequadrant.dev" || host === "www.usequadrant.dev") {
        const pathParts = getQuadrantPathParts(url);
        if (pathParts[0] === "modpack") {
          const code = pathParts[1] ?? "";
          if (code && /^\d{7}$/.test(code)) {
            return { kind: "importModpack", code };
          }
        }
      }
      return { kind: "none" };
    }

    default:
      return { kind: "none" };
  }
}
