# Skyblock Config Manager

Running several Hypixel Skyblock mods at once usually crowds your screen with duplicates: two dungeon maps in the corner, three Diana burrow guessers drawing lines, and multiple pest alerts going off at the same time. Skyblock Config Manager reads your mod settings, finds the features that overlap, and keeps each one turned on in only the mod you choose.

* **247 shared features** in 23 mods, across 16 parts of the game, from Dungeons and Kuudra to the Auction House, Bazaar and item tooltips.
* **You pick which mod does what:** choose a main mod for each area, make exceptions for single features, or turn a feature off in every mod.
* **Safe saving:** nothing changes until you click **Save changes**, and your original files are backed up first.
* **Edit any setting** from your SkyBlock mods, not just the shared ones. Debug settings are locked.
* **Works with your game version:** pick your Minecraft version and the app uses the setting names your mods have in it.
* **Finds other SkyBlock mods** and their settings on its own, even if they aren't supported yet.
* **Stays up to date:** checks Modrinth and CurseForge for newer versions of your mods, downloads new mods and renamed settings without a new release, and updates itself from the **App updates** tab.

## Download

Get these from the [latest release](../../releases/latest):

| System | File | First time |
|---|---|---|
| Windows 10 or 11 | `SkyblockConfigManager.exe`, or the `.zip` if your browser blocks the .exe | If "Windows protected your PC" shows up, click **More info**, then **Run anyway**. |
| macOS 10.13 or newer | `SkyblockConfigManager-macOS.zip` | Unzip it, right-click the app and choose **Open**. If macOS still won't open it, click **Open Anyway** in **System Settings → Privacy & Security**. |
| Linux | `SkyblockConfigManager-Linux.zip` | Unzip it and run `./SkyblockConfigManager`. |

## How to Use It

1. **Close Minecraft:** the game saves its settings when it closes, which would undo your changes.
2. **Open the app:** it finds your `config` folder on its own. If it doesn't, click **Browse…** and pick it.
3. **Overview tab:** pick your **Minecraft version**. **Check for mod updates** shows newer versions of your mods.
4. **Priorities tab:** for each area, pick a **Prioritized mod**. Its features stay on, and the same features in your other mods are turned off. To change a single feature, pick another mod (or **Off in every mod**) in its **Use** column.
5. **Shared and Unique tabs:** look over your settings, and double-click one to change it.
6. **Save:** click **Resolve conflicts** to apply your choices, then **Save changes** (or press `Ctrl+S`).

To update the app, open the **App updates** tab. It shows the latest release on GitHub: click **Update to …**, then **Restart now**.

## Supported Mods

| Mod | Config File |
|---|---|
| [NoFrills](https://modrinth.com/mod/nofrills) | `NoFrills/Configuration.json` |
| [SkyHanni](https://modrinth.com/mod/skyhanni) | `skyhanni/config.json` |
| [Skyblocker](https://modrinth.com/mod/skyblocker-liap) | `skyblocker.json` |
| [Aaron's Mod](https://modrinth.com/mod/aaron-mod) | `aaron-mod.json` |
| [SkyblockAddons](https://modrinth.com/mod/skyblockaddons-unofficial) | `skyblockaddons/configurations.json` |
| [SkyOcean](https://modrinth.com/mod/skyocean) | `skyocean/config.jsonc` |
| [Firmament](https://modrinth.com/mod/firmament) | `firmament/` |
| [Odin](https://modrinth.com/mod/odin) | `odin/odin-config.json` |
| [Skyblock Overhaul (SBO)](https://modrinth.com/mod/skyblock-overhaul) | `sbo/config.jsonc` |
| [Noba](https://modrinth.com/mod/noba) | `noba/config.json` |
| [Custom Scoreboard](https://modrinth.com/mod/skyblock-custom-scoreboard) | `customscoreboard/config.jsonc` |
| [Catharsis](https://modrinth.com/mod/catharsis) | `catharsis/pack_configs.json` |
| [Devonian](https://modrinth.com/mod/devonian) | `devonianConfig.json` |
| [Stella](https://modrinth.com/mod/stella) | `stella/settings.json` |
| [Feesh](https://modrinth.com/mod/feesh) | `feesh/config.jsonc` |
| [SkyBlock Enhancements](https://modrinth.com/mod/skyblock-enhancements) | `skyblock_enhancements.json` |
| [Skyblock Enhanced Storage](https://modrinth.com/mod/skyblock-enhanced-storage) | `enhanced_storage.json` |
| [SkyCubed](https://modrinth.com/mod/skycubed) | `skycubed/config.jsonc` |
| [BtrBz](https://modrinth.com/mod/btrbz) | `btrbz.json` |
| [Bazaar Utils](https://modrinth.com/mod/bazaar-utils) | `bazaarutils.json` |
| [Modern Warp Menu](https://modrinth.com/mod/modern-warp-menu) | `modernwarpmenu.json` |
| [Secret Routes](https://modrinth.com/mod/secret-routes-mod) | `SecretRoutes/xyz.yourboykyle.secretroutes.config.json` |
| [Scatha-Pro](https://modrinth.com/mod/scatha-pro) | `scathapro/configV2.json` |

These are the Fabric versions of each mod, for Minecraft 1.21.5 to 26.3. Config files are read from your `.minecraft/config` folder.

Using a mod that isn't supported? [Open an issue](../../issues/new) with a link to its Modrinth, CurseForge, or GitHub page.

## Backups

The first time a file is saved, the original is copied next to it as `<name>.bak_<date>_<time>`. To undo your changes, delete the updated file and remove `.bak_<date>_<time>` from the backup's name.

## Troubleshooting

* **A mod says "not found":** make sure you opened the right `config` folder.
* **Every mod says "not installed":** the app reads mod versions from the `mods` folder next to your `config` folder.
* **A file wasn't saved:** Minecraft was probably still open. Close it, click **Reload**, and make your changes again.
* **Your antivirus flags the .exe:** it's a false positive. The .exe is built by GitHub from the source code in this repository, and the release page shows each file's SHA-256 so you can check that you have the same file.
* **Something else went wrong:** the **Activity** log on the Overview tab has the details. Include them when you [open an issue](../../issues/new).
