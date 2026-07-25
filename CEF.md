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
`3.0.0-alpha.6` and crate tag `tauri-cef-v3.0.0-alpha.19` are different things.

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
cef = ["tauri/cef"]
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
`tauri-cef-v3.0.0-alpha.19`, and fs/http/opener to plugins-workspace `feat/cef`.
Patching is required rather than a plain git dependency because every
`tauri-plugin-*` resolves `tauri` from crates.io; without it cargo links two
incompatible copies of the same crate.

The redirect also applies to the `wry` build. That is deliberate — the CEF line
is a superset of the 2.11.5 release these crates already pinned, so both
runtimes build from one source. Deleting the block returns you to plain
crates.io Tauri; `wry` keeps working and `cef` stops compiling.

One upstream sharp edge the pins work around:

* **schemars.** The CEF line bumped `tauri-utils` and `tauri-plugin` to
  schemars 1.x. Released fs/http/opener build scripts still hand
  `tauri_plugin::Builder` a schemars 0.8 `RootSchema`, which does not compile.
  Patching those three to `feat/cef` fixes it; the other plugins never pass a
  scope schema and build unmodified.

The workspace `tauri` requirement is also relaxed from `2.11.5` to `2.11`.
alpha.19 happens to be 2.11.5 again, but the tag ranges over 2.11.3–2.11.5 and a
`2.11.5` floor makes cargo reject the patch outright as not satisfying the
requirement.

### On the core/plugin version skew

plugins-workspace `feat/cef` is pinned to `tauri-cef-v3.0.0-alpha.13` upstream,
one tag family behind the core pinned here. The three patched plugins build and
run fine against alpha.19 — nothing in the 57 commits between the tags touches
the `tauri-plugin` build API they use. Worth re-checking whenever either pin
moves.

alpha.19 is worth the skew on Windows: it carries `fix(cef): ensure proper
z-order for child webviews on Windows`, `fix(runtime): defer ACL build until
after runtime init to avoid CEF allocator race`, a `HDC` handle leak fix in the
Windows DPI getter, and the `cef-dll-sys` lock (alpha.13 pinned `cef` exactly
but left its sys crate on a caret, so cargo resolved mismatched bindings and
`cef` failed to compile). It also moves CEF 148 → 150.

## Status

Verified on Windows 11 / MSVC, at core `tauri-cef-v3.0.0-alpha.19` (CEF 150):

* `cargo check` and `cargo build` pass for **both** `--features cef` and the
  default `wry` build.
* `bun run dev:cef` launches and renders the app correctly.
* The `wry` build is unaffected by the patch table.

Not verified: macOS and Linux (the CEF runtime needs helper-app bundling on
macOS and an `$ORIGIN` rpath on Linux — upstream handles both, but neither has
been exercised here), and the bundlers (`build:cef` produces msi/nsis that
nobody has installed yet).

Known upstream issues worth watching: IPC breaks when DevTools is opened on
Linux (tauri#15764), transparency renders a black screen (tauri#15718).
