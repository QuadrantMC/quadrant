/** @format */

import { joinPath, watch } from "./desktop";

/**
 * Calls `onChange` whenever a modpack, or which pack is applied, may have
 * changed on disk. Resolves to a function that stops watching.
 *
 * The Minecraft folder itself is watched flat because the `mods` link flips on
 * apply, and the `modpacks` tree is watched recursively because installs and
 * sync metadata land several levels deep. Watching `mods` directly would
 * throw until a pack is first applied and would follow the old link target
 * after another pack is applied from the tray or the CLI.
 */
export async function watchModpacks(
  minecraftFolder: string,
  onChange: () => void,
): Promise<() => void> {
  let disposed = false;
  let treeAttached = false;
  const unwatchers: Array<() => void> = [];

  const keep = (unwatch: () => void) => {
    if (disposed) unwatch();
    else unwatchers.push(unwatch);
  };

  const watchTree = async () => {
    if (treeAttached || disposed) return;
    treeAttached = true;
    try {
      keep(
        await watch(
          await joinPath(minecraftFolder, "modpacks"),
          () => {
            if (!disposed) onChange();
          },
          { delayMs: 50, recursive: true },
        ),
      );
    } catch (error) {
      // The tree may not exist until the first local pack is created.
      treeAttached = false;
      console.error(error);
    }
  };

  try {
    keep(
      await watch(
        await joinPath(minecraftFolder),
        async () => {
          if (disposed) return;
          await watchTree();
          if (!disposed) onChange();
        },
        { delayMs: 50 },
      ),
    );
  } catch (error) {
    console.error(error);
  }
  await watchTree();

  return () => {
    disposed = true;
    for (const unwatch of unwatchers.splice(0)) {
      try {
        unwatch();
      } catch (error) {
        console.error(error);
      }
    }
  };
}
