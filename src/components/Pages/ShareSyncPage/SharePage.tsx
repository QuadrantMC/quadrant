/** @format */

import { useEffect, useState } from "react";
import { IMod, InstalledModpack } from "../../../intefaces";
import CircularProgress from "../../core/CircularProgress";
import { useTranslation } from "react-i18next";
import { Input } from "@headlessui/react";
import Button from "../../core/Button";
import {
  getMod,
  getQuadrantShareModpack,
  installModpack,
} from "../../../tools";
import Mod from "../../shared/Mod";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { listen } from "@tauri-apps/api/event";
import { ContentContext } from "../../../intefaces";
import { useContext } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface SharePageProps {
  preselectedModpack: InstalledModpack | undefined;
  modpackSync: number | null;
  modpackId: string | null;
}

export default function SharePage({
  preselectedModpack,
  modpackSync,
  modpackId,
}: SharePageProps) {
  const [modpack, setModpack] = useState<InstalledModpack | undefined>(
    undefined,
  );
  const [mods, setMods] = useState<IMod[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const { t } = useTranslation();
  const [code, setCode] = useState("");
  const [progress, setProgress] = useState(1);

  const getModpack = async () => {
    if (code.trim().length !== 7) {
      console.log("Code is not valid");
      return;
    }

    const newModpack = await getQuadrantShareModpack(code);
    console.log("New modpack: " + newModpack);
    setModpack(newModpack);
  };

  const installRemoteModpack = async () => {
    if (progress !== 1) {
      return;
    }
    await installModpack(modpack!);
    if (modpackSync) {
      await invoke("set_modpack_sync_date", {
        time: modpackSync,
        modpack: modpack!.name,
        modpackId: modpackId,
      });
    }
  };

  const context = useContext(ContentContext);

  useEffect(() => {
    console.log("Mods count: " + mods.length);
  }, [mods]);

  useEffect(() => {
    listen("modpackDownloadProgress", (progress: any) => {
      if (progress.payload === 1) {
        context.setSnackbar({
          className: "bg-emerald-700 text-white",
          message: t("downloadSuccess"),
          timeout: 5000,
        });
      }
      setProgress(progress.payload);
    });
    if (preselectedModpack !== undefined) {
      setModpack(preselectedModpack);
    }
  }, []);

  useEffect(() => {
    const effect = async () => {
      if (modpack === undefined) {
        setMods([]);
        return;
      }
      setMods([]);

      const fetchMods = async () => {
        console.log("Mods: " + modpack.mods.length);

        for (const mod of modpack.mods) {
          try {
            const newMod = await getMod(
              {
                deletable: false,
                id: mod.id,
                downloadable: false,
                showPreviousVersion: false,
                versionTarget: "",
                modLoader: modpack.modLoader,
                modpack: modpack.name,
                selectable: false,
                selectUrl: null,
              },
              mod.source,
            );
            setMods((prevMods) => [...prevMods, newMod]);
          } catch (error) {
            console.error("Failed to fetch mod:", error);
          }
        }
        setIsLoading(false);
      };

      fetchMods();
    };
    effect().catch(console.error);
  }, [modpack]);

  return (
    <div className="flex flex-1 flex-col justify-center items-center w-full my-8 h-[80vh] ">
      {isLoading && (
        <div className="bg-slate-800 rounded-4xl p-4">
          <CircularProgress />
        </div>
      )}
      {!isLoading && (
        <>
          <div className="bg-slate-800 p-4 flex flex-col rounded-4xl font-bold">
            {modpack === undefined && (
              <>
                <p>{t("manualInput")}</p>
                <Input
                  className="input mt-8"
                  placeholder={t("manualInput")}
                  value={code}
                  onChange={(e) => {
                    e.preventDefault();
                    // Make sure the code is a number
                    if (isNaN(Number(e.target.value))) {
                      return;
                    }
                    if (e.target.value.length > 7) {
                      // 7 is the max length of the code
                      return;
                    }
                    setCode(e.target.value);
                  }}
                  autoComplete="off"
                  type="text"
                ></Input>
              </>
            )}
            {modpack !== undefined && (
              <>
                <p>
                  {modpack.name} | {modpack.modLoader} | {modpack.version} |{" "}
                  {t("modCount", { amount: mods.length })}
                </p>
                <div className=" items-center justify-center my-4 rounded-4xl p-2 h-min  border-slate-900 border-8 ">
                  <div className="grid grid-cols-3 mb-0 2xl:grid-cols-4 gap-6 p-4 max-h-[35vh] max-w-[80vw] overflow-auto  ">
                    {mods.map((mod) => {
                      return (
                        <Mod
                          key={mod.id}
                          mod={mod}
                          modpack={modpack.name}
                          className={"h-72 "}
                        />
                      );
                    })}
                  </div>
                </div>
              </>
            )}
            <div className="flex w-full">
              <Button
                className={
                  "mt-2 w-full mr-1 " +
                  (progress === 1
                    ? "bg-emerald-700 hover:bg-emerald-800"
                    : "bg-slate-700 hover:bg-slate-700 cursor-not-allowed")
                }
                onClick={
                  modpack === undefined ? getModpack : installRemoteModpack
                }
              >
                {progress === 1
                  ? t("download")
                  : (progress * 100).toFixed(2) + "%"}
              </Button>
              {modpack === undefined && (
                <Button
                  className="mt-2 w-full ml-1 bg-sky-500 hover:bg-sky-700 "
                  onClick={async () => {
                    const clipboardText = await readText();
                    if (clipboardText.trim().length > 7) {
                      return;
                    }
                    if (!isNaN(Number(clipboardText.trim()))) {
                      setCode(clipboardText);
                    }
                  }}
                >
                  {t("paste")}
                </Button>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
