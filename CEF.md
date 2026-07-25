# Tauri "v3" (CEF) alpha build

An experimental build of Quadrant that renders with the **Chromium Embedded
Framework** instead of the OS webview. This is a spike on an unreleased,
actively-moving upstream alpha — it is not a supported build, and nothing here
affects the shipping `wry` build unless you ask for it.

## What "Tauri v3" actually is

There is no Tauri 3.0 release, and no `tauri` 3.x on crates.io — the latest
published core is still 2.11.x. What exists is:

| Piece | Where it lives | Version |
|---|---|---|
| `tauri-runtime-cef` (the CEF runtime) | `tauri-apps/tauri`, `feat/cef` branch — **unpublished** | tagged `tauri-cef-v3.0.0-alpha.N` |
| CEF-aware CLI | npm `@tauri-apps/cli-cef` | `3.0.0-alpha.6` |
| Plugins rebuilt for it | `tauri-apps/plugins-workspace`, `feat/cef` branch | plugin versions unchanged |
| JS API (`@tauri-apps/api`, plugin JS) | npm, unchanged | 2.x |

The "v3" name only appears on the CLI npm package and the crate tags. The core
crate still calls itself 2.11.x. The two counters are also not in sync: CLI
`3.0.0-alpha.6` and crate tag `tauri-cef-v3.0.0-alpha.13` are different things.

**No frontend changes are required.** The renderer, `src/desktop/*`, and every
`@tauri-apps/*` JS package are untouched by this.

## Running it

```bash
bun run dev:cef
```

```bash
bun run build:cef
```

Both resolve the CLI by path (`bun node_modules/@tauri-apps/cli-cef/tauri.js`)
rather than through `node_modules/.bin`. Both CLI packages install a bin called
`tauri`, so whichever wins the `.bin` race would be arbitrary — `bun run tauri`
and every existing script keep using the stable 2.x CLI.

`--no-default-features` has to go after `--`; the CEF CLI has no such flag of
its own and passes trailing args to cargo. `proprietary` and `updater` still
come from `build.features` in `tauri.conf.json`, so only `cef` is passed
explicitly.

### Prerequisites beyond the normal ones

* **CMake and Ninja.** `cef-dll-sys` compiles `libcef_dll_wrapper` from source.
  CMake comes with the Visual Studio C++ workload; Ninja ships with it too but
  is *not* on `PATH`. Either add it:

  ```bash
  export PATH="$PATH:/c/Program Files/Microsoft Visual Studio/18/Community/Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja"
  ```

  or install a standalone one (`winget install Ninja-build.Ninja`). Without it
  the build dies at `CMake was unable to find a build program corresponding to
  "Ninja"`.
* **~2 GB of disk and a slow first build.** The CEF binary distribution is
  downloaded on demand into `CEF_PATH`, defaulting to
  `%LOCALAPPDATA%\tauri-cef` (`dirs::cache_dir()/tauri-cef`). Set `CEF_PATH` to
  point at an existing extracted distribution instead.

## How the build is wired

### Runtime selection

`src-tauri/Cargo.toml` gained two mutually-exclusive features:

```toml
default = ["proprietary", "wry"]
wry = ["tauri/wry"]
cef = ["tauri/cef", "dep:cef-dll-sys"]
```

`tauri` itself is now `default-features = false` with Tauri's default list
minus `wry`, so the runtime is chosen entirely by these.

### The `AppHandle` alias

Tauri applies `#[default_runtime(Wry, wry)]` to `AppHandle`, `Context`,
`TrayIcon` and friends, which only supplies the `= Wry` default **when the
`wry` feature is on**. A CEF build therefore cannot write bare
`tauri::AppHandle`. `src-tauri/src/lib.rs` exports:

```rust
#[cfg(feature = "cef")]
pub type TauriRuntime = tauri::Cef;
#[cfg(all(feature = "wry", not(feature = "cef")))]
pub type TauriRuntime = tauri::Wry;

pub type AppHandle = tauri::AppHandle<TauriRuntime>;
```

Every command module imports `crate::AppHandle`, never `tauri::AppHandle`, and
the builder is `tauri::Builder::<TauriRuntime>::new()`. This is not merely
cosmetic: `tauri-plugin-oauth` declares its `tauri` dependency *without*
`default-features = false`, so `tauri/wry` ends up enabled even in a CEF build.
Bare `AppHandle` would then silently resolve to `Wry` and fail to match a
`Builder<Cef>`. The alias makes the choice explicit either way.

Side effect of that same plugin: CEF builds still compile wry and WebView2.
Wasted build time and binary size, not a correctness problem.

### The subprocess entry point

CEF is multi-process — the same executable is re-launched as renderer, GPU and
utility processes, distinguished by a `--type=` argument. `src-tauri/src/main.rs`
carries `#[cfg_attr(feature = "cef", tauri::cef_entry_point)]`, which inserts
that check at the top of `main` and routes those launches into
`tauri::run_cef_helper_process()` — before the tokio runtime or the Wayland
re-exec would run per process.

### The patch table

`[patch.crates-io]` in `src-tauri/Cargo.toml` redirects the Tauri crates to
`tauri-cef-v3.0.0-alpha.13`, and fs/http/opener to plugins-workspace `feat/cef`.
Patching is required rather than a plain git dependency because every
`tauri-plugin-*` resolves `tauri` from crates.io; without it cargo links two
incompatible copies of the same crate.

The redirect also applies to the `wry` build. That is deliberate — the CEF line
is a superset of the 2.11.5 release these crates already pinned, so both
runtimes build from one source. Deleting the block returns you to plain
crates.io Tauri; `wry` keeps working and `cef` stops compiling.

Three upstream sharp edges the pins work around:

1. **schemars.** The CEF line bumped `tauri-utils` and `tauri-plugin` to
   schemars 1.x. Released fs/http/opener build scripts still hand
   `tauri_plugin::Builder` a schemars 0.8 `RootSchema`, which does not compile.
   Patching those three to `feat/cef` fixes it; the other plugins never pass a
   scope schema and build unmodified.
2. **The tauri version floor.** alpha.13's `tauri` is 2.11.3, so the workspace
   requirement had to relax from `2.11.5` to `2.11` or the patch is rejected as
   not satisfying the requirement.
3. **`cef-dll-sys`.** alpha.13 pins `cef = "=148.0.0"` but leaves `cef-dll-sys`
   on a caret requirement, so cargo resolves 148.4.0 against 148.0.0's generated
   bindings and `cef` fails with `no variant ...
   CEF_CONTENT_SETTING_TYPE_SUB_APP_INSTALLATION_PROMPTS`. `src-tauri` takes a
   direct `cef-dll-sys = "=148.0.0"` dependency purely to pin it. Upstream fixed
   this after alpha.13; drop the dependency when the pins move past it.

### Why alpha.13 and not the newest tag

`tauri-cef-v3.0.0-alpha.19` exists, but plugins-workspace `feat/cef` is pinned
to alpha.13 upstream, and the plugin patches have to agree with the core. The
commits between the two are mostly macOS and Linux fixes. Moving up means
bumping both pins together and re-verifying.

## Status

Verified on Windows 11 / MSVC:

* `cargo check` and `cargo build` pass for **both** `--features cef` and the
  default `wry` build.
* The `wry` build is unaffected by the patch table.

Not verified: macOS and Linux (the CEF runtime needs helper-app bundling on
macOS and an `$ORIGIN` rpath on Linux — upstream handles both, but neither has
been exercised here), and the bundlers (`build:cef` produces msi/nsis that
nobody has installed yet).

Known upstream issues worth watching: IPC breaks when DevTools is opened on
Linux (tauri#15764), transparency renders a black screen (tauri#15718).
