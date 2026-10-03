<!-- @format -->

<p align="center">
  <img src="public/logoNoBg.svg" alt="Quadrant logo" width="128" height="128"/>
</p>

<h1 align="center">Quadrant for Minecraft</h1>

<p align="center">
  Manage your Minecraft: Java Edition mods, modpacks, resource packs and shaders with ease.
  <br/>
  Built with Tauri, React and Rust.
</p>

<p align="center">
  <a href="https://usequadrant.dev">Website</a> ·
  <a href="https://github.com/QuadrantMC/quadrant/releases/latest">Latest release</a> ·
  <a href="docs/cli.md">CLI docs</a> ·
  <a href="https://github.com/QuadrantMC/quadrant/issues">Report a bug</a> ·
  <a href="https://status.bultek.com.ua/status/main">Service status</a>
</p>

<p align="center">
  <img src="Screenshots/1.png" alt="Quadrant's modpack list" width="800"/>
</p>

## Features

- **Modpacks**: create, apply, clear, import and export modpacks for Forge, Fabric and other loaders.
- **Search and install**: browse mods, resource packs and shaders from Modrinth and CurseForge, with dependencies installed alongside.
- **Updates**: update the mods in a modpack from Modrinth and CurseForge.
- **Installed content**: view, copy and delete the resource packs and shaders in your Minecraft folder.
- **Quadrant ID**: share modpacks with friends via Quadrant Share, back them up and collaborate on them with Quadrant Sync, and sync your settings across devices.
- **Prism Launcher** (experimental): apply modpacks to Prism Launcher instances.
- **`quadrantmc` CLI**: everything above from a terminal, installed alongside the app. See [docs/cli.md](docs/cli.md).
- Available in English, Turkish and Ukrainian.

## Installation

<table align="center">
  <tr>
    <td valign="middle"><a href="https://flathub.org/apps/details/dev.mrquantumoff.mcmodpackmanager"><picture><source media="(prefers-color-scheme: dark)" srcset="https://flathub.org/api/badge?svg"/><img height="60" alt="Download on Flathub" src="https://flathub.org/api/badge?svg&amp;light"/></picture></a></td>
    <td valign="middle"><a href="https://apps.microsoft.com/detail/9nlt70m0tvd0"><picture><source media="(prefers-color-scheme: dark)" srcset="https://get.microsoft.com/images/en-us%20dark.svg"/><img height="60" alt="Download on Microsoft Store" src="https://get.microsoft.com/images/en-us%20light.svg"/></picture></a></td>
    <td valign="middle"><a href="https://aur.archlinux.org/packages/quadrant-bin"><img height="46" alt="Get it on AUR" src="https://img.shields.io/aur/version/quadrant-bin?style=for-the-badge&logo=archlinux&label=Get%20it%20on%20AUR"/></a></td>
  </tr>
</table>

You can also [download a build manually](https://github.com/QuadrantMC/quadrant/releases/latest) for Linux and Windows (x86_64/aarch64) or macOS (Apple Silicon).

> [!NOTE]
>
> On macOS, approve the app in **Privacy & Security** before the first launch, and choose **Always allow** when it asks to use the keychain. Quadrant ID and Sync need keychain access.

## Screenshots

<table>
  <tr>
    <td><img src="Screenshots/2.png" alt="Searching for shaders on Modrinth and CurseForge"/></td>
    <td><img src="Screenshots/4.png" alt="Installing a mod into a modpack"/></td>
  </tr>
  <tr>
    <td align="center">Search Modrinth and CurseForge</td>
    <td align="center">Install mods with their dependencies</td>
  </tr>
  <tr>
    <td><img src="Screenshots/3.png" alt="Managing installed resource packs and shaders"/></td>
    <td><img src="Screenshots/1.png" alt="Modpacks synced with Quadrant Sync"/></td>
  </tr>
  <tr>
    <td align="center">Manage installed resource packs and shaders</td>
    <td align="center">Apply, share and sync modpacks</td>
  </tr>
</table>

## Troubleshooting

- **A modpack fails to apply right after installing Quadrant**: delete your `mods` folder and try again.
- **Quadrant ID, Share or Sync don't work**: make sure your system clock is synced, then check the [status page](https://status.bultek.com.ua/status/main).
- **Something doesn't work on Windows**: try enabling developer mode in system settings, or reinstall the app from the Microsoft Store.
- **Requesting deletion of your data**: update to the latest version first. See the [privacy policy](PRIVACY_POLICY.md) for what the app collects.

## Development

See [DEVELOP.md](DEVELOP.md) to set up and build the app, and [TESTING.md](TESTING.md) for running the tests. The backend lives in a Tauri-independent Rust crate, documented in [docs/quadrant-core.md](docs/quadrant-core.md).

## License

Quadrant is licensed under the [Mozilla Public License 2.0](LICENSE). Using Quadrant ID is subject to the [Quadrant ID terms of service](QUADRANT-ID-TOS.md).

Quadrant and its developer are not affiliated with Mojang Studios or Microsoft.
