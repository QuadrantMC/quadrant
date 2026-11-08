/** @format */

import { useEffect, useRef, useState } from "react";
import { AccountInfo, SyncedModpack } from "../../../intefaces";
import { getAccountInfo, getSyncedModpacks } from "../../../tools";
import { listen } from "../../../desktop";

export interface SyncedModpacksState {
  /** Null while signed out or when the account has no Quadrant Sync quota. */
  accountInfo: AccountInfo | null;
  syncedModpacks: SyncedModpack[];
  /** Re-reads one modpack, or the whole list when given no id. */
  refresh: (modpackId?: string) => Promise<void>;
}

/**
 * Folds a single-modpack fetch into the list. An empty result means the pack
 * is no longer ours in the cloud: deleted, or this account was kicked or left.
 */
function withRefreshedModpack(
  current: SyncedModpack[],
  modpackId: string,
  fetched: SyncedModpack[],
): SyncedModpack[] {
  const updated = fetched.find((modpack) => modpack.modpack_id === modpackId);
  if (updated === undefined) {
    return current.filter((modpack) => modpack.modpack_id !== modpackId);
  }
  if (!current.some((modpack) => modpack.modpack_id === modpackId)) {
    return [...current, updated];
  }
  return current.map((modpack) =>
    modpack.modpack_id === modpackId ? updated : modpack,
  );
}

/**
 * Whether the pack's mod list parses. The list is cloud-side data any
 * co-owner can write, and the cards parse it while rendering, so one bad pack
 * would otherwise take the whole window down.
 */
function hasReadableMods(modpack: SyncedModpack): boolean {
  try {
    return Array.isArray(JSON.parse(modpack.mods));
  } catch {
    console.error(`Ignoring synced modpack ${modpack.modpack_id}: bad mods`);
    return false;
  }
}

/**
 * Loads the signed-in account and keeps the Quadrant Sync modpack list fresh,
 * re-fetching whenever the backend raises `refreshSyncedModpacks` and
 * re-reading the account on `recheckAccountToken` (sign-in, sign-out, plan
 * changes). Signed-out users get an empty list and a null account.
 */
export function useSyncedModpacks(): SyncedModpacksState {
  const [accountInfo, setAccountInfo] = useState<AccountInfo | null>(null);
  const [syncedModpacks, setSyncedModpacks] = useState<SyncedModpack[]>([]);

  // Full reads are ordered by `requestRef`; a targeted read is ordered only
  // against reads of the same pack, and is dropped by any later full read.
  const requestRef = useRef(0);
  const pendingFullRef = useRef(0);
  const targetedRef = useRef(new Map<string, number>());
  const refresh = async (modpackId?: string) => {
    // A full read already in flight may predate this pack's change, and
    // patching around it would lose either the list or the change.
    const targetId = pendingFullRef.current > 0 ? undefined : modpackId;
    const request = targetId ? requestRef.current : ++requestRef.current;
    const targeted = targetId
      ? (targetedRef.current.get(targetId) ?? 0) + 1
      : 0;
    if (targetId) {
      targetedRef.current.set(targetId, targeted);
    } else {
      pendingFullRef.current++;
    }
    try {
      const modpacks = (await getSyncedModpacks(true, targetId)).filter(
        hasReadableMods,
      );
      if (
        request !== requestRef.current ||
        (targetId && targetedRef.current.get(targetId) !== targeted)
      ) {
        return;
      }
      setSyncedModpacks((current) =>
        targetId ? withRefreshedModpack(current, targetId, modpacks) : modpacks,
      );
    } catch (error) {
      console.error(error);
    } finally {
      if (!targetId) {
        pendingFullRef.current--;
      }
    }
  };

  useEffect(() => {
    let isUnmounted = false;
    const cleanupFns: Array<() => void> = [];

    // The latest account read wins; an older answer must not undo it.
    let accountRequest = 0;
    let signedIn = false;
    const loadAccount = async () => {
      const request = ++accountRequest;
      let info: AccountInfo | null = null;
      try {
        info = await getAccountInfo();
      } catch (error) {
        // Throws when signed out, which must leave the list empty.
        console.error(error);
      }
      if (isUnmounted || request !== accountRequest) {
        return;
      }
      if (info === null || info.quadrant_sync_limit === 0) {
        signedIn = false;
        requestRef.current++;
        setAccountInfo(null);
        setSyncedModpacks([]);
        return;
      }
      signedIn = true;
      setAccountInfo(info);
      // A transient fetch failure must not prevent subscribing to recovery events.
      void refresh();
    };

    const subscribe = async (
      event: string,
      handler: (payload: unknown) => void,
    ) => {
      const unlisten = await listen<unknown>(event, (e) => {
        if (!isUnmounted) {
          handler(e.payload);
        }
      });
      if (isUnmounted) {
        unlisten();
      } else {
        cleanupFns.push(unlisten);
      }
    };

    const effect = async () => {
      await loadAccount();
      await subscribe("refreshSyncedModpacks", (payload) => {
        if (signedIn) {
          // The host sends an empty payload when it cannot name the pack.
          void refresh((payload as string | null) || undefined);
        }
      });
      await subscribe("recheckAccountToken", () => void loadAccount());
    };

    effect().catch(console.error);

    return () => {
      isUnmounted = true;
      requestRef.current++;
      while (cleanupFns.length > 0) {
        const cleanup = cleanupFns.pop();
        try {
          cleanup?.();
        } catch (error) {
          console.error(error);
        }
      }
    };
  }, []);

  return { accountInfo, syncedModpacks, refresh };
}
