//! Headless CLI mode: run the same install / launch / server pipelines from a
//! terminal, without the GUI.
//!
//! `str-launcher --headless install|launch|server [options]` reuses the exact
//! pipeline the GUI drives (same Params, same manifest/Undo accounting), which
//! lets CI exercise the real install path with fixture archives. Every event
//! is printed to stdout; the exit code is 0 on success, 1 on pipeline failure
//! and 2 on bad usage.
//!
//! Note: on Windows the app is a GUI-subsystem binary, so `--headless` prints
//! nothing when double-clicked — run it from a terminal.

use std::path::PathBuf;

use crate::str_install::Event;

pub const USAGE: &str = "\
Usage: str-launcher --headless <install|launch|server> [options]

Commands:
  install    Detect Skyrim, download (or use offline archives) and install
             Skyrim Together Reborn with the full platform setup.
  launch     Launch Skyrim Together with the configured compatibility tool.
  server     Download and install the dedicated server (Nexus, falling back
             to the official GitHub release).

Options:
  --game-dir <PATH>        Skyrim Special Edition folder containing SkyrimSE.exe
                           (default: auto-detect, else the saved profile)
  --profile <NAME>         Config profile to use (default: the active one)
  --api-key <KEY>          Nexus API key (not needed with offline archives)
  --offline-str <ZIP>      Skyrim Together Reborn archive for offline install
  --offline-al <ZIP>       Address Library archive for offline install
  --offline-server <ZIP>   Server archive for offline use
  --proton <NAME>          Compatibility tool override (Linux)
  -h, --help               Show this help";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Command {
    #[default]
    Install,
    Launch,
    Server,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    pub command: Command,
    pub game_dir: Option<PathBuf>,
    pub profile: Option<String>,
    pub api_key: Option<String>,
    pub offline_str: Option<PathBuf>,
    pub offline_al: Option<PathBuf>,
    pub offline_server: Option<PathBuf>,
    pub proton: Option<String>,
}

/// Parse the arguments after `--headless`. Pure and unit-tested; errors carry
/// a short reason (usage is printed by the caller).
pub fn parse_args<I: Iterator<Item = String>>(args: I) -> Result<Args, String> {
    let mut out = Args::default();
    let mut items = args.peekable();
    let command = items
        .next()
        .ok_or("missing command (install|launch|server)")?;
    out.command = match command.as_str() {
        "install" => Command::Install,
        "launch" => Command::Launch,
        "server" => Command::Server,
        other => return Err(format!("unknown command \"{other}\" (expected install|launch|server)")),
    };
    let value_flag = |name: &str| {
        format!(
            "--{name} needs a value (e.g. --{name} /path/to/thing)"
        )
    };
    while let Some(flag) = items.next() {
        let mut take_value = |name: &str| -> Result<String, String> {
            items
                .next()
                .filter(|v| !v.starts_with("--"))
                .ok_or_else(|| value_flag(name))
        };
        match flag.as_str() {
            "--game-dir" => out.game_dir = Some(PathBuf::from(take_value("game-dir")?)),
            "--profile" => out.profile = Some(take_value("profile")?),
            "--api-key" => out.api_key = Some(take_value("api-key")?),
            "--offline-str" => out.offline_str = Some(PathBuf::from(take_value("offline-str")?)),
            "--offline-al" => out.offline_al = Some(PathBuf::from(take_value("offline-al")?)),
            "--offline-server" => {
                out.offline_server = Some(PathBuf::from(take_value("offline-server")?))
            }
            "--proton" => out.proton = Some(take_value("proton")?),
            other => return Err(format!("unknown option \"{other}\"")),
        }
    }
    Ok(out)
}

/// Run the headless flow if invoked with `--headless`; None means "not a
/// headless invocation, start the GUI". Returns the process exit code.
pub fn maybe_run<I: Iterator<Item = String>>(raw: I) -> Option<i32> {
    let all: Vec<String> = raw.collect();
    if all.first().map(String::as_str) != Some("--headless") {
        return None;
    }
    let rest = &all[1..];
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Some(0);
    }
    let args = match parse_args(rest.iter().cloned()) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return Some(2);
        }
    };
    let runtime = tokio::runtime::Runtime::new().ok()?;
    match runtime.block_on(run(args)) {
        Ok(()) => Some(0),
        Err(e) => {
            eprintln!("error: {e}");
            Some(1)
        }
    }
}

/// Drive one command end to end, printing pipeline events to stdout.
pub async fn run(args: Args) -> Result<(), String> {
    let dirs = crate::paths::AppDirs::init()?;
    run_with_dirs(args, &dirs).await
}

pub async fn run_with_dirs(args: Args, dirs: &crate::paths::AppDirs) -> Result<(), String> {
    let cfg = crate::config::Config::load(dirs);
    match args.command {
        Command::Install => {
            let params = build_params(&args, dirs, &cfg)?;
            drive_pipeline(|tx| crate::str_install::run(params, tx)).await
        }
        Command::Launch => {
            let params = build_params(&args, dirs, &cfg)?;
            let ctx = crate::launch::LaunchContext {
                game_dir: params.game_dir,
                steam_root: params.steam_root,
                proton_tool: params.proton_tool,
                dirs: dirs.clone(),
            };
            println!("Launching Skyrim Together…");
            crate::launch::launch_together(&ctx)
        }
        Command::Server => {
            let key = args.api_key.unwrap_or_else(|| cfg.nexus_api_key.clone());
            let (tx, rx) = tokio::sync::mpsc::channel(256);
            let printer = tokio::spawn(print_events(rx));
            let rep = crate::str_install::Reporter::new(tx);
            println!("Fetching the dedicated server…");
            let result = crate::server::ensure_server(&key, args.offline_server, dirs, &rep).await;
            drop(rep);
            let _ = printer.await;
            result.map(|dir| {
                println!("Server installed in {}", dir.display());
            })
        }
    }
}

/// Spawn a pipeline run, print every event and wait for the final result.
async fn drive_pipeline<F, T>(setup: F) -> Result<(), String>
where
    F: FnOnce(tokio::sync::mpsc::Sender<Event>) -> T + Send + 'static,
    T: std::future::Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(256);
    let printer = tokio::spawn(print_events(rx));
    tokio::spawn(async move { setup(tx).await });
    let _ = printer.await;
    Ok(())
}

/// Print pipeline events; the loop ends when the pipeline drops its sender.
async fn print_events(mut rx: tokio::sync::mpsc::Receiver<Event>) {
    while let Some(event) = rx.recv().await {
        print_event(&event);
    }
}

fn print_event(event: &Event) {
    match event {
        Event::Step { label, index, total } => println!("\n[{}/{}] {label}", index + 1, total),
        Event::Progress { label, done, total } => {
            let pct = done
                .checked_mul(100)
                .and_then(|n| n.checked_div(*total))
                .map(|p| p.min(100));
            if let Some(pct) = pct {
                println!("  {label}: {pct}%");
            }
        }
        Event::Log(line) => println!("  {line}"),
        #[cfg(all(unix, not(target_os = "macos")))]
        Event::NeedsSteamRestart { steam_root, tool } => println!(
            "  Steam is running: the {tool} selection needs a restart. Re-run with Steam closed \
             to apply it automatically (steam root: {})",
            steam_root.display()
        ),
        Event::Done(result) => match result {
            Ok(summary) => match &summary.str_version {
                Some(v) => println!("\nDone — Skyrim Together Reborn v{v} installed."),
                None => println!("\nDone."),
            },
            Err(e) => println!("\nFAILED: {e}"),
        },
    }
}

/// Build pipeline Params from the CLI args + saved config, mirroring the GUI's
/// `App::build_params`.
fn build_params(
    args: &Args,
    dirs: &crate::paths::AppDirs,
    cfg: &crate::config::Config,
) -> Result<crate::str_install::Params, String> {
    let profile = match &args.profile {
        Some(name) => cfg
            .profiles
            .iter()
            .find(|p| p.name == *name)
            .ok_or_else(|| format!("no profile named \"{name}\" in config.json"))?,
        None => cfg.active(),
    };
    let installs = crate::steam::detect();
    let game_dir = resolve_game_dir(args, profile, &installs)?;
    let steam_root = installs
        .iter()
        .find(|i| i.game_dir == game_dir)
        .map(|i| i.steam_root.clone())
        .or_else(|| installs.first().map(|i| i.steam_root.clone()));
    let cc_backup_dir = game_dir
        .parent()
        .and_then(|common| common.parent())
        .map(|steamapps| steamapps.join("str-cc-backup"))
        .unwrap_or_else(|| dirs.cc_backup.clone());
    Ok(crate::str_install::Params {
        game_dir,
        steam_root,
        nexus_key: args.api_key.clone().unwrap_or_else(|| cfg.nexus_api_key.clone()),
        offline_str: args.offline_str.clone(),
        offline_al: args.offline_al.clone(),
        proton_tool: args
            .proton
            .clone()
            .or_else(|| profile.proton_tool.clone())
            .unwrap_or_else(crate::proton::default_tool_name),
        clean_cc: cfg.clean_cc,
        launch_mode: cfg.launch_mode,
        dirs: dirs.clone(),
        cc_backup_dir,
    })
}

/// `--game-dir` wins, then the saved profile, then auto-detection.
fn resolve_game_dir(
    args: &Args,
    profile: &crate::config::Profile,
    installs: &[crate::steam::GameInstall],
) -> Result<PathBuf, String> {
    let game_dir = args
        .game_dir
        .clone()
        .or_else(|| profile.game_dir.clone())
        .or_else(|| installs.first().map(|i| i.game_dir.clone()))
        .ok_or(
            "No Skyrim Special Edition folder found.\nPass --game-dir /path/to/\"Skyrim Special \
             Edition\" (the folder containing SkyrimSE.exe).",
        )?;
    if !game_dir.join("SkyrimSE.exe").is_file() {
        return Err(format!(
            "{} does not contain SkyrimSE.exe — point --game-dir at the game's root folder.",
            game_dir.display()
        ));
    }
    Ok(game_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_every_command_and_flag() {
        let parsed = args(&["install"]).unwrap();
        assert_eq!(parsed.command, Command::Install);
        assert_eq!(parsed, Args { command: Command::Install, ..Default::default() });

        let parsed = args(&[
            "install",
            "--game-dir",
            "/games/Skyrim Special Edition",
            "--profile",
            "Test",
            "--api-key",
            "k",
            "--offline-str",
            "/a.zip",
            "--offline-al",
            "/b.zip",
            "--proton",
            "GE-Proton10-34",
        ])
        .unwrap();
        assert_eq!(parsed.game_dir.as_deref(), Some(std::path::Path::new("/games/Skyrim Special Edition")));
        assert_eq!(parsed.profile.as_deref(), Some("Test"));
        assert_eq!(parsed.api_key.as_deref(), Some("k"));
        assert_eq!(parsed.offline_str.as_deref(), Some(std::path::Path::new("/a.zip")));
        assert_eq!(parsed.offline_al.as_deref(), Some(std::path::Path::new("/b.zip")));
        assert_eq!(parsed.proton.as_deref(), Some("GE-Proton10-34"));

        assert_eq!(args(&["launch"]).unwrap().command, Command::Launch);
        assert_eq!(args(&["server"]).unwrap().command, Command::Server);
        let server = args(&["server", "--offline-server", "/s.zip"]).unwrap();
        assert_eq!(server.offline_server.as_deref(), Some(std::path::Path::new("/s.zip")));
    }

    #[test]
    fn rejects_bad_usage() {
        assert!(args(&[]).is_err()); // no command
        assert!(args(&["install", "--game-dir"]).is_err()); // missing value
        assert!(args(&["frobnicate"]).is_err()); // unknown command
        assert!(args(&["install", "--wat", "x"]).is_err()); // unknown flag
        let err = args(&["nope"]).unwrap_err();
        assert!(err.contains("unknown command"), "err: {err}");
    }

    /// The whole headless install path with fixture archives — the exact flow
    /// CI can run: `str-launcher --headless install --game-dir ... --offline-str ...`
    #[tokio::test]
    async fn headless_install_with_fixtures_succeeds() {
        std::env::set_var("STR_LAUNCHER_TEST_MODE", "1");
        let root = std::env::temp_dir().join(format!("str-headless-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        #[cfg(target_os = "windows")]
        std::env::set_var(
            "USERPROFILE",
            root.join("steamapps/compatdata/489830/pfx/drive_c/users/steamuser"),
        );

        // Fake game + fixture archives.
        let game = root.join("steamapps/common/Skyrim Special Edition");
        std::fs::create_dir_all(game.join("Data")).unwrap();
        std::fs::write(game.join("SkyrimSE.exe"), b"mz-fake").unwrap();
        std::fs::write(game.join("Data/Skyrim.esm"), b"skyrim").unwrap();

        let make_zip = |path: &std::path::Path, files: &[(&str, &[u8])]| {
            let file = std::fs::File::create(path).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            for (name, content) in files {
                zw.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                zw.write_all(content).unwrap();
            }
            zw.finish().unwrap();
        };
        let str_zip = root.join("str.zip");
        make_zip(
            &str_zip,
            &[
                ("SkyrimTogetherReborn/SkyrimTogether.exe", b"st-exe"),
                ("SkyrimTogetherReborn/Data/SkyrimTogether.esp", b"esp"),
            ],
        );
        let al_zip = root.join("al.zip");
        make_zip(&al_zip, &[("SKSE/Plugins/version-1-6-1170-0.dll", b"al")]);

        // A fake Steam root with the default GE-Proton tool, so the pipeline
        // never downloads or touches a real Steam install.
        let fake_proton = root
            .join("steam/compatibilitytools.d")
            .join(crate::steam::DEFAULT_PROTON_TOOL);
        std::fs::create_dir_all(&fake_proton).unwrap();
        std::fs::write(fake_proton.join("proton"), b"#!/bin/sh\n").unwrap();

        let dirs = crate::paths::AppDirs {
            config: root.join("app/config"),
            data: root.join("app/data"),
            downloads: root.join("app/downloads"),
            staging: root.join("app/staging"),
            cc_backup: root.join("app/cc-backup"),
            scripts: root.join("app/scripts"),
            logs: root.join("app/logs"),
        };
        std::fs::create_dir_all(&dirs.downloads).unwrap();

        let parsed = args(&[
            "install",
            "--game-dir",
            game.to_str().unwrap(),
            "--offline-str",
            str_zip.to_str().unwrap(),
            "--offline-al",
            al_zip.to_str().unwrap(),
            "--proton",
            crate::steam::DEFAULT_PROTON_TOOL,
        ])
        .unwrap();
        run_with_dirs(parsed, &dirs).await.unwrap();

        // The install really happened.
        assert_eq!(std::fs::read(game.join("SkyrimTogether.exe")).unwrap(), b"st-exe");
        assert_eq!(std::fs::read(game.join("Data/SkyrimTogether.esp")).unwrap(), b"esp");
        let manifest = crate::config::InstallManifest::load(&game);
        assert!(manifest.files.iter().any(|f| f == "SkyrimTogether.exe"));

        // And the game-dir validation rejects nonsense.
        let bad = args(&["install", "--game-dir", root.join("nowhere").to_str().unwrap()]).unwrap();
        let err = run_with_dirs(bad, &dirs).await.unwrap_err();
        assert!(err.contains("SkyrimSE.exe"), "err: {err}");

        let _ = std::fs::remove_dir_all(&root);
    }
}
