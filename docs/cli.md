# quadrant-cli

`quadrant-cli` does from a terminal what the desktop app does: modpacks, mod search and installs, resource and shader packs, Prism Launcher links, Quadrant ID, Sync, Share and settings. It is a thin layer over `quadrant-host`, the same backend the desktop app and the Node addon use.

By default it works on the desktop app's data. Settings, modpacks and the Quadrant ID login are shared, so a modpack made in one shows up in the other.

## Building

From `src-tauri/`:

```sh
cargo build --release -p quadrant-cli
```

The binary lands in `src-tauri/target/release/quadrant-cli`. The default build includes CurseForge and needs `ETERNAL_API_TOKEN` set, like the app; `--no-default-features` leaves CurseForge out and builds without it. Quadrant ID, Sync, Share and telemetry use the app's other build-time credentials (`QUADRANT_OAUTH2_CLIENT_ID`, `QUADRANT_OAUTH2_CLIENT_SECRET`, `QUADRANT_API_KEY`). A build without them still works for everything else, and those commands name the missing variable.

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
quadrant-cli modpack list [--query TEXT] [--include-free]
quadrant-cli modpack show NAME [--no-details]
quadrant-cli modpack create NAME --loader fabric [--version 1.21.1]
quadrant-cli modpack edit NAME [--rename NEW] [--version V] [--loader L]
quadrant-cli modpack delete NAME [--yes]
quadrant-cli modpack apply NAME
quadrant-cli modpack clear
quadrant-cli modpack export NAME [-o PATH]
quadrant-cli modpack updates NAME [--apply]
quadrant-cli modpack identify NAME
quadrant-cli modpack register NAME --id ID --source modrinth --download-url URL
quadrant-cli modpack share NAME
quadrant-cli modpack import CODE_OR_LINK [--name NAME]
quadrant-cli modpack folder [--open]
```

`list` orders modpacks as the app does: the applied one first, then the most recently synced. `create` defaults to the latest release. `clear` applies the empty `free` modpack, as the app's clear button does. `export` writes `./NAME.quadrantExport.zip` unless `-o` says otherwise. `updates` names every mod it couldn't look up or check, and exits with status 1 when there was one, after `--apply` has installed the updates it did find. `identify` matches files the modpack doesn't track and prints a `register` command for each candidate. `delete` asks first, and without a terminal to ask on it needs `--yes`.

### Mods and packs

```sh
quadrant-cli mod search [QUERY] [--source cf|mr]... [--type mod|resourcepack|shaderpack|modpack|datapack]
                        [--version V] [--loader L] [--category C]... [--open-source]
                        [--sort relevance|downloads|name|updated] [--offset N] [--limit N] [--modpack NAME]
quadrant-cli mod info ID --source S
quadrant-cli mod deps ID --source S
quadrant-cli mod owners ID --source S
quadrant-cli mod install ID --source S [--modpack NAME] [--version V] [--loader L]
                        [--file-id F] [--location LOC] [--with-deps]
quadrant-cli mod remove MODPACK ID
quadrant-cli mod update MODPACK ID
quadrant-cli mod categories [--source S] [--type T]
```

`search` queries the providers enabled in settings, or only the ones named with `--source`. Results merge the way the search page merges them. Relevance interleaves each provider's ranking; the other sorts sort the union. A category only one provider has limits the search to that provider, `--open-source` limits it to Modrinth, and a loader only Modrinth supports does the same. `--modpack` makes that modpack's version and loader the defaults.

`install` fills in what the install page would. A mod goes into `--modpack`, else the last used modpack, the applied one, or the first. `--version` and `--loader` win; otherwise the install targets the modpack's own version and loader, and only a pack installed outside a modpack falls back to the last used ones or the latest release. The target prints on stderr before the download starts. Resource and shader packs go to the Minecraft folder plus any Prism instance linked to the named modpack, unless `--location` names a location from `content list`. Choices named on the command line are remembered for the next install, as the install page's pickers remember them. `--with-deps` also installs the dependencies not already in the modpack; the app never installs them on its own. Modrinth's dependency list includes optional dependencies, so it can pull in mods the mod doesn't require. A dependency that fails to install doesn't stop the others; the command names it and exits with status 1 at the end.

Loaders: `fabric`, `forge`, `neoforge`, `quilt`, `liteloader`, `babric`, `bta-babric`, `java-agent`, `legacy-fabric`, `modloader`, `nilloader`, `ornithe`, `rift`. Sources: `curseforge` (`cf`) and `modrinth` (`mr`).

### Installed content and Prism Launcher

```sh
quadrant-cli content list [--no-files]
quadrant-cli content copy --from LOC --to LOC --type resourcepack|shaderpack (FILE... | --all)
quadrant-cli content delete --location LOC --type T FILE... [--yes]
quadrant-cli content folder --location LOC --type T [--open]

quadrant-cli prism list
quadrant-cli prism plan MODPACK
quadrant-cli prism apply MODPACK INSTANCE
quadrant-cli prism detach INSTANCE
```

Locations are `minecraft` or `prism:<instance>`. Prism support is experimental in the app too; turn it on with `settings set experimentalFeatures true`.

### Quadrant ID, notifications and Sync

```sh
quadrant-cli account login [--no-browser]
quadrant-cli account logout
quadrant-cli account info
quadrant-cli account open | register

quadrant-cli notifications list [--all]
quadrant-cli notifications read ID
quadrant-cli notifications accept ID | decline ID
quadrant-cli notifications watch

quadrant-cli sync list
quadrant-cli sync push MODPACK [--force]
quadrant-cli sync pull MODPACK_ID [--name NAME]
quadrant-cli sync members MODPACK_ID
quadrant-cli sync invite MODPACK_ID USERNAME [--admin]
quadrant-cli sync kick MODPACK_ID USERNAME
quadrant-cli sync delete MODPACK_ID [--yes]
quadrant-cli sync share MODPACK_ID
```

`account login` signs in the way the app does. It opens the browser and prints the link, then waits up to five minutes for the redirect on the first free port of `127.0.0.1:4000` to `4005`. The login lands in the OS keyring, so the desktop app is signed in too, and `account logout` signs both out.

`notifications list` hides what the app hides: modpack update notices when they are turned off, and ones for updates you made yourself. `--all` shows them. `notifications watch` runs the app's background workers, the same notification stream and settings sync the app runs, and prints new notifications until Ctrl+C.

`sync push` refuses to overwrite a newer cloud copy unless `--force` is passed. `sync pull` installs the cloud copy under the name of the local modpack it is linked to.

### Settings

```sh
quadrant-cli settings list
quadrant-cli settings get KEY
quadrant-cli settings set KEY VALUE
quadrant-cli settings unset KEY
quadrant-cli settings mc-folder [PATH | --reset]
quadrant-cli settings push | pull
```

`set` stores JSON literals (`true`, `100`, `{"a":1}`) as JSON and anything else as text. Keys that hold text, like `lastUsedVersion` or `mcFolder`, stay text even when the value looks like a number. Every change updates `lastSettingsUpdated`, as the app does, so settings sync treats it as the newest copy. Turning `collectUserData` on or off sends or withdraws telemetry right away, as the settings page does.

### Links and everything else

```sh
quadrant-cli open LINK [--modpack NAME] [--version V] [--loader L] [--name NAME]
quadrant-cli versions
quadrant-cli news
quadrant-cli telemetry info | send | remove
quadrant-cli invoke --list
quadrant-cli invoke COMMAND [JSON]
```

`open` takes the links the app registers for: `curseforge://install?addonId=…`, `modrinth://mod/<id>` (also `resourcepack`, `shader`, and wrapped `modrinth://https://modrinth.com/mod/<id>` links), `quadrantnext://modrinth|curseforge|modpack|login…`, and `https://usequadrant.dev/modpack/<code>`. Mod links install like `mod install`, modpack links import like `modpack import`, and login links finish a sign-in started by `account login`.

`invoke` calls a host command directly with a camelCase JSON payload, for anything the other commands don't cover:

```sh
quadrant-cli invoke get_config_value '{"key":"mcFolder"}'
```

## Examples

```sh
quadrant-cli modpack create "Survival" --loader fabric --version 1.21.1
quadrant-cli mod search sodium --modpack Survival --limit 5
quadrant-cli mod install AANobbMI --source modrinth --modpack Survival
quadrant-cli mod install 238222 --source curseforge --modpack Survival
quadrant-cli modpack updates Survival --apply
quadrant-cli modpack apply Survival
quadrant-cli --json modpack list | jq '.[].name'
```

## Not in the CLI

These belong to the desktop shell and have no CLI command: the app updater, windows, the tray, zoom, the UI language and decorations, file watching, and the last opened page.
