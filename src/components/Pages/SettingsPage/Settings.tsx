/** @format */

import { useTranslation } from "react-i18next";
import Button from "../../core/Button";
import { LazyStore } from "@tauri-apps/plugin-store";
import {
  getMinecraftFolder,
  openIn,
  requestCheckForUpdates,
} from "../../../tools";
import { Field, Label, Select, Switch } from "@headlessui/react";
import quadrantLocale from "../../../i18n";
import { useEffect, useState } from "react";
import "./SettingsPage.css";
import { open } from "@tauri-apps/plugin-dialog";
import { getTauriVersion, getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { motion } from "motion/react";

export default function SettingsPage() {
  const { t } = useTranslation();
  const box = new LazyStore("config.json");
  const updateChannelBox = new LazyStore("updateConfig.json");

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
  const [currentVersion, setCurrentVersion] = useState("");
  const [currentTauriVersion, setCurrentTauriVersion] = useState("");
  const [extendedNavigation, setExtendedNavigation] = useState(false);
  const [showUpdateSettings, setShowUpdateSettings] = useState(true);

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
      setUpdateChannel(
        (await updateChannelBox.get<string>("channel")) ?? "stable",
      );
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
      console.log("Minecraft folder: " + (await getMinecraftFolder(false)));
      setMcFolder(await getMinecraftFolder(false));
      setSyncSettings((await box.get("syncSettings")) || false);
      setCurrentVersion(await getVersion());
      setCurrentTauriVersion(await getTauriVersion());
      setExtendedNavigation(
        (await box.get<boolean>("extendedNavigation")) ?? false,
      );
      setShowUpdateSettings(await invoke("is_autoupdate_enabled"));
    };

    initializeValues();
  }, []);

  return (
    <motion.div
      initial={{ y: 24, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      exit={{ y: 24, opacity: 0 }}
      className="mt-2 mx-8 flex flex-1 flex-col transform-gpu [backface-visibility:hidden] [will-change:transform,opacity]"
    >
      <h1 className="font-extrabold text-4xl my-4 bg-yellow-400 text-slate-950 rounded-4xl w-fit p-4">
        {t("someSettingsRequireReload")}
      </h1>
      <div className="flex flex-col items-center align-middle w-full p-4 bg-slate-700 rounded-4xl">
        <p className="font-extrabold my-2 bg-slate-900 rounded-4xl p-4">
          {t("currentVersion", {
            version: currentVersion,
            tauriVersion: currentTauriVersion,
          })}
        </p>
      </div>
      {showUpdateSettings ? (
        <Field className={"flex flex-col font-bold my-4"}>
          <Label>{t("updateChannel")}</Label>
          <Select
            className="my-4 input w-1/4"
            aria-label={t("locale")}
            value={updateChannel}
            onChange={async (e) => {
              e.preventDefault();
              e.preventDefault();
              const newChannel = e.target.value;
              await updateChannelBox.set("channel", newChannel);
              setUpdateChannel(newChannel);
              await updateChannelBox.save();
              requestCheckForUpdates();
            }}
          >
            <option value="stable">{t("stable")}</option>
            <option value="preview">{t("preview")}</option>
          </Select>
        </Field>
      ) : (
        <></>
      )}
      <Field className={"flex flex-col font-bold my-4"}>
        <Label>{t("language")}</Label>
        <Select
          className="input my-4 w-1/4"
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
      <div className="flex flex-col items-center align-middle w-full p-4 bg-slate-700 rounded-4xl">
        <p className="font-extrabold my-2 bg-slate-900 rounded-4xl p-4">
          {mcFolder}
        </p>
        <div className="flex flex-row w-full">
          <Button
            className="bg-slate-800 w-full hover:bg-slate-900 mr-4"
            onClick={async () => {
              const newFolder = await getMinecraftFolder(true);
              console.log("New Minecraft folder: " + newFolder);
              setMcFolder(newFolder);
              await box.set("mcFolder", newFolder);
            }}
          >
            {t("resetMinecraftFolder")}
          </Button>
          <Button
            className="bg-slate-800 hover:bg-slate-900 w-full ml-4"
            onClick={async () => {
              const newFolder = await open({
                multiple: false,
                directory: true,
                recursive: true,
                title: t("overrideMinecraftFolder"),
              });
              if (newFolder === null) {
                return;
              }
              console.log("New Minecraft folder: " + newFolder);
              setMcFolder(newFolder);
              await box.set("mcFolder", newFolder);
            }}
          >
            {t("overrideMinecraftFolder")}
          </Button>
        </div>
      </div>
      <Field className="flex items-center font-bold my-4">
        <Switch
          className={
            "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600"
          }
          checked={collectData}
          onChange={async () => {
            if (collectData) {
              await invoke("remove_telemetry");
              setCollectData(false);
              await box.set("collectUserData", false);
              await box.save();
            } else {
              await invoke("send_telemetry");
              setCollectData(true);
              await box.set("collectUserData", true);
              await box.save();
            }
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
          await invoke("send_telemetry");
          setCollectData(true);
          await box.set("collectUserData", true);
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
        className="bg-slate-800 hover:text-slate-50 hover:bg-red-700 w-fit my-4"
        onClick={async () => {
          await invoke("remove_telemetry");
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
          checked={extendedNavigation}
          onChange={async (newValue) => {
            setExtendedNavigation(newValue);
            await box.set("extendedNavigation", newValue);
            await box.save();
          }}
        >
          <span
            aria-hidden="true"
            className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
          />
        </Switch>
        <Label className="ml-4">{t("extendedNavigation")}</Label>
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
