//! Linux launch integration, applied as step 8 of the install pipeline:
//! GE-Proton install + selection in Steam, launcher replacement (so launching
//! through Steam starts multiplayer), protonfixes, Proton-prefix registry keys
//! and a standalone launch script + desktop entry.
//!
//! Ported from the MIT-licensed SteamDeckSTR project.

use std::path::{Path, PathBuf};

use super::{Params, Reporter, ThrottledProgress};
use crate::config::LaunchMode;
use crate::steam::SKYRIM_APPID;

/// Configure everything Linux-specific for a freshly installed game.
pub(super) async fn configure_linux(
    params: &Params,
    rep: &Reporter,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let game_dir = &params.game_dir;
    let steam_root = resolve_steam_root(params);
    apply_proton_tool(params, rep, steam_root.as_deref(), warnings).await;
    write_prefix_registry(game_dir, rep, warnings).await;

    // Launch script (shared by the Launch button, desktop entries and —
    // in Steam-game mode — the Steam library entry itself).
    let script = write_launch_script(params, rep).await?;

    // Launch integration, exactly one of the two modes.
    match params.launch_mode {
        LaunchMode::LauncherReplacement => {
            // Clean up a possible leftover Steam entry from the other mode.
            crate::steam_entry::remove(None, rep).await;
            setup_launcher_replacement(game_dir, rep, warnings).await;
            install_protonfix(game_dir, rep).await;
        }
        LaunchMode::SteamGame => {
            restore_launcher(game_dir, rep).await;
            let plan = crate::steam_entry::plan_for(game_dir, Some(script.clone()));
            crate::steam_entry::ensure(&plan, rep, warnings).await;
        }
    }

    // Desktop entries (both modes — always a handy shortcut).
    write_desktop_entries(&script, rep, warnings).await;

    Ok(())
}

/// The Steam root that owns the library holding `game_dir` (the detected one,
/// or the first root with a usable config).
fn resolve_steam_root(params: &Params) -> Option<PathBuf> {
    params.steam_root.clone().or_else(|| {
        crate::steam::steam_roots().into_iter().find(|r| {
            params.game_dir.starts_with(r) || r.join("config/config.vdf").is_file()
        })
    })
}

/// Make sure the selected Proton tool exists (downloading GE-Proton when
/// missing) and select it for Skyrim in Steam.
///
/// `config.vdf` must be edited while Steam is not running, so Steam is
/// restarted around the change. (Skipped in test mode: never touch a real
/// Steam session.)
async fn apply_proton_tool(
    params: &Params,
    rep: &Reporter,
    steam_root: Option<&Path>,
    warnings: &mut Vec<String>,
) {
    use crate::platform::linux as plat;

    let Some(root) = steam_root else {
        warnings.push(
            "No Steam installation root was found; the compatibility tool could not be selected \
             automatically."
                .into(),
        );
        return;
    };

    // 1. Make sure the Proton tool exists.
    let mut prog =
        ThrottledProgress::new(rep.sender(), format!("Downloading {}", params.proton_tool));
    let roots = [root.to_path_buf()];
    let ensure = crate::proton::ensure_tool(
        &roots,
        &params.proton_tool,
        &params.dirs.staging,
        &mut |d, t| prog.send(d, t),
    )
    .await;
    match ensure {
        Ok(proton_path) => {
            rep.log(format!(
                "Using compatibility tool {} ({})",
                params.proton_tool,
                proton_path.display()
            ))
            .await;
        }
        Err(e) => warnings.push(format!("Proton setup failed: {e}")),
    }

    // 2. Select it for Skyrim in Steam. Steam is never force-restarted: while
    //    the client runs, the change is deferred and the UI shows a
    //    "Restart Steam" confirmation (see Message::RestartSteamForCompatTool).
    let interactive = std::env::var("STR_LAUNCHER_TEST_MODE").is_err();
    let needs_change = crate::proton::current_compat_tool(root, SKYRIM_APPID)
        .as_deref()
        != Some(params.proton_tool.as_str());
    if needs_change && !interactive {
        rep.log("Test mode: skipping Steam compatibility-tool selection.".to_string()).await;
    } else if needs_change && plat::is_steam_running() {
        rep.log(format!(
            "Steam is running — the {} selection needs Steam to restart. \
             Press RESTART STEAM in the banner when you're ready.",
            params.proton_tool
        ))
        .await;
        let _ = rep
            .sender()
            .send(super::Event::NeedsSteamRestart {
                steam_root: root.to_path_buf(),
                tool: params.proton_tool.clone(),
            })
            .await;
    } else if needs_change {
        rep.log(format!("Selecting {} for Skyrim in Steam...", params.proton_tool)).await;
        match crate::proton::select_compat_tool(root, SKYRIM_APPID, &params.proton_tool) {
            Ok(_) => {
                rep.log(format!("Selected {} for Skyrim.", params.proton_tool)).await;
            }
            Err(e) => warnings.push(format!(
                "Could not select {} in Steam ({e}). Pick it manually: \
                 Steam → Skyrim → Properties → Compatibility.",
                params.proton_tool
            )),
        }
    } else {
        rep.log(format!("Steam already uses {} for Skyrim.", params.proton_tool)).await;
    }
}

/// Proton prefix registry: let STR find SkyrimSE.exe inside the prefix.
async fn write_prefix_registry(game_dir: &Path, rep: &Reporter, warnings: &mut Vec<String>) {
    use crate::platform::linux as plat;

    let Some(compat) = crate::steam::compat_dir_for(game_dir) else {
        return;
    };
    let user_reg = compat.join("pfx/user.reg");
    if !user_reg.is_file() {
        warnings.push(
            "The Proton prefix does not exist yet — run the game once through Steam, then \
             press Install again to finish configuration."
                .into(),
        );
        return;
    }
    let z_dir = plat::wine_z_path(game_dir);
    let section = format!(
        "[Software\\\\TiltedPhoques\\\\TiltedEvolution\\\\Skyrim Special Edition] {}",
        crate::paths::unix_now()
    );
    let values = vec![
        format!("\"TitleExe\"=\"{z_dir}\\\\SkyrimSE.exe\""),
        format!("\"TitlePath\"=\"{z_dir}\""),
    ];
    match plat::append_user_reg_if_missing(&user_reg, &section, &values) {
        Ok(true) => rep.log("Proton prefix registry updated (STR will find SkyrimSE.exe).").await,
        Ok(false) => rep.log("Proton prefix registry already configured.").await,
        Err(e) => warnings.push(format!(
            "Could not write the Proton registry keys ({e}). On first launch, \
             SkyrimTogether may ask you to locate SkyrimSE.exe."
        )),
    }
}

/// Generate the launch script through `str-launch` so the app's Launch button,
/// the Steam entry and the desktop shortcuts all run the exact same script.
async fn write_launch_script(params: &Params, rep: &Reporter) -> Result<PathBuf, String> {
    let ctx = crate::launch::LaunchContext {
        game_dir: params.game_dir.clone(),
        steam_root: params.steam_root.clone(),
        proton_tool: params.proton_tool.clone(),
        dirs: params.dirs.clone(),
    };
    let script_path = crate::launch::ensure_launch_script(&ctx)?;
    rep.log(format!("Launch script written to {}", script_path.display())).await;
    Ok(script_path)
}

/// Undo the launcher replacement: restore the original `SkyrimSELauncher.exe`.
pub(super) async fn restore_launcher(game_dir: &Path, rep: &Reporter) {
    let launcher = game_dir.join("SkyrimSELauncher.exe");
    let backup = game_dir.join("_SkyrimSELauncher.exe");
    if launcher.is_symlink() {
        let _ = std::fs::remove_file(&launcher);
    }
    if backup.is_file() && !launcher.exists() {
        match std::fs::rename(&backup, &launcher) {
            Ok(()) => rep.log("Original SkyrimSELauncher.exe restored.").await,
            Err(e) => rep.log(format!("restoring SkyrimSELauncher.exe failed: {e}")).await,
        }
    }
    let _ = std::fs::remove_file(
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".config/protonfixes/localfixes/489830.py"),
    );
}

/// Symlink `SkyrimSELauncher.exe` → `SkyrimTogether.exe` so starting Skyrim
/// through Steam begins multiplayer (the original is kept as a backup).
async fn setup_launcher_replacement(game_dir: &Path, rep: &Reporter, warnings: &mut Vec<String>) {
    let launcher = game_dir.join("SkyrimSELauncher.exe");
    let backup = game_dir.join("_SkyrimSELauncher.exe");
    let together = game_dir.join("SkyrimTogether.exe");
    if !together.is_file() {
        warnings.push("SkyrimTogether.exe missing after install — launcher not replaced.".into());
        return;
    }
    if launcher.is_symlink() {
        // Already set up.
        rep.log("Launcher replacement already in place.").await;
        return;
    }
    if launcher.is_file() {
        if backup.exists() {
            let _ = std::fs::remove_file(&launcher);
        } else if let Err(e) = std::fs::rename(&launcher, &backup) {
            warnings.push(format!("renaming SkyrimSELauncher.exe failed: {e}"));
            return;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        match symlink("SkyrimTogether.exe", &launcher) {
            Ok(()) => rep.log(
                "SkyrimSELauncher.exe now launches Skyrim Together (original kept as _SkyrimSELauncher.exe).",
            ).await,
            Err(e) => warnings.push(format!(
                "Could not link the launcher ({e}). Start multiplayer with the desktop entry or \
                 by running SkyrimTogether.exe directly."
            )),
        }
    }
}

/// Local protonfixes fix for app 489830: keep GE-Proton from launching
/// skse64_loader.exe instead of our STR launcher when SKSE is installed.
/// (Ported from the SteamDeckSTR project, MIT licensed.)
async fn install_protonfix(game_dir: &Path, rep: &Reporter) {
    let home = match std::env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => return,
    };
    let mut targets = vec![home.join(".config/protonfixes/localfixes/489830.py")];
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            targets.push(PathBuf::from(xdg).join("protonfixes/localfixes/489830.py"));
        }
    }
    let content = PROTONFIX_PY;
    let mut wrote = false;
    for target in &targets {
        if let Some(parent) = target.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                continue;
            }
        }
        // Only overwrite when the launcher replacement is actually active for
        // this game, mirroring the reference behaviour.
        let is_str_launcher = std::fs::symlink_metadata(game_dir.join("SkyrimSELauncher.exe"))
            .map(|m| m.is_symlink())
            .unwrap_or(false);
        if !is_str_launcher {
            continue;
        }
        if std::fs::write(target, content).is_ok() {
            wrote = true;
        }
    }
    if wrote {
        rep.log("Installed the GE-Proton localfix for Skyrim (489830).").await;
    }
}

const PROTONFIX_PY: &str = r#""""Skyrim Special Edition fix for GE-Proton, installed by STR Launcher.

A local fix in ~/.config/protonfixes/localfixes replaces GE-Proton's own one. It does the
same thing (start skse64_loader.exe instead of the launcher when SKSE is installed),
except while Skyrim Together Reborn is set up: then SkyrimSELauncher.exe is STR's
launcher, which loads SKSE itself, and starting SKSE's loader would skip STR.
Ported from the SteamDeckSTR project (MIT).
"""

import os

from protonfixes import util


def main() -> None:
    # Mod Organizer 2 has its own redirector
    if os.path.exists('modorganizer2'):
        return

    # STR Post-Deploy links SkyrimSELauncher.exe to SkyrimTogether.exe
    launcher = 'SkyrimSELauncher.exe'
    if os.path.islink(launcher) and os.path.realpath(launcher).lower().endswith('skyrimtogether.exe'):
        return

    if os.path.isfile('skse64_loader.exe'):
        util.replace_command(launcher, 'skse64_loader.exe')
"#;

/// Write the XDG application entry plus Desktop copies, using the bundled
/// dragon emblem as the icon. The entry runs the shared launch `script`.
async fn write_desktop_entries(script: &Path, rep: &Reporter, warnings: &mut Vec<String>) {
    use crate::platform::linux as plat;

    let exec = format!("\"{}\"", script.display());
    let name = "Skyrim Together Reborn";
    let comment = "Play Skyrim Special Edition with Skyrim Together Reborn (via STR Launcher)";
    let icon_dir = std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".local/share/icons"))
        .ok();
    let icon_name = "str-launcher.png";
    let mut icon = "applications-games".to_string();
    if let Some(icon_dir) = icon_dir {
        let icon_path = icon_dir.join(icon_name);
        if std::fs::write(&icon_path, crate::LOGO_ICON_PNG).is_ok() {
            icon = icon_path.display().to_string();
        }
    }
    let entry_path = plat::desktop_entry_dir().join("str-launcher-skyrim-together.desktop");
    if let Err(e) = plat::write_desktop_entry(&entry_path, name, comment, &exec, &icon, false) {
        warnings.push(format!("Desktop entry failed: {e}"));
        return;
    }
    rep.log(format!("Application entry: {}", entry_path.display())).await;
    for desk in plat::desktop_dirs() {
        let target = desk.join("str-launcher-skyrim-together.desktop");
        match std::fs::copy(&entry_path, &target) {
            Ok(_) => rep.log(format!("Desktop shortcut: {}", target.display())).await,
            Err(e) => warnings.push(format!("Desktop shortcut failed ({}): {e}", target.display())),
        }
    }
}
