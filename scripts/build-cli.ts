/** @format */

// Builds quadrantmc and copies it to src-tauri/binaries/ under the
// target-triple name Tauri expects for `bundle.externalBin`. Runs as part of
// `beforeBuildCommand` in src-tauri/tauri.cli.conf.json, where the Tauri CLI
// sets TAURI_ENV_TARGET_TRIPLE and, for debug builds, TAURI_ENV_DEBUG.

import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const tauriDir = join(import.meta.dirname, "..", "src-tauri");
const manifestPath = join(tauriDir, "Cargo.toml");

function run(command: string, args: string[], capture = false): string {
  const result = spawnSync(command, args, {
    stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit",
    encoding: "utf8",
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(
      `\`${command} ${args.join(" ")}\` exited with status ${result.status}`,
    );
  }
  return result.stdout ?? "";
}

const hostTriple = run("rustc", ["-vV"], true).match(/^host: (\S+)$/m)?.[1];
if (!hostTriple) {
  throw new Error("`rustc -vV` did not report a host triple");
}
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || hostTriple;
const profile = process.env.TAURI_ENV_DEBUG === "true" ? "debug" : "release";
const exe = triple.includes("windows") ? ".exe" : "";

const cargoArgs = [
  "build",
  "--manifest-path",
  manifestPath,
  "-p",
  "quadrant-cli",
];
if (profile === "release") {
  cargoArgs.push("--release");
}
// Without --target, a host build shares target/release with the app build
// instead of compiling every dependency a second time.
if (triple !== hostTriple) {
  cargoArgs.push("--target", triple);
}
run("cargo", cargoArgs);

const { target_directory: targetDir } = JSON.parse(
  run(
    "cargo",
    [
      "metadata",
      "--format-version",
      "1",
      "--no-deps",
      "--manifest-path",
      manifestPath,
    ],
    true,
  ),
) as { target_directory: string };
const builtPath = join(
  targetDir,
  ...(triple === hostTriple ? [] : [triple]),
  profile,
  `quadrantmc${exe}`,
);
const binariesDir = join(tauriDir, "binaries");
const sidecarPath = join(binariesDir, `quadrantmc-${triple}${exe}`);

mkdirSync(binariesDir, { recursive: true });
copyFileSync(builtPath, sidecarPath);
console.log(`Copied ${builtPath} to ${sidecarPath}`);
