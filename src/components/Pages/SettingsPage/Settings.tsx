/** @format */

import { useTranslation } from "react-i18next";
import Button from "../../core/Button";
import {
  getDefaultMinecraftFolder,
  getMinecraftFolder,
  openIn,
  requestCheckForUpdates,
} from "../../../tools";
import { Field, Label, Select, Switch } from "@headlessui/react";
import quadrantLocale from "../../../i18n";
import { useEffect, useRef, useState } from "react";
import "./SettingsPage.css";
import { motion } from "motion/react";
import {
  createDesktopStore,
  getAppVersion,
  getRuntimeName,
  getRuntimeVersion,
  invoke,
  isAutoupdateEnabled,
  openDialog,
} from "../../../desktop";
import { MdAdd, MdRemove } from "react-icons/md";
import {
  clampUiScale,
  COMPACT_UI_SCALE,
  setUiScale,
  UI_SCALE_KEY,
  UI_SCALE_STEP,
} from "../../../uiScale";
import { useReportError } from "../../../useReportError";

/**
 * The `channel` setting's values with their labels. Quadrant-Native shares
 * the setting, and a GPUI channel switches to it (quadrant-host's `Channel`).
 */
const UPDATE_CHANNELS = [
  ["stable", "stable"],
  ["preview", "preview"],
  ["gpui-preview", "gpuiPreview"],
  ["gpui-experimental", "gpuiExperimental"],
] as const;

export default function SettingsPage() {
  const { t } = useTranslation();
  const reportError = useReportError();
  const boxRef = useRef(createDesktopStore("config.json"));
  const box = boxRef.current;

  const [currentLocale, setCurrentLocale] = useState("en");
  const [updateChannel, setUpdateChannel] = useState("stable");
  const [clipButtons, setClipButtons] = useState(false);
  const [collectData, setCollectData] = useState(false);
  const [curseForge, setCurseForge] = useState(false);
  const [modrinth, setModrinth] = useState(false);
  const [devMode, setDevMode] = useState(false);
  // const [rssFeeds, setRssFeeds] = useState(false);
  // const [silentNews, setSilentNews] = useState(false);
  const [autoQuadrantSync, setAutoQuadrantSync] = useState(false);
  const [showModpackUpdateNotifications, setShowModpackUpdateNotifications] =
    useState(true);
  const [showUnupgradeableMods, setShowUnupgradeableMods] = useState(false);
  const [experimentalFeatures, setExperimentalFeatures] = useState(false);
  const [syncSettings, setSyncSettings] = useState(false);
  const [mcFolder, setMcFolder] = useState("");
  // null means no override is stored, so the backend detects the folder itself.
  const [prismFolder, setPrismFolder] = useState<string | null>(null);
  const [currentVersion, setCurrentVersion] = useState("");
  const [currentRuntimeName, setCurrentRuntimeName] = useState("");
  const [currentRuntimeVersion, setCurrentRuntimeVersion] = useState("");
  const [nativeDecorations, setNativeDecorations] = useState(false);
  const [showUpdateSettings, setShowUpdateSettings] = useState(true);
  const [uiScale, setUiScaleState] = useState(COMPACT_UI_SCALE);

  const changeUiScale = async (newScale: number) => {
    const applied = await setUiScale(newScale);
    setUiScaleState(applied);
  };

  const applyCollectDataPreference = async (enabled: boolean) => {
    setCollectData(enabled);
    await box.set("collectUserData", enabled);
    await box.save();

    try {
      await invoke(enabled ? "send_telemetry" : "remove_telemetry");
    } catch (error) {
      reportError(error);
    }
  };

  useEffect(() => {
    const initializeValues = async () => {
      const actualCurrentLocale = (await box.get<string>("locale")) ?? "en";
      setCurrentLocale(actualCurrentLocale);
      setClipButtons((await box.get<boolean>("clipIcons")) ?? true);
      setCollectData((await box.get<boolean>("collectUserData")) ?? false);
      setCurseForge((await box.get<boolean>("curseforge")) ?? false);
      setModrinth((await box.get<boolean>("modrinth")) ?? false);
      setDevMode((await box.get<boolean>("devMode")) ?? false);
      // setRssFeeds((await box.get<boolean>("rssFeeds")) ?? false);
      // setSilentNews((await box.get<boolean>("silentNews")) ?? false);
      setUpdateChannel((await box.get<string>("channel")) ?? "stable");
      setAutoQuadrantSync(
        (await box.get<boolean>("autoQuadrantSync")) ?? false,
      );
      setShowModpackUpdateNotifications(
        (await box.get<boolean>("showModpackUpdateNotifications")) ?? true,
      );

      setShowUnupgradeableMods(
        (await box.get("showUnupgradeableMods")) ?? false,
      );
      setExperimentalFeatures((await box.get("experimentalFeatures")) || false);
      setPrismFolder((await box.get<string>("prismLauncherFolder")) ?? null);
      console.log("Minecraft folder: " + (await getMinecraftFolder(false)));
      setMcFolder(await getMinecraftFolder(false));
      setSyncSettings((await box.get("syncSettings")) || false);
      setCurrentVersion(await getAppVersion());
      setCurrentRuntimeName(await getRuntimeName());
      setCurrentRuntimeVersion(await getRuntimeVersion());
      setNativeDecorations(
        (await box.get<boolean>("nativeDecorations")) ?? false,
      );
      setShowUpdateSettings(await isAutoupdateEnabled());
      setUiScaleState(clampUiScale(await box.get<number>(UI_SCALE_KEY)));
    };

    initializeValues();

    // Keeps the displayed percentage in sync with the zoom shortcuts.
    let isUnmounted = false;
    let uiScaleUnlisten: (() => void) | null = null;
    const subscribe = async () => {
      uiScaleUnlisten = await box.onKeyChange<number>(
        UI_SCALE_KEY,
        (newValue) => {
          if (!isUnmounted) {
            setUiScaleState(clampUiScale(newValue));
          }
        },
      );
      if (isUnmounted) {
        uiScaleUnlisten();
        uiScaleUnlisten = null;
      }
    };
    subscribe().catch(console.error);

    return () => {
      isUnmounted = true;
      uiScaleUnlisten?.();
    };
  }, [box]);

  return (
    <motion.div
      initial={{ y: 24, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      exit={{ y: 24, opacity: 0 }}
      className="mt-2 mx-8 flex flex-1 flex-col transform-gpu [backface-visibility:hidden] [will-change:transform,opacity]"
    >
      <h1 className="font-extrabold text-2xl my-4 bg-slate-800 rounded-4xl w-fit p-3">
        {t("someSettingsRequireReload")}
      </h1>
      <div className="flex flex-col items-center align-middle w-full p-4 bg-slate-800 rounded-4xl">
        <p className="font-extrabold my-2 bg-slate-700 rounded-4xl p-4">
          {t("currentVersion", {
            version: currentVersion,
            runtimeName: currentRuntimeName,
            runtimeVersion: currentRuntimeVersion,
          })}
        </p>
      </div>
      {showUpdateSettings ? (
        <Field className={"flex flex-col font-bold my-4"}>
          <Label>{t("updateChannel")}</Label>
          <Select
            className="my-4 input w-full max-w-xs"
            aria-label={t("updateChannel")}
            value={updateChannel}
            onChange={async (e) => {
              e.preventDefault();
              e.preventDefault();
              const newChannel = e.target.value;
              await box.set("channel", newChannel);
              setUpdateChannel(newChannel);
              await box.save();
              requestCheckForUpdates();
            }}
          >
            {UPDATE_CHANNELS.map(([value, label]) => (
              <option key={value} value={value}>
                {t(label)}
              </option>
            ))}
          </Select>
        </Field>
      ) : (
        <></>
      )}
      <Field className={"flex flex-col font-bold my-4"}>
        <Label>{t("language")}</Label>
        <Select
          className="input my-4 w-full max-w-xs"
          aria-label={t("language")}
          value={currentLocale}
          onChange={async (e) => {
            const newLocale = e.target.value;
            await quadrantLocale.changeLanguage(newLocale);
            await box.set("locale", newLocale);
            setCurrentLocale(newLocale);
            await box.save();
          }}
        >
          <option value="en">English</option>
          <option value="uk">Українська</option>
          <option value="tr">Türkçe</option>
        </Select>
      </Field>
      <Field className={"flex flex-col font-bold my-4"}>
        <Label>{t("uiScale")}</Label>
        <div className="flex flex-row flex-wrap items-center my-4 gap-2">
          <Button
            className="bg-slate-700 hover:bg-slate-600 flex items-center justify-center"
            onClick={async () => {
              await changeUiScale(uiScale - UI_SCALE_STEP);
            }}
          >
            <MdRemove className="w-5 h-5" />
          </Button>
          <span className="bg-slate-800 rounded-4xl px-4 py-2 w-20 text-center">
            {uiScale}%
          </span>
          <Button
            className="bg-slate-700 hover:bg-slate-600 flex items-center justify-center"
            onClick={async () => {
              await changeUiScale(uiScale + UI_SCALE_STEP);
            }}
          >
            <MdAdd className="w-5 h-5" />
          </Button>
        </div>
      </Field>
      <div className="flex flex-col items-center align-middle w-full p-4 bg-slate-800 rounded-4xl">
        <p className="font-extrabold my-2 bg-slate-700 rounded-4xl p-4">
          {mcFolder}
        </p>
        <div className="flex flex-row w-full">
          <Button
            className="bg-slate-700 w-full hover:bg-slate-600 mr-4"
            onClick={async () => {
              const newFolder = await getDefaultMinecraftFolder();
              console.log("New Minecraft folder: " + newFolder);
              setMcFolder(newFolder);
              await box.set("mcFolder", newFolder);
              await box.save();
            }}
          >
            {t("resetMinecraftFolder")}
          </Button>
          <Button
            className="bg-slate-700 hover:bg-slate-600 w-full ml-4"
            onClick={async () => {
              const newFolder = await openDialog({
                multiple: false,
                directory: true,
                recursive: true,
                title: t("overrideMinecraftFolder"),
              });
              if (typeof newFolder !== "string" || newFolder.length === 0) {
                return;
              }
              console.log("New Minecraft folder: " + newFolder);
              setMcFolder(newFolder);
              await box.set("mcFolder", newFolder);
              await box.save();
            }}
          >
            {t("overrideMinecraftFolder")}
          </Button>
        </div>
      </div>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={collectData}
          onChange={async () => {
            await applyCollectDataPreference(!collectData);
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("collectData")}</Label>
      </Field>
      <Button
        className="bg-slate-800 hover:bg-slate-700 w-fit my-4"
        onClick={async () => {
          await applyCollectDataPreference(true);
        }}
      >
        {t("collectData")}
      </Button>
      <Button
        className="bg-slate-800 hover:bg-slate-700 w-fit my-4"
        onClick={async () => {
          await openIn("https://mrquantumoff.dev/projects/quadrant/analytics");
        }}
      >
        {t("viewPublicUsage", { productName: t("productName") })}
      </Button>
      <Button
        className="bg-slate-800 hover:bg-slate-700 w-fit my-4"
        onClick={async () => {
          await openIn(
            "https://mrquantumoff.dev/projects/quadrant/analytics/" +
              (await box.get<string>("hardwareId")),
          );
        }}
      >
        {t("viewYourUsageData")}
      </Button>
      <Button
        className="bg-slate-800 hover:bg-red-700 w-fit my-4"
        onClick={async () => {
          await applyCollectDataPreference(false);
        }}
      >
        {t("deleteYourUsageData")}
      </Button>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={clipButtons}
          onChange={async (newValue) => {
            setClipButtons(newValue);
            await box.set("clipIcons", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("clipIcons")}</Label>
      </Field>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={curseForge}
          onChange={async (newValue) => {
            setCurseForge(newValue);
            await box.set("curseforge", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{"CurseForge"}</Label>
      </Field>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={modrinth}
          onChange={async (newValue) => {
            setModrinth(newValue);
            await box.set("modrinth", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{"Modrinth"}</Label>
      </Field>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={nativeDecorations}
          onChange={async (newValue) => {
            setNativeDecorations(newValue);
            await box.set("nativeDecorations", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("nativeDecorations")}</Label>
      </Field>
      {/* <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={rssFeeds}
          onChange={async (newValue) => {
            setRssFeeds(newValue);
            await box.set("rssFeeds", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("rssFeeds")}</Label>
      </Field>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={silentNews}
          onChange={async (newValue) => {
            setSilentNews(newValue);
            await box.set("silentNews", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("silentNews")}</Label>
      </Field> */}
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={autoQuadrantSync}
          onChange={async (newValue) => {
            setAutoQuadrantSync(newValue);
            await box.set("autoQuadrantSync", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("autoQuadrantSync")}</Label>
      </Field>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={showModpackUpdateNotifications}
          onChange={async (newValue) => {
            setShowModpackUpdateNotifications(newValue);
            await box.set("showModpackUpdateNotifications", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("showModpackUpdateNotifications")}</Label>
      </Field>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={syncSettings}
          onChange={async (newValue) => {
            setSyncSettings(newValue);
            await box.set("syncSettings", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("quadrantSettingsSync")}</Label>
      </Field>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={showUnupgradeableMods}
          onChange={async (newValue) => {
            setShowUnupgradeableMods(newValue);
            await box.set("showUnupgradeableMods", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("showUnupgradeableMods")}</Label>
      </Field>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={experimentalFeatures}
          onChange={async (newValue) => {
            setExperimentalFeatures(newValue);
            await box.set("experimentalFeatures", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("experimentalFeatures")}</Label>
      </Field>
      {experimentalFeatures && (
        <div className="flex flex-row flex-wrap items-center gap-4 w-full p-4 my-4 bg-slate-800 rounded-4xl">
          <span className="font-extrabold">{t("prismLauncherFolder")}</span>
          <span className="grow min-w-0 break-all bg-slate-700 rounded-4xl px-4 py-2">
            {prismFolder ?? t("prismLauncherFolderAuto")}
          </span>
          <Button
            className="bg-slate-700 hover:bg-slate-600"
            onClick={async () => {
              const newFolder = await openDialog({
                multiple: false,
                directory: true,
                recursive: true,
                title: t("prismLauncherFolder"),
              });
              if (typeof newFolder !== "string" || newFolder.length === 0) {
                return;
              }
              try {
                await box.set("prismLauncherFolder", newFolder);
                await box.save();
                setPrismFolder(newFolder);
              } catch (e) {
                reportError(e);
              }
            }}
          >
            {t("choosePrismLauncherFolder")}
          </Button>
          <Button
            className="bg-slate-700 hover:bg-slate-600"
            onClick={async () => {
              try {
                await box.delete("prismLauncherFolder");
                await box.save();
                setPrismFolder(null);
              } catch (e) {
                reportError(e);
              }
            }}
          >
            {t("resetPrismLauncherFolder")}
          </Button>
        </div>
      )}
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
          }
          checked={devMode}
          onChange={async (newValue) => {
            setDevMode(newValue);
            await box.set("devMode", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("devMode")}</Label>
      </Field>
    </motion.div>
  );
}
