//! The `roastmcp` command line.

mod audio;
mod mcp;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Open-source coffee roasting with AI under guardrails.
#[derive(Parser)]
#[command(name = "roastmcp", version, about)]
struct Cli {
    /// Data directory (default: the platform's application-support folder).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the daemon: roaster connection, safety layer, API and web UI.
    Daemon {
        /// TCP port on 127.0.0.1.
        #[arg(long, default_value_t = 7878)]
        port: u16,
        /// Connect to the built-in simulator on start.
        #[arg(long, conflicts_with = "serial")]
        simulator: bool,
        /// Connect to this serial port on start, e.g. /dev/cu.AKROASTER.
        #[arg(long)]
        serial: Option<String>,
        /// Machine profile (TOML); default: the built-in Skywalker Cubean+.
        #[arg(long)]
        machine: Option<PathBuf>,
        /// Directory of the built web UI (default: app/dist if present).
        #[arg(long)]
        ui_dir: Option<PathBuf>,
        /// Simulator speed factor (2 = twice as fast as real time).
        #[arg(long, default_value_t = 1.0)]
        sim_speed: f64,
    },
    /// MCP server on stdio for AI clients; forwards to the running daemon.
    Mcp,
    /// List serial ports.
    Ports,
    /// Write a checked backup of the database: into the folder set in the
    /// app, or into `--to`.
    Backup {
        /// Folder for the backup (default: the one set in the app).
        #[arg(long)]
        to: Option<PathBuf>,
    },
    /// Put a backup back in place of the database. RoastMCP must not be
    /// running; the database that was there is kept beside it.
    Restore {
        /// The backup file (`roastmcp-<time>.sqlite`).
        file: PathBuf,
    },
    /// Recorded roast sound and the crack detector (ADR 0021).
    #[command(subcommand)]
    Audio(AudioCommand),
    /// Import Artisan `.alog` files (or folders of them) into the archive.
    /// The files themselves are only read.
    Import {
        /// `.alog` files or folders (searched recursively).
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AudioCommand {
    /// Play a recorded sound file (.wav, .flac) through the crack detector
    /// and compare what it hears with the roast's marks. The roast is the
    /// one recorded while the sound ran, unless `--roast` names one.
    Analyze {
        /// The sound file.
        file: PathBuf,
        /// A roast id from the archive, or a `.roast.json` / `.alog` file.
        #[arg(long)]
        roast: Option<String>,
        /// Local start time of the sound, `YYYY-MM-DD HH:MM:SS` (default:
        /// from a name like `2026-11-14_19-02-11.flac`).
        #[arg(long)]
        start: Option<String>,
        /// low, normal or high.
        #[arg(long, default_value = "normal", value_parser = parse_sensitivity)]
        sensitivity: roast_audio::Sensitivity,
        /// Print the whole analysis as JSON.
        #[arg(long)]
        json: bool,
    },
}

fn parse_sensitivity(s: &str) -> Result<roast_audio::Sensitivity, String> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .map_err(|_| format!("{s}: low, normal or high"))
}

fn find_alogs(path: &std::path::Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            find_alogs(&p, out);
        }
    } else if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("alog"))
    {
        out.push(path.to_owned());
    }
}

fn open_store(data_dir: &std::path::Path) -> anyhow::Result<std::sync::Arc<roast_store::Store>> {
    use anyhow::Context as _;
    let path = roastd::database_path(data_dir);
    let store = roast_store::Store::open(&path)
        .with_context(|| format!("cannot open the database {}", path.display()))?;
    Ok(std::sync::Arc::new(store))
}

/// Whether a daemon is serving this data directory right now.
fn daemon_running(data_dir: &std::path::Path) -> bool {
    roastd::read_daemon_info(data_dir).is_ok_and(|info| {
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], info.port)),
            std::time::Duration::from_millis(500),
        )
        .is_ok()
    })
}

fn backup(data_dir: &std::path::Path, to: Option<PathBuf>) -> anyhow::Result<()> {
    let store = open_store(data_dir)?;
    let report = match to {
        Some(dir) => {
            let dir = std::path::absolute(dir)?;
            store.backup(&dir, roastd::backup::config(&store).keep)?
        }
        None => {
            let done = roastd::backup::run(&store)
                .map_err(|e| anyhow::anyhow!("{e} (set one in the app, or pass --to <folder>)"))?;
            roast_store::BackupReport {
                path: done.path.unwrap_or_default(),
                bytes: done.bytes.unwrap_or_default(),
                removed: 0,
            }
        }
    };
    println!(
        "Backup written and checked: {} ({} kB)",
        report.path.display(),
        report.bytes / 1024
    );
    Ok(())
}

fn restore(data_dir: &std::path::Path, file: &std::path::Path) -> anyhow::Result<()> {
    if daemon_running(data_dir) {
        anyhow::bail!("RoastMCP is running; quit the app (or stop the daemon) and try again");
    }
    let db = roastd::database_path(data_dir);
    let kept = roast_store::restore(&db, file)?;
    println!("Restored {} from {}", db.display(), file.display());
    if let Some(kept) = kept {
        println!("The database that was there is kept as {}", kept.display());
    }
    Ok(())
}

fn import(data_dir: &std::path::Path, paths: &[PathBuf]) -> anyhow::Result<()> {
    let roasts_dir = data_dir.join("roasts");
    let archive = roastd::files::Archive::new(open_store(data_dir)?, roasts_dir.clone());
    // Roast files the database does not know yet (a first start) come first.
    archive.sync()?;
    let mut files = Vec::new();
    for p in paths {
        find_alogs(p, &mut files);
    }
    let (mut saved, mut there, mut failed) = (0, 0, 0);
    for f in &files {
        match archive.import_alog(f) {
            Ok(roastd::files::Imported::Saved(_)) => saved += 1,
            Ok(roastd::files::Imported::AlreadyThere(_)) => there += 1,
            Err(e) => {
                failed += 1;
                eprintln!("{}: {e}", f.display());
            }
        }
    }
    println!(
        "{} .alog files: {saved} imported, {there} already in the archive, {failed} failed.",
        files.len()
    );
    println!("Archive: {}", roasts_dir.display());
    if failed > 0 {
        anyhow::bail!("{failed} files could not be imported");
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let data_dir = cli.data_dir.unwrap_or_else(roastd::default_data_dir);
    match cli.command {
        Command::Daemon {
            port,
            simulator,
            serial,
            machine,
            ui_dir,
            sim_speed,
        } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info".into()),
                )
                .init();
            // On macOS the main thread serves Bluetooth (IOBluetooth calls back
            // only there, ADR 0013); the runtime runs on a worker thread.
            roast_driver::run_with_main_loop(move || -> anyhow::Result<()> {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(roastd::run(roastd::DaemonOptions {
                    data_dir,
                    port,
                    machine,
                    ui_dir,
                    simulator,
                    serial_port: serial,
                    sim_speed,
                }))
            })?
        }
        Command::Mcp => mcp::serve(&data_dir),
        Command::Ports => {
            for p in roast_driver::run_with_main_loop(roast_driver::list_ports)? {
                println!("{p}");
            }
            Ok(())
        }
        Command::Import { paths } => import(&data_dir, &paths),
        Command::Audio(AudioCommand::Analyze {
            file,
            roast,
            start,
            sensitivity,
            json,
        }) => audio::analyze(
            &data_dir,
            &audio::AnalyzeArgs {
                file,
                roast,
                start,
                sensitivity,
                json,
            },
        ),
        Command::Backup { to } => backup(&data_dir, to),
        Command::Restore { file } => restore(&data_dir, &file),
    }
}
