# quadrantmc

`quadrantmc` does from a terminal what the desktop app does: modpacks, mod search and installs, resource and shader packs, Prism Launcher links, Quadrant ID, Sync, Share and settings. It is a thin layer over `quadrant-host`, the same backend the desktop app and the Node addon use.

By default it works on the desktop app's data. Settings, modpacks and the Quadrant ID login are shared, so a modpack made in one shows up in the other.

## Installing

Every desktop package ships `quadrantmc` next to the app.

| Package | How to run it |
|---|---|
| Debian/Ubuntu `.deb`, Fedora `.rpm`, AUR `quadrant-bin` | `quadrantmc`, installed to `/usr/bin/quadrantmc`. |
| Flatpak | `flatpak run --command=quadrantmc dev.mrquantumoff.mcmodpackmanager`. It runs inside the sandbox and works on the Flatpak app's data. |
| Windows installer | `quadrantmc`. The installer adds its install folder (`%LOCALAPPDATA%\Quadrant` by default) to your user `PATH`, so it works in terminals opened after installing. Uninstalling removes the entry. |
| Microsoft Store | `quadrantmc`, an app execution alias. Windows lists it under Settings > Apps > Advanced app settings > App execution aliases. |
| AppImage | Bundled at `usr/bin/quadrantmc` inside the image but not on `PATH`. Run `./Quadrant*.AppImage --appimage-extract` and call `squashfs-root/usr/bin/quadrantmc` by path. |
| macOS | Bundled at `Quadrant.app/Contents/MacOS/quadrantmc` but not on `PATH`. Call it by that path or link it into a folder on your `PATH`. |

## Building

From `src-tauri/`:

```sh
cargo build --release -p quadrant-cli
```

The crate is `quadrant-cli`; the binary lands in `src-tauri/target/release/quadrantmc`. The default build includes CurseForge and needs `ETERNAL_API_TOKEN` set, like the app; `--no-default-features` leaves CurseForge out and builds without it. Quadrant ID, Sync, Share and telemetry use the app's other build-time credentials (`QUADRANT_OAUTH2_CLIENT_ID`, `QUADRANT_OAUTH2_CLIENT_SECRET`, `QUADRANT_API_KEY`). A build without them still works for everything else, and those commands name the missing variable.

Packaging builds bundle it as a Tauri sidecar. `bun run build:tauri` and the release workflows pass `--config src-tauri/tauri.cli.conf.json`, whose `beforeBuildCommand` runs `scripts/build-cli.ts`. That builds `quadrantmc` for the Tauri target and copies it to `src-tauri/binaries/quadrantmc-<target triple>[.exe]`, where `bundle.externalBin` picks it up. The config also adds the Windows installer hooks in `src-tauri/windows/` that manage `PATH`. The base `tauri.conf.json` leaves all of this out, because `tauri-build` fails when an `externalBin` file is missing, which would break `cargo test`, rust-analyzer and `tauri dev`.

## Global options

| Option | Meaning |
|---|---|
| `--json` | Print the result as JSON on stdout. Progress and notes stay on stderr. |
| `-q`, `--quiet` | Hide progress and notes. Warnings about part of a command failing still show. |
| `-v`, `--verbose` | Show backend logs on stderr. Repeat for more detail. `RUST_LOG` overrides it. |
| `--data-dir DIR` | Use another data folder instead of the desktop app's (`<data dir>/dev.mrquantumoff.mcmodpackmanager`). |
| `--api-url URL` | Quadrant API base URL. Also read from `QUADRANT_API_BASE_URL`. |
| `--keyring-service NAME` | Keep the Quadrant ID login under another OS keyring service. Also read from `QUADRANT_KEYRING_SERVICE`. |

Errors print on stderr and exit with status 1. Errors the backend classifies print in the same wording the app uses.

A sandbox that must not touch real data needs three things: `--data-dir` pointing at an empty folder, the Minecraft folder moved with `settings mc-folder <scratch folder>` before any modpack command, and `--keyring-service` set, since the keyring is not inside the data folder.

## Commands

### Modpacks

```sh
quadrantmc modpack list [--query TEXT] [--include-free]
quadrantmc modpack show NAME [--no-details]
quadrantmc modpack create NAME --loader fabric [--version 1.21.1]
quadrantmc modpack edit NAME [--rename NEW] [--version V] [--loader L]
quadrantmc modpack delete NAME [--yes]
quadrantmc modpack apply NAME
quadrantmc modpack clear
quadrantmc modpack export NAME [-o PATH] [--yes]
quadrantmc modpack updates NAME [--apply]
quadrantmc modpack identify NAME
quadrantmc modpack register NAME --id ID --source modrinth --download-url URL
quadrantmc modpack share NAME
quadrantmc modpack import CODE_OR_LINK [--name NAME] [--yes]
quadrantmc modpack folder [--open]
```

`list` orders modpacks as the app does: the applied one first, then the most recently synced. `create` defaults to the latest release. `clear` applies the empty `free` modpack, as the app's clear button does. `export` writes `./NAME.quadrantExport.zip` unless `-o` says otherwise, and asks before overwriting a file that is already there. `updates` names every mod it couldn't look up or check, and exits with status 1 when there was one, after `--apply` has installed the updates it did find. `identify` matches files the modpack doesn't track and prints a `register` command for each candidate. `delete` asks first, and without a terminal to ask on it needs `--yes`. So does `import` when a local modpack already has the name it installs under, since installing over it deletes the files of its mods the imported copy lacks.

### Mods and packs

```sh
quadrantmc mod search [QUERY] [--source cf|mr]... [--type mod|resourcepack|shaderpack|modpack|datapack]
                      [--version V] [--loader L] [--category C]... [--open-source]
                      [--sort relevance|downloads|name|updated] [--offset N] [--limit N] [--modpack NAME]
quadrantmc mod info ID --source S
quadrantmc mod deps ID --source S
quadrantmc mod owners ID --source S
quadrantmc mod install ID --source S [--modpack NAME] [--version V] [--loader L]
                      [--file-id F] [--location LOC] [--with-deps]
quadrantmc mod remove MODPACK ID
quadrantmc mod update MODPACK ID
quadrantmc mod categories [--source S] [--type T]
```

`search` queries the providers enabled in settings, or only the ones named with `--source`. Results merge the way the search page merges them. Relevance interleaves each provider's ranking; the other sorts sort the union. A category only one provider has limits the search to that provider, `--open-source` limits it to Modrinth, and a loader only Modrinth supports does the same. `--modpack` makes that modpack's version and loader the defaults.

`install` fills in what the install page would. A mod goes into `--modpack`, else the last used modpack, the applied one, or the first. `--version` and `--loader` win; otherwise the install targets the modpack's own version and loader, and only a pack installed outside a modpack falls back to the last used ones or the latest release. The target prints on stderr before the download starts. Resource and shader packs go to the Minecraft folder plus any Prism instance linked to the named modpack, unless `--location` names a location from `content list`. Choices named on the command line are remembered for the next install, as the install page's pickers remember them. `--with-deps` also installs the dependencies not already in the modpack; the app never installs them on its own. Modrinth's dependency list includes optional dependencies, so it can pull in mods the mod doesn't require. A dependency that fails to install doesn't stop the others; the command names it and exits with status 1 at the end.

Loaders: `fabric`, `forge`, `neoforge`, `quilt`, `liteloader`, `babric`, `bta-babric`, `java-agent`, `legacy-fabric`, `modloader`, `nilloader`, `ornithe`, `rift`. Sources: `curseforge` (`cf`) and `modrinth` (`mr`).

### Installed content and Prism Launcher

```sh
quadrantmc content list [--no-files]
quadrantmc content copy --from LOC --to LOC --type resourcepack|shaderpack (FILE... | --all)
quadrantmc content delete --location LOC --type T FILE... [--yes]
quadrantmc content folder --location LOC --type T [--open]

quadrantmc prism list
quadrantmc prism plan MODPACK
quadrantmc prism apply MODPACK INSTANCE
quadrantmc prism detach INSTANCE
```

Locations are `minecraft` or `prism:<instance>`. Prism support is experimental in the app too; turn it on with `settings set experimentalFeatures true`.

### Quadrant ID, notifications and Sync

```sh
quadrantmc account login [--no-browser]
quadrantmc account logout
quadrantmc account info
quadrantmc account open | register

quadrantmc notifications list [--all]
quadrantmc notifications read ID
quadrantmc notifications accept ID | decline ID
quadrantmc notifications watch

quadrantmc sync list
quadrantmc sync push MODPACK [--force]
quadrantmc sync pull MODPACK_ID [--name NAME] [--yes]
quadrantmc sync members MODPACK_ID
quadrantmc sync invite MODPACK_ID USERNAME [--admin]
quadrantmc sync kick MODPACK_ID USERNAME
quadrantmc sync delete MODPACK_ID [--yes]
quadrantmc sync share MODPACK_ID
```

`account login` signs in the way the app does. It opens the browser and prints the link, then waits up to five minutes for the redirect on the first free port of `127.0.0.1:4000` to `4005`. The login lands in the OS keyring, so the desktop app is signed in too, and `account logout` signs both out.

`notifications list` hides what the app hides: modpack update notices when they are turned off, and ones for updates you made yourself. `--all` shows them. `notifications watch` prints new notifications until Ctrl+C, one JSON object per line under `--json`. It does so by running the desktop app's background workers, which change state, not only read it. Settings sync may pull the cloud settings over the local ones or push the local ones. When `autoQuadrantSync` is on, modpacks with a remote update get it applied. The notification cursor, which the desktop app shares, moves forward.

`sync push` refuses to overwrite a newer cloud copy unless `--force` is passed. `sync pull` installs the cloud copy under the name of the local modpack it is linked to, and like `modpack import` it asks before replacing a local modpack.

### Settings

```sh
quadrantmc settings list
quadrantmc settings get KEY
quadrantmc settings set KEY VALUE
quadrantmc settings unset KEY
quadrantmc settings mc-folder [PATH | --reset]
quadrantmc settings push | pull
```

`set` stores JSON literals (`true`, `100`, `{"a":1}`) as JSON and anything else as text. Keys that hold text, like `lastUsedVersion` or `mcFolder`, stay text even when the value looks like a number. Every change updates `lastSettingsUpdated`, as the app does, so settings sync treats it as the newest copy. Turning `collectUserData` on or off sends or withdraws telemetry right away, as the settings page does, so a build without `QUADRANT_API_KEY` refuses to change it. `mcFolder` and `prismLauncherFolder`, whether set with `set` or `mc-folder`, must name an existing folder and are stored as absolute paths.

### Links and everything else

```sh
quadrantmc open LINK [--modpack NAME] [--version V] [--loader L] [--name NAME] [--yes]
quadrantmc versions
quadrantmc news
quadrantmc telemetry info | send | remove
quadrantmc invoke --list
quadrantmc invoke COMMAND [JSON]
```

`open` takes the links the app registers for: `curseforge://install?addonId=…`, `modrinth://mod/<id>` (also `resourcepack`, `shader`, and wrapped `modrinth://https://modrinth.com/mod/<id>` links), `quadrantnext://modrinth|curseforge|modpack|login…`, and `https://usequadrant.dev/modpack/<code>`. Mod links install like `mod install`, modpack links import like `modpack import`, and login links finish a sign-in started by `account login`.

`invoke` calls a host command directly with a camelCase JSON payload, for anything the other commands don't cover:

```sh
quadrantmc invoke get_config_value '{"key":"mcFolder"}'
```

## Examples

```sh
quadrantmc modpack create "Survival" --loader fabric --version 1.21.1
quadrantmc mod search sodium --modpack Survival --limit 5
quadrantmc mod install AANobbMI --source modrinth --modpack Survival
quadrantmc mod install 238222 --source curseforge --modpack Survival
quadrantmc modpack updates Survival --apply
quadrantmc modpack apply Survival
quadrantmc --json modpack list | jq '.[].name'
```

## Not in the CLI

These belong to the desktop shell and have no CLI command: the app updater, windows, the tray, zoom, the UI language and decorations, file watching, and the last opened page.
