# AGENTS.md

> Quadrant Next — a Tauri + React desktop app to manage Minecraft mods and modpacks.
> `CLAUDE.md` is a symlink to this file. Keep both audiences (Claude + humans) in mind when editing.

## Quick start
- **Package manager**: `bun` (`bun@1.3.13`) — never use npm, yarn, or pnpm. `bun install` in root.
- All TS/JSX files start with `/** @format */`; keep this header when creating new files. (Rust files don't.)
- **React 19** with the **React Compiler** enabled (`babel-plugin-react-compiler`) — don't hand-write `useMemo`/`useCallback` for things the compiler already memoizes.

## Commands

| Task | Command |
|------|---------|
| Renderer-only dev (Vite, browser) | `bun run dev` |
| Tauri dev (full desktop app) | `bun run dev:tauri` |
| Build shared renderer | `bun run build` (runs `tsc && vite build`) |
| Build Tauri app | `bun run build:tauri` |
| Lint | `bun run lint` (ESLint flat config, `eslint.config.js`) |
| Tauri dev without proprietary features | `bun tauri dev -- -- --no-default-features` |
| Ad-hoc macOS DMG (no Apple cert) | `bun run tauri:macos:adhoc:build` |

There is **no format/typecheck script** beyond `tsc` inside `bun run build`. Prettier runs via ESLint (`eslint-plugin-prettier`).

## Repository layout

```
src/                     React renderer (shared between browser + Tauri)
  App.tsx                Root; page-based navigation (NO react-router — see below)
  main.tsx               React entry
  tools.ts               Frontend API layer — wraps invoke() into typed functions
  intefaces.tsx          Shared TS types (NOTE the misspelling "intefaces", it is load-bearing)
  modLoaders.ts, uiScale.ts, i18n.ts
  desktop/               RuntimeAdapter abstraction (see Architecture)
  components/
    core/                Primitives: Button, CircularProgress, LinearProgress
    shared/              Mod, Notifications, LoaderOption
    Pages/               One dir per screen: ApplyPage, CurrentModpackPage, SearchPage,
                         ModInstallPage, AccountPage, ShareSyncPage, SettingsPage
  locales/               i18n JSON: en, tr (Turkish), uk (Ukrainian)
src-tauri/               Tauri desktop shell (Rust)
  src/                   Thin Tauri layer: ~56 #[tauri::command]s that delegate into the crates
    lib.rs               App setup + tauri::generate_handler![...] command registration
    tauri_adapter.rs     Implements quadrant-core ports (SettingsStore/SecretStore/EventSink)
    account/ config/ mc_mod/ modpacks/ other/   Command modules
  crates/                Rust workspace (see below)
  capabilities/ gen/ icons/ tauri.conf.json
scripts/quadrant-node/   Node app consuming the N-API addon (quadrant-napi)
.github/workflows/       release.yml, validate-desktop.yml, flatpak.yml, msstore.yml
```

## Architecture

### Frontend ↔ backend boundary
- Feature code **must** use `src/desktop/index.ts` exports (`invoke`, `listen`, `DesktopStore`, `openDialog`, `platform`, …) — **never import `@tauri-apps/*` directly** in feature code.
- The `RuntimeAdapter` interface (`src/desktop/contract.ts`) defines the full host contract. Two implementations:
  - `src/desktop/tauri.ts` — real Tauri host.
  - `src/desktop/browser.ts` — browser fallback (renderer-only `bun run dev`).
- Runtime detection is lazy (`src/desktop/runtime.ts`): Tauri globals first, then browser fallback.
- `src/tools.ts` is the app-facing API surface: it turns `invoke("command_name", args)` calls into typed functions used by pages. Start here to trace a UI action to its backend command.

### Navigation
- **No router.** `App.tsx` holds a `pages: Page[]` array and swaps the active page via `useState` + a `changePage(name)` helper, with its own back/forward history stack. Deep links (mod install, shared modpack codes) mutate this state. Add a screen by adding an entry to the `pages` array.

### Rust workspace (`src-tauri/Cargo.toml`)
Three crates, layered:
- **`quadrant-core`** — Tauri-independent backend/domain logic. Owns config, `models`, `events`, `ports` (host-provided interfaces: `SettingsStore`, `SecretStore`, `EventSink`), `modpacks`, `mc_mod` (Modrinth + CurseForge providers, install, cache, fingerprint/identify), `account` (id/oauth, sync, share, settings-sync), `rss`, `telemetry`. Does **not** own windows, tray, dialogs, or transport.
- **`quadrant-host`** — embeds `quadrant-core` and provides concrete host services (e.g. keyring-backed `SecretStore`, event forwarding). Reusable across the Tauri shell and the Node addon.
- **`quadrant-napi`** — Node N-API bindings (`QuadrantHostAddon`) wrapping `quadrant-host`, consumed by `scripts/quadrant-node`.
- **`src-tauri/src`** is a thin shell over `quadrant-host`/`quadrant-core`: commands mostly forward arguments. Business logic changes belong in the crates, not here.

## Build order & gotchas

- **Compile-time credentials**: Rust uses `env!()` macros (`QUADRANT_API_KEY`, `QUADRANT_OAUTH2_CLIENT_ID`, `QUADRANT_OAUTH2_CLIENT_SECRET`, `ETERNAL_API_TOKEN`). These must be set before building or the build fails. CI provides them; locally supply them or build with `--no-default-features`. To just check that the Rust compiles locally, see the dummy-creds memory ([[rust-cargo-check-dummy-creds]]).
- **Cargo features cascade**: `default` → `proprietary` → `telemetry` + `curseforge` + `quadrant_id`. `--no-default-features` drops CurseForge, telemetry, and Quadrant ID.
- Vite dev server is **fixed at port 1420** with `strictPort: true`. Vite ignores `src-tauri/**` for file watching.
- On Linux, Tauri builds also need `libsecret-1-dev` (keyring).

## Tests
See **[TESTING.md](TESTING.md)** for the full strategy. Quick reference:

- **Rust**: `#[cfg(test)]` modules live next to the code in `quadrant-core`/`quadrant-host`. Run from `src-tauri/` with placeholder creds:
  `QUADRANT_OAUTH2_CLIENT_ID=dev QUADRANT_OAUTH2_CLIENT_SECRET=dev ETERNAL_API_TOKEN=dev QUADRANT_API_KEY=dev cargo test --workspace`
  (`quadrant-core` alone compiles credential-free.) DI is via `&impl SettingsStore/SecretStore/EventSink` port params, so tests pass hand-rolled fakes; I/O uses `tempfile`; HTTP uses `httpmock` against the `#[cfg(test)]` `QUADRANT_TEST_*_API_BASE` / runtime `QUADRANT_API_BASE_URL` seams (serialize on the module's test mutex + clear the shared cache).
- **Frontend**: Vitest + React Testing Library + jsdom. `bun run test` (watch: `bun run test:watch`). Colocated `*.test.ts(x)`. Mock the Tauri boundary at the facade — `vi.mock("./desktop")` / `vi.mock("../../../tools")`; never mock `@tauri-apps/*`. `src/desktop/runtime.ts` exports `__setRuntimeForTests` to reset the memoized adapter.
- **CI** (`validate-desktop.yml`): `test-rust` (cargo test + fmt --check + advisory clippy) and `test-frontend` (bun run test) run on every PR alongside the build jobs.

When you change something, run the tests for the affected side and add coverage for new logic — don't consider a change verified by a successful build alone.

## Styling
- **Tailwind CSS v4** (via PostCSS, not the Tailwind CLI). Config in `postcss.config.cjs`. Per-component `.css` files sit next to some pages/components.
- ESM project (`"type": "module"`). `.cjs` files are the CommonJS exceptions (PostCSS/Tailwind config).
- Animations via `motion` (Framer Motion successor). Icons via `react-icons`. Headless UI via `@headlessui/react`.

## Versioning
`YY.MM.REVISION` (e.g. `26.6.0-stable`). Tags use `v` prefix with a suffix (`-stable`, `-flatpak`, `-msstore`). Prerelease tags skip the suffix. Version lives in `package.json`, `src-tauri/tauri.conf.json`, and the `*.metainfo.xml`/manifest files.
