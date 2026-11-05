# STR Launcher

**Universal Skyrim Together Reborn launcher for Windows and Linux** — a single
Rust desktop app that takes you from a clean Skyrim Special Edition install to a
working multiplayer session with one click. No Vortex, no MO2, no manual file
shuffling.

![STR Launcher](assets/screenshot.png)

Built with [Iced](https://iced.rs), fully asynchronous, and modeled directly on
the official documentation:

- Mod page: <https://www.nexusmods.com/skyrimspecialedition/mods/69993>
- Official getting-started guide: <https://wiki.tiltedphoques.com/tilted-online/guides/getting-started>
- Quick setup TL;DR: <https://telegra.ph/Skyrim-Together-Quick-Setup-Vortex-TLDR-10-30>
- Linux reference: <https://github.com/Cyphs/SteamDeckSTR>

---

## What it automates

| Step | Windows | Linux (incl. Steam Deck) |
| --- | --- | --- |
| Find Skyrim SE via Steam | Registry + `libraryfolders.vdf` | `libraryfolders.vdf` (native + Flatpak Steam) **+ fallback scan of `/mnt`, `/media`, `/run/media` for libraries on other drives** |
| Download **Address Library (All in One)** | Nexus API | Nexus API |
| Download **Skyrim Together Reborn** | Nexus API | Nexus API |
| Place files (exe + `Data/` merge) | ✅ tracked by manifest | ✅ tracked by manifest |
| Remove bundled Creation Club content (Survival, Fishing, Curios, Elite Merchandise) | ✅ backed up first | ✅ backed up first |
| Enable `SkyrimTogether.esp` in `Plugins.txt` | ✅ | ✅ (every existing profile location) |
| STR finds `SkyrimSE.exe` (no picker dialog) | `HKCU\Software\TiltedPhoques\TiltedEvolution` registry keys | Proton prefix `user.reg` (`TitleExe` / `TitlePath`) |
| Correct Proton | — | GE-Proton **10** selected for Skyrim in Steam (`config.vdf`), auto-downloaded if missing |
| Launch via Steam starts multiplayer | — | Choose a **launch mode** (Settings): either launcher replacement (`SkyrimSELauncher.exe` → `SkyrimTogether.exe`, original kept as `_SkyrimSELauncher.exe`) + protonfixes localfix, **or** a separate *"Skyrim Together Reborn"* game in your Steam library with full SteamGridDB artwork (hero, grids, logo, recolored icon) |
| Shortcuts | Start Menu + Desktop `.lnk` | Application entry + Desktop `.desktop` + standalone launch script with `STEAM_COMPAT_DATA_PATH` |
| Undo / uninstall | ✅ restores everything | ✅ restores everything (either mode) |

The tool **never deletes** anything without keeping a backup: removed CC content
goes to `steamapps/str-cc-backup/`, the original launcher is renamed, and a
`.str-launcher-manifest.json` in the game folder records every file the launcher
installed so updates and uninstall are clean.

### Launch modes (Linux)

| | Replace the Skyrim launcher | Add a separate Steam game |
| --- | --- | --- |
| Starting Skyrim via Steam | launches STR directly | launches vanilla Skyrim |
| New Steam library entry | — | ✅ "Skyrim Together Reborn" with its own artwork |
| Skyrim game folder | launcher exe symlinked | 100% untouched |
| Undo | restores original launcher | removes the Steam entry + artwork |

Both modes share the same generated launch script (`launch-skyrim-together.sh`),
which drives Proton with the game's own prefix — so the in-app **Launch
Together** button, the Steam entry and the desktop shortcut behave identically.

> **Why GE-Proton 10?** GE-Proton 11 (Wine 11) closes Skyrim Together Reborn 1.8
> right after launch — the launcher pins and installs GE-Proton 10 by default
> (same choice as the SteamDeckSTR reference implementation).

---

## Steam only — GOG / Epic users read this

Automatic detection and the Steam launch integrations (Proton selection, the
launcher replacement, `steam://` launching) are **Steam-only** by design — the
mod itself is distributed for the Steam build of Skyrim Special Edition.
GOG / Epic / Heroic users can still use the installer: run *Settings → Game →
Browse…* and select the folder containing `SkyrimSE.exe`.
Everything except the Steam-specific launch paths works the same.

## Usage

1. **Have Skyrim Special Edition (Steam, v1.6+) installed** and launch it once
   through Steam (reach the main menu, then quit). Anniversary Upgrade DLC is
   *not* needed and is recommended to be disabled.
2. **Download and run STR Launcher.** It detects your install automatically.
3. Press **Install Skyrim Together Reborn**. The tool downloads, extracts,
   cleans, and configures everything. The **Launch readiness** checklist shows
   every precondition (game build, Address Library, Plugins.txt, Proton tool,
   prefix registry, launch mode) so nothing fails silently.
4. Press **Launch Together** — or start the game through Steam / the desktop
   shortcut, depending on your launch mode. In-game, press **F2** (or
   **Right Ctrl**) to open the multiplayer menu, create a party, and play.
   *Complete the Helgen intro first — see the official [play guide](https://wiki.tiltedphoques.com/tilted-online/general-information/playguide).*

### Nexus API key (one time)

Automatic downloads use the official Nexus Mods API with **your personal API
key** (Account Settings → API keys — the very bottom of the page — *Personal use*). The key is stored locally in the
app config and is only ever sent to `api.nexusmods.com`. Paste it under
Settings → Nexus Mods, or skip the network entirely:

### Offline mode

Prefer downloading manually? Grab the latest archives from Nexus
([STR](https://www.nexusmods.com/skyrimspecialedition/mods/69993?tab=files) ·
[Address Library](https://www.nexusmods.com/skyrimspecialedition/mods/32444?tab=files)),
drop them into Settings → Offline mode, and press Install. No API key
needed.

### Dedicated server (optional)

The **Server** section downloads the latest dedicated server, lets you edit
`config/STServer.ini` (port, name, password, player count, mod check) and
launches it — natively on Windows, under the game's Proton on Linux.

---

## Building

Requirements: Rust 1.77+ (stable).

```bash
# Linux (needs the usual wayland/x11 dev bits for winit; on Arch:
#   pacman -S base-devel wayland libxkbcommon libx11)
cargo build --release

# Windows (in a MSVC environment)
cargo build --release
```

The binary is self-contained; there is no installer and no runtime dependency on
Vortex, MO2, SKSE, SteamCMD, or any scripting runtime.

## Troubleshooting

- **"Skyrim Together is not running!"** — Run the launcher's *Undo*, then
  *Install* again; it re-applies the launcher replacement and protonfix.
- **Crash on connect** — Make sure bundled Creation Club content was removed
  (the launcher does this by default) and that all players run the same STR
  version (use *Check updates*).
- **Black screen in Game Mode (Steam Deck)** — This happens if the old
  `SteamGameId=302190` launch option is set; this launcher's fix avoids adding
  it. Remove any custom launch options for Skyrim in Steam.
- **Steam compatibility tool** — If Steam still uses an old Proton, pick the
  GE-Proton 10 entry manually under *Properties → Compatibility*; the launcher
  attempts this automatically (it briefly restarts Steam when needed).
- The launcher logs every step in the activity pane; the data folder is
  reachable via Settings → Open downloads.

## Project layout

```
src/
├── main.rs           Iced bootstrap
├── app/              UI state, update loop, view (mod.rs / update.rs / view.rs)
├── str_install.rs    The automated install/undo pipeline (the heart)
├── steam.rs          Steam library scanning + Skyrim detection
├── vdf.rs            VDF parsing + config.vdf CompatToolMapping surgery
├── proton.rs         GE-Proton discovery / install / selection
├── nexus.rs          Nexus Mods API v1 client
├── download.rs       Async downloader with retry + resume
├── archive.rs        ZIP / tar.gz extraction with traversal protection
├── cc_clean.rs       Creation Club backup & restore
├── plugins.rs        Plugins.txt handling
├── pe_version.rs     PE VS_VERSIONINFO parser (game/exe version display)
├── launch.rs         Launchers for game / vanilla / server
├── server.rs         Dedicated server download + STServer.ini editing
├── platform/         Windows (registry, .lnk) and Linux (proton, .desktop) code
└── ui.rs             Theme + styles
```

## Credits

- [Tilted Phoques](https://github.com/tiltedphoques) — Skyrim Together Reborn,
  and the dragon emblem used in this launcher's icon and menu
  ([TiltedEvolution skyrim_ui assets](https://github.com/tiltedphoques/TiltedEvolution))
- [Cinzel](https://fonts.google.com/specimen/Cinzel) (SIL Open Font License) —
  the carved-caps Skyrim-style heading font bundled with the launcher
- [Cyphs](https://github.com/Cyphs/SteamDeckSTR) (based on
  [pikdum](https://github.com/pikdum/steam-deck)'s scripts) — the Linux approach
  (launcher replacement, protonfixes localfix, config.vdf handling) is ported
  from and inspired by their MIT-licensed work
- [GloriousEggroll](https://github.com/GloriousEggroll/proton-ge-custom) — GE-Proton

## License

MIT — see [LICENSE](LICENSE).
