# About

Desktop app that turns off duplicate features across your Hypixel SkyBlock Fabric mods.

Runs on Windows, Linux, and macOS, including Apple silicon.

[**Latest release**](https://github.com/hxpeerr/skyblock-config-manager/releases/latest) — everything stays on your computer, and your original config files are backed up first.

<!-- Add a screenshot or collage of the app here:
![Skyblock Config Manager](path/to/screenshot.png)
-->

Includes the [desktop app](rust) for managing shared and unique mod settings, a [feature list](features.jsonc) that maps duplicate settings across mods, and [check scripts](tools/modcheck) that watch for added or renamed settings. No in-game mods or extra launcher plugins needed.

[**Download**](#download), [How to use](#how-to-use), [Supported mods](#supported-mods), [Build from source](#build-from-source), [Request a mod](https://github.com/hxpeerr/skyblock-config-manager/issues/new)

## Status

[![Release](https://img.shields.io/github/v/release/hxpeerr/skyblock-config-manager?label=release)](https://github.com/hxpeerr/skyblock-config-manager/releases/latest) [![Supported mods](https://img.shields.io/badge/supported%20mods-23-4cc4a2)](#supported-mods) [![Minecraft versions](https://img.shields.io/badge/Minecraft-1.21.5%20--%2026.3-4cc4a2)](#supported-mods)

<sub>* Supported mods: mods whose shared features are compared directly (listed in [features.jsonc](features.jsonc)). Other Hypixel SkyBlock mods found in your `config` folder still show up in the **Unique** tab so you can search and edit their settings, even though they aren’t compared with other mods.</sub>

Before changing any file, the app saves a backup of the original. Debug settings are locked so they can’t be changed by accident, and if a file has comments that won’t be kept, the app warns you before saving.

The app checks Modrinth and CurseForge for newer Fabric builds of the mods in your `mods` folder, downloads updates to the [feature list](features.jsonc) on its own so newly added mods and renamed settings work without a new release, and updates itself from the **App updates** tab.

## Download

Get the files for your system from the [latest release](https://github.com/hxpeerr/skyblock-config-manager/releases/latest):

- **Windows:** `SkyblockConfigManager.exe`, or `SkyblockConfigManager.zip` if your browser blocks `.exe` files. If “Windows protected your PC” pops up, click **More info**, then **Run anyway**.
- **macOS:** `SkyblockConfigManager-macOS.zip`. Unzip it and open `SkyblockConfigManager.app`. If macOS blocks it, open **System Settings → Privacy & Security** and click **Open Anyway**.
- **Linux:** `SkyblockConfigManager-Linux.zip`. Unzip it and run `./SkyblockConfigManager`.

## How to use

1. Close Minecraft so running mods don’t overwrite your changes.
2. Open the app. It finds your Minecraft `config` folder on its own, or click **Browse…** to pick it.
3. On the **Priorities** tab, pick the mod you want to keep for each area. You can also change any single feature to use a different mod, or turn it **Off in every mod**.
4. Click **Resolve conflicts**, then **Save changes** (`Ctrl+S` / `Cmd+S`).

You can also search, filter, and edit any setting in the **Shared** and **Unique** tabs, or pick your Minecraft version in **Overview** so setting names and mod update checks match your game.

## Supported mods

Works with 23 Fabric mods for Minecraft 1.21.5 to 26.3:

[Aaron’s Mod](https://modrinth.com/mod/aaron-mod), [Bazaar Utils](https://modrinth.com/mod/bazaar-utils), [BtrBz](https://modrinth.com/mod/btrbz), [Catharsis](https://modrinth.com/mod/catharsis), [Custom Scoreboard](https://modrinth.com/mod/skyblock-custom-scoreboard), [Devonian](https://modrinth.com/mod/devonian), [Feesh](https://modrinth.com/mod/feesh), [Firmament](https://modrinth.com/mod/firmament), [Modern Warp Menu](https://modrinth.com/mod/modern-warp-menu), [Noba](https://modrinth.com/mod/noba), [NoFrills](https://modrinth.com/mod/nofrills), [Odin](https://modrinth.com/mod/odin), [Scatha-Pro](https://modrinth.com/mod/scatha-pro), [Secret Routes](https://modrinth.com/mod/secret-routes-mod), [SkyblockAddons](https://modrinth.com/mod/skyblockaddons-unofficial), [SkyBlock Enhancements](https://modrinth.com/mod/skyblock-enhancements), [Skyblock Enhanced Storage](https://modrinth.com/mod/skyblock-enhanced-storage), [Skyblock Overhaul](https://modrinth.com/mod/skyblock-overhaul), [Skyblocker](https://modrinth.com/mod/skyblocker-liap), [SkyCubed](https://modrinth.com/mod/skycubed), [SkyHanni](https://modrinth.com/mod/skyhanni), [SkyOcean](https://modrinth.com/mod/skyocean), and [Stella](https://modrinth.com/mod/stella).

Want another mod supported? [Open an issue](https://github.com/hxpeerr/skyblock-config-manager/issues/new).

## Build from source

Requires Rust 1.85 or newer. On Linux, install `libx11-dev libxkbcommon-dev libwayland-dev` first.

```bash
git clone [https://github.com/hxpeerr/skyblock-config-manager.git](https://github.com/hxpeerr/skyblock-config-manager.git)
cd skyblock-config-manager/rust
cargo build --release --bin SkyblockConfigManager
```

Run the tests with `cargo test --no-default-features`.

## Disclaimer

This tool only reads and edits local config files already on your computer.

Skyblock Config Manager is an independent project and is not affiliated with, approved by, or endorsed by Hypixel, Mojang, Microsoft, or any of the supported mod authors. “Minecraft” is a trademark of Mojang Synergies AB.
