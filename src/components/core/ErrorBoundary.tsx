/** @format */

import { Component, type ErrorInfo, type ReactNode } from "react";
import quadrantLocale from "../../i18n";

interface ErrorBoundaryState {
  failed: boolean;
}

/**
 * Keeps one failed render from leaving a blank window: the user gets a
 * message and a reload instead.
 */
export default class ErrorBoundary extends Component<
  { children: ReactNode },
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error(error, info.componentStack);
  }

  render() {
    if (!this.state.failed) {
      return this.props.children;
    }
    return (
      <main className="flex h-screen w-screen flex-col items-center justify-center gap-4 p-8 text-center">
        <h1 className="text-2xl font-black">
          {quadrantLocale.t("errorBoundaryTitle")}
        </h1>
        <p className="max-w-xl">{quadrantLocale.t("errorBoundaryText")}</p>
        <button
          className="rounded-4xl bg-emerald-600 p-2 px-6 font-extrabold hover:cursor-pointer hover:bg-emerald-700"
          onClick={() => window.location.reload()}
        >
          {quadrantLocale.t("errorBoundaryReload")}
        </button>
      </main>
    );
  }
}
