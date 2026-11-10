//! The RoastMCP daemon: owns the roaster connection, the safety layer and the
//! running roast, and serves the HTTP/WebSocket API, the web UI and (through
//! `roastmcp mcp`) the MCP tools.

pub mod api;
pub mod backup;
mod driver_thread;
pub mod engine;
pub mod files;
pub mod inventory;
pub mod learned;
mod legacy;
pub mod profiles;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use roast_machine::MachineProfile;
use serde::{Deserialize, Serialize};

/// The Cubean+ profile, built in so the binary is self-contained.
pub const DEFAULT_PROFILE: &str = include_str!("../../../machines/skywalker-cubean-plus.toml");

/// Bundle identifier, also the name of the data directory.
pub const APP_ID: &str = "com.andreasbauer.roastmcp";

/// Contents of `daemon.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    /// TCP port on 127.0.0.1.
    pub port: u16,
    /// Bearer token.
    pub token: String,
    /// Process id.
    pub pid: u32,
    /// Daemon version.
    pub version: String,
}

/// Options of `roastmcp daemon`.
#[derive(Debug, Clone)]
pub struct DaemonOptions {
    /// Data directory.
    pub data_dir: PathBuf,
    /// TCP port.
    pub port: u16,
    /// Machine profile file; the built-in Cubean+ profile if `None`.
    pub machine: Option<PathBuf>,
    /// Directory of the built web UI.
    pub ui_dir: Option<PathBuf>,
    /// Connect to the simulator right away.
    pub simulator: bool,
    /// Connect to this serial port right away.
    pub serial_port: Option<String>,
    /// Simulator speed factor.
    pub sim_speed: f64,
}

/// The default data directory.
#[must_use]
pub fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_ID)
}

/// Path of the database in a data directory.
#[must_use]
pub fn database_path(data_dir: &Path) -> PathBuf {
    data_dir.join("roastmcp.sqlite")
}

/// Path of `daemon.json` in a data directory.
#[must_use]
pub fn daemon_json(data_dir: &Path) -> PathBuf {
    data_dir.join("daemon.json")
}

/// Reads `daemon.json`.
pub fn read_daemon_info(data_dir: &Path) -> anyhow::Result<DaemonInfo> {
    let path = daemon_json(data_dir);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("is the daemon running? cannot read {}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

fn write_daemon_info(data_dir: &Path, info: &DaemonInfo) -> anyhow::Result<()> {
    use std::io::Write as _;
    std::fs::create_dir_all(data_dir)?;
    let path = daemon_json(data_dir);
    let _ = std::fs::remove_file(&path);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path)?;
    f.write_all(serde_json::to_string_pretty(info)?.as_bytes())?;
    Ok(())
}

fn find_ui_dir(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(d) = explicit {
        return Some(d);
    }
    let mut candidates = vec![PathBuf::from("app/dist")];
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("ui"));
        candidates.push(dir.join("../share/roastmcp/ui"));
        candidates.push(dir.join("../../app/dist"));
    }
    candidates
        .into_iter()
        .find(|d| d.join("index.html").exists())
}

/// Runs the daemon until Ctrl-C or SIGTERM.
pub async fn run(opts: DaemonOptions) -> anyhow::Result<()> {
    let profile = match &opts.machine {
        Some(p) => MachineProfile::load(p)?,
        None => MachineProfile::from_toml_str(DEFAULT_PROFILE)?,
    };
    let roasts_dir = opts.data_dir.join("roasts");
    let db_path = database_path(&opts.data_dir);
    let store = Arc::new(
        roast_store::Store::open(&db_path)
            .with_context(|| format!("cannot open the database {}", db_path.display()))?,
    );
    let engine = engine::start(engine::EngineOptions {
        profile,
        store: Arc::clone(&store),
        roasts_dir: roasts_dir.clone(),
        audit_path: opts.data_dir.join("logs").join("actions.jsonl"),
        inventory_path: opts.data_dir.join("inventory.json"),
        control_path: opts.data_dir.join("control.json"),
        profiles_path: opts.data_dir.join("profiles.json"),
        sim_speed: opts.sim_speed,
    });

    let token: String = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let addr = SocketAddr::from(([127, 0, 0, 1], opts.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot listen on {addr} - is another daemon running?"))?;
    let port = listener.local_addr()?.port();
    write_daemon_info(
        &opts.data_dir,
        &DaemonInfo {
            port,
            token: token.clone(),
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
    )?;

    let ui_dir = find_ui_dir(opts.ui_dir.clone());
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let state = api::AppState {
        engine: engine.clone(),
        token: Arc::from(token.as_str()),
        archive: files::Archive::new(Arc::clone(&store), roasts_dir),
        store: Arc::clone(&store),
        shutdown: shutdown.clone(),
    };
    // The daily backup (ADR 0003); a roast being saved makes one as well.
    let daily = Arc::clone(&store);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(600));
        loop {
            tick.tick().await;
            if backup::due(&daily, chrono::Utc::now()) {
                let store = Arc::clone(&daily);
                let _ = tokio::task::spawn_blocking(move || backup::run(&store)).await;
            }
        }
    });
    let app = api::router(state, ui_dir.clone());

    if opts.simulator || opts.serial_port.is_some() {
        let req: engine::ConnectReq = serde_json::from_value(serde_json::json!({
            "simulator": opts.simulator,
            "port": opts.serial_port,
        }))?;
        if let Ok(Err(e)) = engine.call(move |e| e.connect(req)).await {
            tracing::error!("connect failed: {}", e.error);
        }
    }

    println!(
        "RoastMCP daemon {} on http://127.0.0.1:{port}",
        env!("CARGO_PKG_VERSION")
    );
    match &ui_dir {
        Some(d) => println!(
            "Open the app: http://127.0.0.1:{port}/#token={token}   (UI from {})",
            d.display()
        ),
        None => println!("No web UI found (build it with: cd app && npm run build)."),
    }
    println!("Data: {}", opts.data_dir.display());

    let shutdown_engine = engine.clone();
    let stopped = Arc::new(tokio::sync::Notify::new());
    let stopped_tx = stopped.clone();
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        tokio::select! {
            () = shutdown_signal() => {},
            () = shutdown.notified() => {},
        }
        println!("Shutting down: roaster to a safe state if the burner is on.");
        let _ = shutdown_engine.call(|e| e.shutdown()).await;
        // Give the driver thread a moment to send the last commands.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        stopped_tx.notify_one();
    });
    // Open WebSockets would keep a graceful shutdown waiting; the roaster is
    // already safe and the roast saved by then, so stop waiting after 3 s.
    tokio::select! {
        r = server => r?,
        () = async {
            stopped.notified().await;
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        } => {},
    }
    let _ = std::fs::remove_file(daemon_json(&opts.data_dir));
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = term => {},
    }
}
