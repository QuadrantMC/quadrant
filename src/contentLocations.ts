/** @format */

import type { TFunction } from "i18next";
import { ContentLocation, ModType } from "./intefaces";

/** A kind of content the user can put in a folder of their own choosing. */
export type PackType = ModType.ResourcePack | ModType.ShaderPack;

/** What each kind of pack is called, and by omission which kinds are packs. */
const SECTION_TITLE_KEYS: Record<PackType, string> = {
  [ModType.ResourcePack]: "contentResourcePacks",
  [ModType.ShaderPack]: "contentShaders",
};

/** Whether this kind of content can be routed to a location of its own. */
export function isPackType(modType: ModType): modType is PackType {
  return modType in SECTION_TITLE_KEYS;
}

/**
 * Whether the install flow can install this kind of project. Modpacks and
 * data packs can be browsed, but their download is not supported.
 */
export function isInstallable(modType: ModType): boolean {
  return (
    modType !== ModType.Unknown &&
    modType !== ModType.Modpack &&
    modType !== ModType.DataPack
  );
}

/** What the user calls one section: its kind of pack, in their language. */
export function sectionTitle(modType: ModType, t: TFunction): string {
  return isPackType(modType) ? t(SECTION_TITLE_KEYS[modType]) : modType;
}

/** What the user calls a location: the Minecraft folder, or the instance name. */
export function locationTitle(location: ContentLocation, t: TFunction): string {
  return location.kind === "minecraft"
    ? t("installedContentMinecraft")
    : location.name;
}

/** How a location reads in a picker, saying which ones Prism Launcher owns. */
export function locationOptionLabel(
  location: ContentLocation,
  t: TFunction,
): string {
  const title = locationTitle(location, t);
  return location.kind === "prism"
    ? t("installedContentPrismOption", { name: title })
    : title;
}
