//! The `roastmcp` command line.

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
    /// Import Artisan `.alog` files (or folders of them) into the archive.
    /// The files themselves are only read.
    Import {
        /// `.alog` files or folders (searched recursively).
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
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

fn import(roasts_dir: &std::path::Path, paths: &[PathBuf]) -> anyhow::Result<()> {
    let mut files = Vec::new();
    for p in paths {
        find_alogs(p, &mut files);
    }
    let (mut saved, mut there, mut failed) = (0, 0, 0);
    for f in &files {
        match roastd::files::import_alog(roasts_dir, f) {
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
        }
        Command::Mcp => mcp::serve(&data_dir),
        Command::Ports => {
            for p in roast_driver::list_ports() {
                println!("{p}");
            }
            Ok(())
        }
        Command::Import { paths } => import(&data_dir.join("roasts"), &paths),
    }
}
