# Backend Compatibility Contract

This document freezes the backend-facing surface while the Rust backend is extracted out of Tauri.

## Tauri Commands That Must Stay Stable
- `get_modpacks`
- `frontend_apply_modpack`
- `delete_mod`
- `update_modpack`
- `create_modpack`
- `delete_modpack`
- `open_modpacks_folder`
- `register_mod`
- `install_modpack`
- `export_modpack`
- `set_modpack_sync_date`
- `get_news`
- `get_minecraft_folder`
- `init_config`
- `search_mods`
- `get_versions`
- `get_user_url`
- `install_mod`
- `install_remote_file`
- `identify_modpack`
- `check_mod_updates`
- `get_mod_modrinth`
- `get_mod_owners_modrinth`
- `get_mod_deps_modrinth`
- `get_mod_curseforge`
- `get_mod_owners_curseforge`
- `get_mod_deps_curseforge`
- `set_secret`
- `clear_account_token`
- `get_account_info`
- `oauth2_login`
- `oauth2_client_id`
- `read_notification`
- `get_synced_modpacks`
- `kick_member`
- `invite_member`
- `delete_synced_modpack`
- `sync_modpack`
- `answer_invite`
- `share_modpack`
- `share_modpack_raw`
- `get_quadrant_share_modpack`
- `get_quadrant_settings`
- `submit_quadrant_settings`
- `send_telemetry`
- `remove_telemetry`

## Tauri Event Names That Must Stay Stable
- `modDownloadProgress`
- `modInstallProgress`
- `modpackDownloadProgress`
- `quadrantExportProgress`
- `quadrantShareSubmission`
- `refreshNotifications`
- `refreshSyncedModpacks`
- `recheckAccountToken`
- `updateDownloadProgress`
- `disableRightClick`

## Config And Storage Files That Must Stay Stable
- `config.json`
- `updateConfig.json`
- keyring service: `dev.mrquantumoff.mcmodpackmanager`
- keyring keys: `accountToken`, `refreshToken`

## Filesystem Layout That Must Stay Stable
- Minecraft root from `config.json::mcFolder`
- modpacks directory: `<mcFolder>/modpacks`
- active mods symlink or directory: `<mcFolder>/mods`
- modpack manifest: `<mcFolder>/modpacks/<name>/modConfig.json`
- sync metadata: `<mcFolder>/modpacks/<name>/quadrantSync.json`

## Sync Metadata Shape
- `quadrantSync.json` must continue to accept legacy `{ "last_synced": <seconds> }`
- current writers may also persist `{ "last_synced": <seconds>, "modpack_id": "<id>|null" }`

## Notifications Transport Notes
- live modpack sync is carried by `/api/v3/account/notifications/get` and `/api/v3/account/notifications/ws`
- Quadrant opts into those events with `modpack_sync=true`
- `/account/info/get` remains a profile endpoint and is not the live modpack sync transport
