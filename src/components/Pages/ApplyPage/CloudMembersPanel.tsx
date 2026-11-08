/** @format */

import { Fragment, useContext, useState } from "react";
import { useTranslation } from "react-i18next";
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
import { AnimatePresence, motion } from "motion/react";
import {
  MdCheck,
  MdDelete,
  MdDownload,
  MdExpandMore,
  MdPersonAdd,
} from "react-icons/md";
import {
  AccountInfo,
  ContentContext,
  InstalledModpack,
  SyncContext,
  SyncedModpack,
} from "../../../intefaces";
import { inviteMember, kickMember } from "../../../tools";
import { invoke } from "../../../desktop";
import Button from "../../core/Button";
import CancelButton from "../../core/CancelButton";
import ConfirmDialog from "../../core/ConfirmDialog";
import { useModpackInstall } from "./useModpackInstall";
import { useReportError } from "../../../useReportError";

export interface CloudMembersPanelProps {
  modpack: SyncedModpack;
  accountInfo: AccountInfo | null;
  /**
   * Adds a Force pull button that reinstalls the cloud version over the local
   * copy in place, for modpacks that are also installed locally.
   */
  localName?: string;
}

/**
 * The collapsed Quadrant Sync management panel of a modpack card: members
 * with kick, invite, delete from the cloud, and optionally download.
 */
export default function CloudMembersPanel({
  modpack,
  accountInfo,
  localName,
}: CloudMembersPanelProps) {
  const { t } = useTranslation();
  const reportError = useReportError();
  const syncContext = useContext(SyncContext);
  const contentContext = useContext(ContentContext);

  const [userToInvite, setUserToInvite] = useState<string>("");
  const [userToInviteAdmin, setUserToInviteAdmin] = useState<boolean>(false);
  const [openInviteDialog, setOpenInviteDialog] = useState(false);
  const [memberToKick, setMemberToKick] = useState<string | null>(null);
  const [isDeleteOpen, setIsDeleteOpen] = useState(false);

  const closeInviteDialog = () => {
    setOpenInviteDialog(false);
    setUserToInvite("");
    setUserToInviteAdmin(false);
  };

  const currentUserIsAdmin = modpack.owners.some(
    (candidate) => candidate.username === accountInfo?.login && candidate.admin,
  );

  const modConfigObject: InstalledModpack = {
    name: localName ?? modpack.name,
    mods: JSON.parse(modpack.mods),
    modLoader: modpack.mod_loader,
    version: modpack.minecraft_version,
  };
  const { install: forcePull, progress: pullProgress } = useModpackInstall(
    modConfigObject,
    { syncedAt: modpack.last_synced, modpackId: modpack.modpack_id },
  );

  return (
    <Disclosure as="div" className={"w-full"}>
      {({ open }) => (
        <div className="bg-slate-700 rounded-4xl">
          <DisclosureButton
            className={
              "w-full text-start data-open:rounded-b-none p-2 flex flex-1 group h-12 items-center"
            }
          >
            <span className="text-start ml-2 flex w-full font-bold items-start">
              {t("cloudMembers")}
            </span>
            <div className="w-full items-end justify-end flex">
              <MdExpandMore className="h-6 flex w-6 ml-0 group-data-open:rotate-180" />
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
                    const canKick =
                      currentUserIsAdmin &&
                      owner.username !== accountInfo?.login;
                    return (
                      <div
                        key={owner.username}
                        className="my-3 bg-slate-800 rounded-full p-2 flex items-center justify-center font-bold h-14"
                      >
                        <p className="flex items-center px-4 justify-start w-full">
                          {owner.admin
                            ? t("owner", { username: owner.username })
                            : owner.username}
                        </p>
                        <div className="w-full items-end text-end justify-end">
                          {canKick && (
                            <Button
                              className="bg-slate-700 hover:bg-red-700 px-4 mr-4"
                              onClick={() => setMemberToKick(owner.username)}
                            >
                              {t("kick")}
                            </Button>
                          )}
                        </div>
                      </div>
                    );
                  })}
                  <div className="flex w-full flex-wrap items-center justify-center gap-2">
                    {localName !== undefined && (
                      <Button
                        className={
                          "flex items-center justify-center flex-1 " +
                          (pullProgress === 1
                            ? "bg-emerald-600 hover:bg-emerald-700"
                            : "bg-slate-800 cursor-not-allowed")
                        }
                        onClick={forcePull}
                      >
                        {pullProgress === 1
                          ? t("forcePull")
                          : (pullProgress * 100).toFixed(2) + "%"}
                        <MdDownload className="w-6 h-6 mx-2" />
                      </Button>
                    )}
                    <Button
                      className="bg-slate-800 flex items-center justify-center hover:bg-red-700 flex-1"
                      onClick={() => setIsDeleteOpen(true)}
                    >
                      {t("delete")}
                      <MdDelete className="w-6 h-6 mx-2" />
                    </Button>
                    <Button
                      className="bg-emerald-600 flex items-center justify-center hover:bg-emerald-700 flex-1"
                      onClick={() => setOpenInviteDialog(true)}
                    >
                      {t("invite")}
                      <MdPersonAdd className="w-6 h-6 ml-2" />
                    </Button>
                  </div>
                </motion.div>
              )}
              <ConfirmDialog
                open={memberToKick !== null}
                title={t("confirmKickMember", { username: memberToKick })}
                message={t("confirmKickMemberText", {
                  username: memberToKick,
                  name: modpack.name,
                })}
                confirmLabel={t("kick")}
                onCancel={() => setMemberToKick(null)}
                onConfirm={async () => {
                  if (memberToKick === null) return;
                  try {
                    await kickMember(modpack.modpack_id, memberToKick);
                    syncContext.refreshSyncedModpacks();
                  } catch (e: any) {
                    reportError(e);
                  }
                }}
              />
              <ConfirmDialog
                open={isDeleteOpen}
                title={t("confirmDeleteSyncedModpack", { name: modpack.name })}
                message={t("confirmDeleteSyncedModpackText")}
                confirmLabel={t("delete")}
                onCancel={() => setIsDeleteOpen(false)}
                onConfirm={async () => {
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
                      className: "bg-emerald-600 font-bold",
                      timeout: 5000,
                    });
                    syncContext.refreshSyncedModpacks();
                  } catch (e: any) {
                    reportError(e);
                  }
                }}
              />
              <Dialog
                open={openInviteDialog}
                onClose={closeInviteDialog}
                className={"relative z-50"}
              >
                <DialogBackdrop className="fixed inset-0 opacity-60 bg-slate-950/30" />
                <div className="fixed inset-0 flex w-screen items-center justify-center p-4">
                  <DialogPanel
                    className={
                      "max-w-xl space-y-4 rounded-4xl bg-slate-800 p-8"
                    }
                  >
                    <DialogTitle className={"font-black text-xl"}>
                      {t("invite")}
                    </DialogTitle>
                    <Input
                      type="text"
                      value={userToInvite}
                      autoComplete="off"
                      className="bg-slate-700 focus:bg-slate-600 focus:border-2 focus:border-slate-500 w-full p-2 rounded-4xl font-semibold hover:bg-slate-600 focus:outline-hidden"
                      placeholder={t("username")}
                      onChange={(e) => setUserToInvite(e.target.value)}
                    ></Input>
                    <Field className={"flex items-center"}>
                      <Switch
                        className={
                          "group inline-flex h-8 align-middle w-16 rounded-full bg-slate-700 transition data-checked:bg-emerald-800 hover:bg-slate-600 hover:data-checked:bg-emerald-700"
                        }
                        checked={userToInviteAdmin}
                        onChange={setUserToInviteAdmin}
                      >
                        <span
                          aria-hidden="true"
                          className="pointer-events-none inline-block size-8 translate-x-0 rounded-full bg-slate-300 ring-0 shadow-lg transition duration-200 ease-in-out group-data-checked:translate-x-8"
                        />
                      </Switch>
                      <Label className={"mx-2 font-black text-base"}>
                        {t("admin")}
                      </Label>
                    </Field>
                    <div className="flex">
                      <Button
                        className="bg-emerald-600 hover:bg-emerald-700 w-full"
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
                              className: "bg-emerald-600 font-bold",
                              timeout: 5000,
                            });
                          } catch (e: any) {
                            reportError(e);
                          }
                          closeInviteDialog();
                        }}
                      >
                        {t("invite")}
                      </Button>
                      <CancelButton
                        className="ml-2 w-full"
                        onClick={closeInviteDialog}
                      />
                    </div>
                  </DialogPanel>
                </div>
              </Dialog>
            </AnimatePresence>
          </DisclosurePanel>
        </div>
      )}
    </Disclosure>
  );
}
