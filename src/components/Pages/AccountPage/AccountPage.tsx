/** @format */

import { useContext, useEffect, useRef, useState } from "react";
import { AccountInfo } from "../../../intefaces";
import { clearAccountToken, getAccountInfo, openIn } from "../../../tools";
import Button from "../../core/Button";
import CircularProgress from "../../core/CircularProgress";
import { useTranslation } from "react-i18next";
import { codeChallengeS256, createCodeVerifier } from "../../../pkce";
import { MdOpenInBrowser } from "react-icons/md";
import { ContentContext } from "../../../intefaces";
import {
  cancelOAuthServer,
  createDesktopStore,
  emit,
  invoke,
  listen,
  onOAuthUrl,
  startOAuthServer,
} from "../../../desktop";

// How long a started sign-in waits for the browser to come back.
const OAUTH_LOGIN_TIMEOUT_MS = 10 * 60 * 1000;

// The callback server outlives this page: switching tabs while the browser
// is open must not drop the login, and a remounted page must still be able to
// shut down a server the previous instance started.
let endPendingLogin: (() => Promise<void>) | null = null;

async function cancelPendingLogin() {
  const end = endPendingLogin;
  endPendingLogin = null;
  await end?.();
}

export default function AccountPage() {
  const { t } = useTranslation();
  const [accountInfo, setAccountInfo] = useState<
    AccountInfo | null | undefined
  >(undefined);
  const [loginWarning, setLoginWarning] = useState<string | null>(null);
  const loginStartingRef = useRef(false);
  const [loginStarting, setLoginStarting] = useState(false);

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

  const configRef = useRef(createDesktopStore("config.json"));
  const config = configRef.current;
  const context = useContext(ContentContext);

  const showLoginFailureWarning = () => {
    const warningMessage = t("accountLoginFailed");
    setLoginWarning(warningMessage);
    context.setSnackbar({
      message: warningMessage,
      className: "bg-red-700 rounded-4xl",
      timeout: 6000,
    });
  };

  useEffect(() => {
    let isUnmounted = false;
    let unlistenRecheck: (() => void) | null = null;

    const effect = async () => {
      const unlisten = await listen<string>("recheckAccountToken", async () => {
        if (!isUnmounted) {
          await updateAccountInfo();
        }
      });
      if (isUnmounted) {
        unlisten();
        return;
      }
      unlistenRecheck = unlisten;

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
        <div className="flex w-[75%] max-w-4xl flex-col items-center justify-center rounded-4xl bg-slate-800 px-6 py-10 text-center shadow-2xl">
          <CircularProgress />
          <h1 className="mt-6 text-2xl font-extrabold">
            {t("loadingAccount")}
          </h1>
          <p className="mt-4 max-w-2xl text-lg text-slate-400">
            {t("loadingAccountDetails")}
          </p>
        </div>
      </div>
    );
  }

  return accountInfo !== null ? (
    <>
      <div className="flex flex-col items-center justify-center align-middle flex-1 h-full">
        <h1 className="font-extrabold text-2xl">
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
            className="bg-slate-800 hover:bg-red-700 h-min mx-2 w-full"
            onClick={async () => {
              await clearAccountToken();
              setLoginWarning(null);
              // The bell and the cloud modpack list belong to the account
              // that just left; they drop it when they hear this.
              await emit("recheckAccountToken", null);
              await updateAccountInfo(false);
            }}
          >
            {t("signout")}
          </Button>
          <Button
            className="bg-blue-600 hover:bg-blue-700 h-min mx-2 w-full"
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
          <h1 className="font-extrabold text-2xl my-2">{t("signIn")}</h1>
        </div>
        {loginWarning !== null && (
          <div className="my-4 rounded-4xl border border-red-400/40 bg-red-950/60 p-4 text-left">
            <p className="text-base font-bold text-red-100">{loginWarning}</p>
          </div>
        )}
        <div className="w-full flex flex-row">
          <Button
            onClick={async () => {
              // A second click before the callback server is up would start a
              // second server and leak the first one's cleanup.
              if (loginStartingRef.current) {
                return;
              }
              loginStartingRef.current = true;
              setLoginStarting(true);
              setLoginWarning(null);
              await cancelPendingLogin();
              const randomBytes = new Uint8Array(24);
              crypto.getRandomValues(randomBytes);
              const randomString = Array.from(randomBytes, (b) =>
                b.toString(16).padStart(2, "0"),
              ).join("");

              try {
                const codeVerifier = createCodeVerifier();
                await config.set("oauthState", randomString);
                await config.set("oauthVerifier", codeVerifier);
                const port = await startOAuthServer({
                  response:
                    "<html><body><h1>" +
                    t("returnToTheApp") +
                    "</h1></body></html>",
                  ports: [4000, 4001, 4002, 4003, 4004, 4005],
                });
                let unlistenOAuth: (() => void) | null = null;
                const expiry = setTimeout(
                  () => void cancelPendingLogin().catch(console.error),
                  OAUTH_LOGIN_TIMEOUT_MS,
                );
                endPendingLogin = async () => {
                  clearTimeout(expiry);
                  unlistenOAuth?.();
                  await cancelOAuthServer(port);
                };

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
                authUrl.searchParams.set(
                  "code_challenge",
                  await codeChallengeS256(codeVerifier),
                );
                authUrl.searchParams.set("code_challenge_method", "S256");
                openIn(authUrl.toString());

                unlistenOAuth = await onOAuthUrl(async (rawUrl) => {
                  let url: URL;
                  try {
                    url = new URL(rawUrl);
                  } catch {
                    return;
                  }
                  // The local OAuth callback server surfaces every request it
                  // receives, including incidental ones like /favicon.ico.
                  // Ignore anything that isn't the actual OAuth redirect so a
                  // successful login isn't reported as a failure.
                  if (
                    !url.searchParams.has("code") &&
                    !url.searchParams.has("error")
                  ) {
                    return;
                  }
                  try {
                    const oAuthState = await config.get<string>("oauthState");

                    const providedState = url.searchParams.get("state");
                    if (providedState !== oAuthState) {
                      throw new Error("OAuth state mismatch");
                    }
                    const codeVerifier =
                      await config.get<string>("oauthVerifier");
                    // A state is good for one redirect only.
                    await config.delete("oauthState");
                    await config.delete("oauthVerifier");
                    const code = url.searchParams.get("code");
                    if (code === null) {
                      throw new Error("Missing OAuth code");
                    }
                    setAccountInfo(undefined);
                    await invoke("oauth2_login", {
                      code: code,
                      redirectUri: redirectUri,
                      codeVerifier,
                    });
                  } catch (error) {
                    console.error(error);
                    setAccountInfo(null);
                    showLoginFailureWarning();
                  } finally {
                    await cancelPendingLogin();
                  }
                });
              } catch (error) {
                console.error("Error starting OAuth server:", error);
                await cancelPendingLogin();
                showLoginFailureWarning();
              } finally {
                // The guard only spans server startup; once the callback
                // listener is registered a new click may restart the flow.
                loginStartingRef.current = false;
                setLoginStarting(false);
              }
            }}
            disabled={loginStarting}
            className="bg-blue-600 hover:bg-blue-700 w-full mx-2"
          >
            {t("signInWithOAuth")}
          </Button>
          <Button
            onClick={() => openIn("https://mrquantumoff.dev/account/register")}
            className="bg-slate-800 hover:bg-slate-700 w-full mx-2"
          >
            {t("dontHaveAccount")}
          </Button>
        </div>
      </div>
      <div className="mt-4 w-[75%]">
        <Button
          className="bg-slate-800 hover:bg-slate-700 w-full h-full flex flex-row items-center justify-center text-lg"
          onClick={async () => {
            openIn(
              "https://usequadrant.dev/terms",
            );
          }}
        >
          {t("acceptQuadrantIDTOS")}
          <MdOpenInBrowser className="w-6 h-6 ml-2 "></MdOpenInBrowser>
        </Button>
      </div>
    </div>
  );
}
