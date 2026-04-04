/* eslint-disable @typescript-eslint/no-unused-vars */
/** @format */

import { useContext, useEffect, useState } from "react";
import { AccountInfo } from "../../../intefaces";
import { clearAccountToken, getAccountInfo, openIn } from "../../../tools";
import Button from "../../core/Button";
import CircularProgress from "../../core/CircularProgress";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { LazyStore } from "@tauri-apps/plugin-store";
import { MdOpenInBrowser, MdOutlineAccountCircle } from "react-icons/md";
import { ContentContext } from "../../../intefaces";
import FirstRegisterStep from "./RegisterPages/Step1";
import { start } from "@fabianlars/tauri-plugin-oauth";
import { cancel, onUrl as onOAuth } from "@fabianlars/tauri-plugin-oauth";
import { invoke } from "@tauri-apps/api/core";

export default function AccountPage() {
  const { t } = useTranslation();
  const [accountInfo, setAccountInfo] = useState<
    AccountInfo | null | undefined
  >(undefined);
  const [loginWarning, setLoginWarning] = useState<string | null>(null);

  const updateAccountInfo = async (showLoader = true) => {
    if (showLoader) {
      setAccountInfo(undefined);
    }
    try {
      const newAccountInfo = await getAccountInfo();
      setAccountInfo(newAccountInfo);
      setLoginWarning(null);
    } catch (error) {
      console.error("Failed to get account info", error);
      setAccountInfo(null);
    }
  };

  const config = new LazyStore("config.json");
  const context = useContext(ContentContext);

  const showLoginFailureWarning = () => {
    const warningMessage = t("accountLoginFailed");
    setLoginWarning(warningMessage);
    context.setSnackbar({
      message: warningMessage,
      className: "bg-red-700 text-white rounded-4xl",
      timeout: 6000,
    });
  };

  useEffect(() => {
    let isUnmounted = false;
    let unlistenRecheck: (() => void) | null = null;

    const effect = async () => {
      unlistenRecheck = await listen<string>(
        "recheckAccountToken",
        async () => {
          if (!isUnmounted) {
            await updateAccountInfo();
          }
        },
      );

      await updateAccountInfo();
    };

    effect().catch(console.error);

    return () => {
      isUnmounted = true;
      unlistenRecheck?.();
    };
  }, []);

  if (accountInfo === undefined) {
    return (
      <div className="flex flex-1 h-full w-full items-center justify-center">
        <div className="flex w-[75%] max-w-4xl flex-col items-center justify-center rounded-[2rem] bg-slate-800 px-6 py-10 text-center shadow-2xl">
          <CircularProgress />
          <h1 className="mt-6 text-4xl font-extrabold">
            {t("loadingAccount")}
          </h1>
          <p className="mt-4 max-w-2xl text-lg text-slate-300">
            {t("loadingAccountDetails")}
          </p>
        </div>
      </div>
    );
  }

  return accountInfo !== null ? (
    <>
      <div className="flex flex-col items-center justify-center align-middle flex-1 h-full">
        <h1 className="font-extrabold text-4xl">
          {t("hello", { name: accountInfo.name })}
        </h1>
        <div className="w-[50%] items-center justify-center text-center font-bold">
          <h2 className="bg-slate-800 p-2 rounded-4xl my-4">
            {t("email")}: {accountInfo.email}
          </h2>
          <h2 className="bg-slate-800 p-2 rounded-4xl my-4">
            {t("username")}: {accountInfo.login}
          </h2>
          <h2 className="bg-slate-800 p-2 rounded-4xl my-4">
            {t("syncLimit")}: {accountInfo.quadrant_sync_limit}
          </h2>
          <h2 className="bg-slate-800 p-2 rounded-4xl my-4">
            {t("shareLimit")}: {accountInfo.quadrant_share_limit}
          </h2>
        </div>
        <div className="flex flex-row w-[75%] items-center justify-center mt-4">
          <Button
            className="bg-red-700 hover:bg-red-800 h-min mx-2 w-full"
            onClick={async () => {
              await clearAccountToken();
              setLoginWarning(null);
              updateAccountInfo(false);
            }}
          >
            {t("signout")}
          </Button>
          <Button
            className="bg-blue-600 hover:bg-blue-800 h-min mx-2 w-full"
            onClick={async () => {
              openIn("https://mrquantumoff.dev/account");
            }}
          >
            {t("edit")}
          </Button>
        </div>
      </div>
    </>
  ) : (
    <div className="items-center justify-center align-middle flex flex-1 h-full flex-col w-full ">
      <div className="place-content-center w-[75%] ">
        <div className="bg-slate-800 rounded-4xl p-2 my-4">
          <h1 className="font-extrabold text-4xl my-2">{t("signIn")}</h1>
        </div>
        {loginWarning !== null && (
          <div className="my-4 rounded-4xl border border-red-400/40 bg-red-950/60 p-4 text-left">
            <p className="text-base font-bold text-red-100">{loginWarning}</p>
          </div>
        )}
        <div className="w-full flex flex-row">
          <Button
            onClick={async () => {
              setLoginWarning(null);
              const randomString = Math.random().toString(36).substring(2, 26);

              await config.set("oauthState", randomString);

              try {
                const port = await start({
                  response:
                    "<html><body><h1>" +
                    t("returnToTheApp") +
                    "</h1></body></html>",
                  ports: [4000, 4001, 4002, 4003, 4004, 4005],
                });
                let unlistenOAuth: (() => void) | null = null;

                console.log(`OAuth server started on port ${port}`);

                const redirectUri = `http://127.0.0.1:${port}`;
                const clientId = await invoke<string>("oauth2_client_id");
                const authUrl = new URL(
                  "https://mrquantumoff.dev/account/oauth2/authorize",
                );
                authUrl.searchParams.set("client_id", clientId);
                authUrl.searchParams.set("redirect_uri", redirectUri);
                authUrl.searchParams.set(
                  "scope",
                  "profile:read profile:write sync:read sync:write share:read share:write settings:read settings:write notifications:read",
                );
                authUrl.searchParams.set("response_type", "code");
                authUrl.searchParams.set("state", randomString);
                openIn(authUrl.toString());

                unlistenOAuth = await onOAuth(async (rawUrl) => {
                  try {
                    const url = new URL(rawUrl);
                    const oAuthState = await config.get<string>("oauthState");

                    const providedState = url.searchParams.get("state");
                    console.log("State: " + oAuthState);
                    console.log("Provided state: " + providedState);
                    if (providedState !== oAuthState) {
                      throw new Error("OAuth state mismatch");
                    }
                    const code = url.searchParams.get("code");
                    console.log("Code: " + code);
                    if (code === null) {
                      throw new Error("Missing OAuth code");
                    }
                    setAccountInfo(undefined);
                    await invoke("oauth2_login", {
                      code: code,
                      redirectUri: redirectUri,
                    });
                  } catch (error) {
                    console.error(error);
                    setAccountInfo(null);
                    showLoginFailureWarning();
                  } finally {
                    await cancel(port);
                    unlistenOAuth?.();
                  }
                });
              } catch (error) {
                console.error("Error starting OAuth server:", error);
                showLoginFailureWarning();
              }
            }}
            className="bg-sky-500 hover:bg-sky-800 w-full mx-2"
          >
            {t("signInWithOAuth")}
          </Button>
          <Button
            onClick={async () => {
              context.changeContent({
                content: <FirstRegisterStep />,
                icon: <MdOutlineAccountCircle />,
                title: t("register"),
                main: false,
                name: t("register"),
                style: "",
              });
            }}
            className="bg-blue-500 hover:bg-blue-800 w-full mx-2"
          >
            {t("dontHaveAccount")}
          </Button>
        </div>
      </div>
      <div className="mt-4 w-[47.5vw]">
        <Button
          className="bg-slate-700 hover:bg-slate-800 w-full h-full flex flex-row items-center justify-center text-lg"
          onClick={async () => {
            openIn(
              "https://github.com/quadrantmc/quadrant/blob/master/QUADRANT-ID-TOS.md",
            );
          }}
        >
          {t("acceptQuadrantIDTOS")}
          <MdOpenInBrowser className="w-12 h-12 ml-2 "></MdOpenInBrowser>
        </Button>
      </div>
    </div>
  );
}
