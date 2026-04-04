/** @format */

import { useTranslation } from "react-i18next";
import quadrantLocale from "../../../../i18n";
import {
  AccountInfo,
  InstalledModpack,
  LocalModpack,
  SyncContext,
  SyncedModpack,
} from "../../../../intefaces";
import {
  Dialog,
  DialogBackdrop,
  DialogPanel,
  DialogTitle,
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
  Field,
  Input,
  Label,
  Switch,
} from "@headlessui/react";
import {
  MdCheck,
  MdDelete,
  MdDownload,
  MdExpandMore,
  MdPersonAdd,
  MdShare,
} from "react-icons/md";
import { AnimatePresence, motion } from "motion/react";

export interface SyncedModpackProps {
  modpack: SyncedModpack;
  localModpack?: LocalModpack;
}
import { Fragment, useContext, useEffect, useState } from "react";
import Button from "../../../core/Button";
import {
  getAccountInfo,
  inviteMember,
  kickMember,
  shareModpackRaw,
} from "../../../../tools";
import { ContentContext } from "../../../../intefaces";
import { ShareSyncContext } from "../ShareSyncPage";
import { invoke } from "@tauri-apps/api/core";

export default function SyncedModpackComponent({
  modpack,
  localModpack,
}: SyncedModpackProps) {
  const [accountInfo, setAccountInfo] = useState<AccountInfo | null>(null);

  const formatter = new Intl.DateTimeFormat(quadrantLocale.language, {
    weekday: "long",
    day: "2-digit",
    month: "2-digit",
    year: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
  let localFormattedDate = "-";
  if (localModpack !== undefined) {
    let localDateMultiplier = 1;
    const localTimeStamp = localModpack.lastSynced;
    if (localTimeStamp <= 170406720000) {
      localDateMultiplier = 1000;
    }
    const localDate = new Date(localTimeStamp * localDateMultiplier);
    localFormattedDate = formatter.format(localDate);
  }
  let cloudDateMultiplier = 1;
  if (modpack.last_synced <= 170406720000) {
    cloudDateMultiplier = 1000;
  }

  const date = new Date(modpack.last_synced * cloudDateMultiplier);

  const formattedDate = formatter.format(date);

  const { t } = useTranslation();

  const syncContext = useContext(SyncContext);
  const contentContext = useContext(ContentContext);
  const shareSyncContext = useContext(ShareSyncContext);

  const [userToInvite, setUserToInvite] = useState<string>("");
  const [userToInviteAdmin, setUserToInviteAdmin] = useState<boolean>(false);
  const [openInviteDialog, setOpenInviteDialog] = useState(false);

  const modConfigObject: InstalledModpack = {
    name: modpack.name,
    mods: JSON.parse(modpack.mods),
    modLoader: modpack.mod_loader,
    version: modpack.minecraft_version,
  };

  useEffect(() => {
    const effect = async () => {
      const accountInfo = await getAccountInfo();
      setAccountInfo(accountInfo);
    };
    effect().catch(console.error);
  }, []);

  return (
    <motion.div
      className="p-4 bg-slate-900 m-4 rounded-4xl transform-gpu backface-hidden will-change-[transform,opacity]"
      initial={{ y: 24, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
    >
      <div className="mx-4">
        <h1 className="font-extrabold text-4xl">{modpack.name}</h1>
        <h2 className="text-2xl text-slate-300 ">
          {modpack.mod_loader} {modpack.minecraft_version} |{" "}
          {t("modCount", { amount: JSON.parse(modpack.mods).length })}
        </h2>
        <div className="text-bold text-lg">
          <h3>{t("cloudSyncDate", { date: formattedDate })}</h3>
          <h3>{t("localSyncDate", { date: localFormattedDate })}</h3>
        </div>
        <div className="flex items-center py-4 px-2 bg-slate-700 rounded-4xl my-4 h-16">
          <Button
            className="flex items-center self-center bg-emerald-700 hover:bg-emerald-800"
            onClick={async () => {
              shareSyncContext.changeTab(0);
              shareSyncContext.setModpack(modConfigObject);
              shareSyncContext.setSync(modpack.last_synced);
              shareSyncContext.setModpackId(modpack.modpack_id);
            }}
          >
            {t("download")}
            <MdDownload className="w-6 h-6 mx-2" />
          </Button>
          <Button
            className="flex items-center self-center bg-blue-600 hover:bg-blue-800 ml-2 justify-center"
            onClick={async () => {
              try {
                await shareModpackRaw(modConfigObject);
              } catch (e: any) {
                console.error(e);
                contentContext.setSnackbar({
                  message: t(e),
                  className: "bg-red-700 rounded-4xl",
                  timeout: 5000,
                });
              }
            }}
          >
            {t("share")}
            <MdShare className="w-6 h-6 mx-2" />
          </Button>
        </div>
        <Disclosure as="div" className={"w-full"}>
          {({ open }) => (
            <>
              <div className="bg-slate-700 rounded-4xl">
                <DisclosureButton
                  className={
                    "w-full text-start  data-open:rounded-b-none p-2 flex flex-1 group h-16 items-center "
                  }
                >
                  <span className="text-start ml-2 flex w-full font-bold items-start">
                    {t("details")}
                  </span>
                  <div className="w-full items-end justify-end flex  ">
                    <MdExpandMore className="h-8 flex w-8 ml-0 group-data-open:rotate-180" />
                  </div>
                </DisclosureButton>
                <DisclosurePanel static as={Fragment}>
                  <AnimatePresence>
                    {open && (
                      <motion.div
                        className={"p-4"}
                        initial={{ opacity: 1, y: -50 }}
                        animate={{ opacity: 1, y: 0 }}
                        transition={{ duration: 0.1, ease: "easeIn" }}
                        exit={{ opacity: 0, y: -50 }}
                      >
                        {modpack.owners.map((owner) => {
                          const userIfOwner = {
                            username: accountInfo?.login,
                            admin: true,
                          };
                          const isOwner =
                            owner !== userIfOwner &&
                            owner.username !== accountInfo?.login;
                          const kickButton = isOwner ? (
                            <Button
                              className="bg-red-700 hover:bg-red-800 text-white px-4 mr-4"
                              onClick={async () => {
                                await kickMember(
                                  modpack.modpack_id,
                                  owner.username,
                                );
                                syncContext.refreshSyncedModpacks();
                              }}
                            >
                              {t("kick")}
                            </Button>
                          ) : (
                            <></>
                          );

                          return (
                            <div
                              key={owner.username}
                              className="my-4 bg-slate-800 rounded-full p-2 flex items-center justify-center font-bold h-20 "
                            >
                              <p className="flex items-center px-4 justify-start w-full">
                                {owner.admin
                                  ? t("owner", { username: owner.username })
                                  : owner.username}
                              </p>
                              <div className="w-full items-end text-end justify-end">
                                {kickButton}
                              </div>
                            </div>
                          );
                        })}
                        <div className="flex w-full items-center justify-center">
                          <Button
                            className="bg-red-700 flex items-center justify-center  hover:bg-red-800 w-full mr-2 "
                            onClick={async () => {
                              try {
                                await invoke("delete_synced_modpack", {
                                  modpackId: modpack.modpack_id,
                                });
                                contentContext.setSnackbar({
                                  message: (
                                    <span className="flex items-center justify-center">
                                      <p>{t("delete")}</p>
                                      <MdDelete className="w-6 h-6 mx-2" />
                                    </span>
                                  ),
                                  className:
                                    "bg-emerald-700 text-white font-bold",
                                  timeout: 5000,
                                });
                                syncContext.refreshSyncedModpacks();
                              } catch (e: any) {
                                console.error(e);
                                contentContext.setSnackbar({
                                  message: t(e),
                                  className: "bg-red-700 rounded-4xl",
                                  timeout: 5000,
                                });
                              }
                            }}
                          >
                            {t("delete")}
                            <MdDelete className="w-6 h-6 mx-2" />
                          </Button>
                          <Button
                            className="bg-emerald-700 flex items-center justify-center hover:bg-emerald-800 w-full "
                            onClick={async () => {
                              setOpenInviteDialog(true);
                            }}
                          >
                            {t("invite")}
                            <MdPersonAdd className="w-6 h-6 ml-2" />
                          </Button>
                        </div>
                      </motion.div>
                    )}
                    <Dialog
                      open={openInviteDialog}
                      onClose={() => {
                        setOpenInviteDialog(false);
                        setUserToInvite("");
                        setUserToInviteAdmin(false);
                      }}
                      className={"relative z-50"}
                    >
                      <DialogBackdrop className="fixed inset-0 opacity-60 bg-slate-950/30" />
                      <div className="fixed inset-0 flex w-screen items-center justify-center p-4">
                        <DialogPanel
                          className={
                            "max-w-xl space-y-4 rounded-4xl bg-slate-800 p-8"
                          }
                        >
                          <DialogTitle className={"font-black text-2xl"}>
                            {t("invite")}
                          </DialogTitle>
                          <Input
                            type="text"
                            value={userToInvite}
                            autoComplete="off"
                            className="bg-slate-700 focus:bg-slate-600 focus: focus:border-2 focus:border-slate-500 w-full p-2 rounded-4xl font-semibold hover:bg-slate-600  focus:outline-hidden"
                            placeholder={t("username")}
                            onChange={async (e) =>
                              setUserToInvite(e.target.value)
                            }
                          ></Input>
                          <Field className={"flex items-center "}>
                            <Switch
                              className={
                                "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700 "
                              }
                              checked={userToInviteAdmin}
                              onChange={async (newValue) => {
                                setUserToInviteAdmin(newValue);
                              }}
                            >
                              <span
                                aria-hidden="true"
                                className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
                              />
                            </Switch>
                            <Label className={"mx-2 font-black text-xl"}>
                              {t("admin")}
                            </Label>
                          </Field>
                          <div className="flex">
                            <Button
                              className="bg-emerald-700 hover:bg-emerald-800 w-full "
                              onClick={async () => {
                                try {
                                  await inviteMember(
                                    modpack.modpack_id,
                                    userToInvite,
                                    userToInviteAdmin,
                                  );
                                  contentContext.setSnackbar({
                                    message: (
                                      <span className="flex items-center justify-center">
                                        <p>{t("invite")}</p>
                                        <MdCheck className="w-6 h-6 mx-2" />
                                      </span>
                                    ),
                                    className:
                                      "bg-emerald-700 text-white font-bold",
                                    timeout: 5000,
                                  });
                                  setOpenInviteDialog(false);
                                } catch (e: any) {
                                  contentContext.setSnackbar({
                                    className:
                                      "bg-red-700 text-white font-bold",
                                    message: t(e),
                                    timeout: 5000,
                                  });
                                  setOpenInviteDialog(false);
                                }
                              }}
                            >
                              {t("invite")}
                            </Button>
                            <Button
                              className="ml-2 bg-slate-600 w-full hover:bg-slate-700"
                              onClick={async () => {
                                setOpenInviteDialog(false);
                                setUserToInvite("");
                                setUserToInviteAdmin(false);
                              }}
                            >
                              {t("cancel")}
                            </Button>
                          </div>
                        </DialogPanel>
                      </div>
                    </Dialog>
                  </AnimatePresence>
                </DisclosurePanel>
              </div>
            </>
          )}
        </Disclosure>
      </div>
    </motion.div>
  );
}
