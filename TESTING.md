# Testing

Quadrant Next is tested at three layers: Rust unit tests, Rust integration tests
(HTTP + filesystem), and frontend unit/component tests. An end-to-end layer is
designed but not yet built (see the end of this document).

CI runs the whole suite on every pull request via `.github/workflows/validate-desktop.yml`
(`test-rust` and `test-frontend` jobs), in parallel with the existing build jobs.

## Running the tests

### Rust

From `src-tauri/`:

```sh
cargo test --workspace
```

`quadrant-core` compiles and tests **credential-free**. The top-level Tauri crate
and the `curseforge` feature still read compile-time `env!()` credentials, so a
full `--workspace` run needs placeholder values (any string works):

```sh
QUADRANT_API_KEY=dev QUADRANT_OAUTH2_CLIENT_ID=dev \
QUADRANT_OAUTH2_CLIENT_SECRET=dev ETERNAL_API_TOKEN=dev \
cargo test --workspace
```

To include the CurseForge provider tests, add the feature:

```sh
QUADRANT_API_KEY=dev QUADRANT_OAUTH2_CLIENT_ID=dev \
QUADRANT_OAUTH2_CLIENT_SECRET=dev ETERNAL_API_TOKEN=dev \
cargo test -p quadrant-core --features curseforge
```

Use the default (dev) profile — the `release` profile has fat LTO and is far too
slow for tests. Also run `cargo fmt --all --check` before pushing.

### Frontend

From the repo root:

```sh
bun run test        # vitest run (one-shot, used in CI)
bun run test:watch  # vitest (watch mode)
```

Runner: **Vitest** with `@vitejs/plugin-react` (React Compiler preset) + jsdom, so
components compile and render exactly as in the app. Config: `vitest.config.ts`;
setup: `src/test/setup.ts` (jest-dom matchers). Test files are colocated with the
code as `*.test.ts` / `*.test.tsx`.

## How the layers are built

### Rust — dependency injection via ports

`quadrant-core` takes its host services as `&impl Trait` parameters
(`SettingsStore`, `SecretStore`, `EventSink`, `Shell`, `Notifier`, `RuntimeState`
in `crates/quadrant-core/src/ports.rs`). Tests supply small hand-rolled fakes —
there is **no mocking framework** and we don't add one. Examples of the fakes are
inline in `config.rs`, `modpacks.rs`, and `account/id.rs`.

- **Pure logic** (model conversions, serde round-trips, fingerprinting, name/path
  validation, RSS parsing): tested directly with fixed inputs.
- **Filesystem** (`modpacks.rs`, `mc_mod/cache.rs`): tested with `tempfile::tempdir()`.
  Functions take `mc_folder: &Path`, so tests point them at a temp dir. `cache.rs`
  reads a global cache dir; a `#[cfg(test)]` `QUADRANT_TEST_CACHE_DIR` override
  seam redirects it (guarded by a test mutex).
- **HTTP** (`mc_mod/modrinth.rs`, `mc_mod/curseforge.rs`, `account/*`): tested with
  `httpmock`. Providers read their base URL from a `#[cfg(test)]` env seam
  (`QUADRANT_TEST_MODRINTH_API_BASE`, `QUADRANT_TEST_CURSEFORGE_API_BASE`); the
  account backend honors `QUADRANT_API_BASE_URL` at runtime. **Important:** these
  tests share a process-global HTTP client + cache, so each must lock the module's
  test mutex (`PROVIDER_HTTP_TEST_MUTEX` / `ACCOUNT_ENV_TEST_MUTEX`) and clear the
  provider cache before asserting.

`quadrant-host` tests (`crates/quadrant-host/src/lib.rs`) cover the concrete
port implementations: `JsonFileStore` persistence, the keyring-free paths, and the
`HostEventBridge` broadcast fan-out.

The thin Tauri command layer (`src-tauri/src`) and the `quadrant-napi` bindings are
deliberately not unit-tested — they are one-line delegations to `QuadrantHost`.

### Frontend — mock the Tauri boundary at the facade

All Tauri access funnels through `src/desktop/index.ts` (with a real `browserRuntime`
fake in `src/desktop/browser.ts`). Feature code never imports `@tauri-apps/*`
directly, so tests mock **the facade**, not the plugins:

```ts
vi.mock("./desktop", () => ({ invoke: ..., listen: ..., createDesktopStore: ... }));
vi.mock("../../../tools", () => ({ getModpacks: ..., /* ... */ }));
```

Stub every named export the component imports, or the module mock breaks the import.
Heavy child components (e.g. `ModpackView`, `Mod`) are stubbed to simple markers.
`src/desktop/runtime.ts` exports `__setRuntimeForTests(adapter | undefined)` to
inject a fake adapter or reset the memoized singleton between tests.

- **Pure logic** — `modLoaders.ts`, `uiScale.ts`, extracted `deepLinks.ts`,
  `snackbar.ts`, and `SearchPage/searchLogic.ts` are tested with no mocking.
- **`tools.ts`** — tested with the `./desktop` facade mocked.
- **Components** — tested with React Testing Library + `user-event`; async data
  loads are awaited with `waitFor`.
- **i18n** — `src/locales/locales.test.ts` asserts key parity across `en`/`tr`/`uk`
  and no empty values, guarding against silent drift (`fallbackLng` masks missing
  keys at runtime).

## Adding tests

- Match the existing idiom of the file/area you're touching. Rust files have no
  `/** @format */` header; TS/TSX files do.
- Prefer testing **behavior** (observable output) over implementation details.
- When you add product logic, add coverage for it. A green build is not verification.
- If a Rust function is hard to test because it does its own I/O or reads a global,
  add a narrow injection seam (a `&Path`/base-URL parameter, or a `#[cfg(test)]`
  env override) rather than reaching for a mocking framework.

## Deferred: end-to-end tests

An app-level E2E layer is planned but **not implemented yet**:

- **Tooling**: `tauri-driver` + WebdriverIO driving the built app over the
  webkit2gtk WebDriver. Linux/Windows only — there is no macOS WebDriver for Tauri.
- **Scope**: a single smoke spec — the app boots (with placeholder creds), the main
  page renders, page navigation works, and a settings toggle persists across a restart.
- **CI**: a separate, non-required job (E2E is inherently flakier than unit tests),
  needing `webkit2gtk-driver` + `xvfb` on the Linux runner.

The unit and integration layers cover most of the real risk; build the E2E layer
once they've stabilized.
