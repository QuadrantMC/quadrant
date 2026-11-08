/** @format */

import { LocalMod, ModSource } from "./intefaces";

export interface ReviewedMod {
  name: string;
  /** Host the file downloads from; empty when the link doesn't parse. */
  host: string;
}

export interface ModpackReview {
  /** Mods that download from an arbitrary link, not a provider's CDN. */
  online: ReviewedMod[];
  /** Mods Quadrant has no file hash for, so it can't verify their files. */
  unverified: ReviewedMod[];
}

function reviewed(mod: LocalMod): ReviewedMod {
  let host = "";
  try {
    host = new URL(mod.downloadUrl).host;
  } catch {
    // A malformed link is shown without a host.
  }
  return { name: mod.name || mod.slug || mod.id, host };
}

/**
 * Sorts out the mods of a pack that deserve a warning before it is installed.
 * An Online mod without a hash is in both lists. The same rule as
 * `quadrant_core::modpacks::review_incoming_mods` with nothing installed yet.
 */
export function reviewMods(mods: LocalMod[]): ModpackReview {
  return {
    online: mods.filter((mod) => mod.source === ModSource.Online).map(reviewed),
    unverified: mods.filter((mod) => !mod.sha512).map(reviewed),
  };
}

export function hasWarnings(review: ModpackReview): boolean {
  return review.online.length > 0 || review.unverified.length > 0;
}
