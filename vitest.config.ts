/** @format */

import { defineConfig } from "vitest/config";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";

// Standalone test config on purpose: the app's vite.config.ts carries
// Tauri-specific server settings (fixed port, HMR host) that don't apply here.
export default defineConfig({
  plugins: [react({ babel: { presets: [reactCompilerPreset()] } })],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
