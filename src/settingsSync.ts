/** @format */

/**
 * Settings a user edits that the cloud settings sync carries to other
 * devices. Mirrors `SYNCED_KEYS` in quadrant-core, minus `lastPage` and
 * `lastPageName`: those track navigation, which is no edit worth pushing.
 */
const USER_SYNCED_SETTINGS: ReadonlySet<string> = new Set([
  "collectUserData",
  "modrinth",
  "curseforge",
  "rssFeeds",
  "silentNews",
  "autoQuadrantSync",
  "showUnupgradeableMods",
  "uiScale",
  "experimentalFeatures",
  "cacheKeepAlive",
  "clipIcons",
  "colorTheme",
]);

/**
 * Whether a renderer write to this config key must advance the settings
 * clock. Only the renderer's own writes reach this check; values the backend
 * applies from the cloud never pass through it, so a pull cannot make the
 * local copy look newer than the cloud one.
 */
export function bumpsSettingsClock(key: string): boolean {
  return USER_SYNCED_SETTINGS.has(key);
}
