//! Finding, starting and stopping the RoastMCP daemon (`roastmcp daemon`).
//!
//! The daemon is a separate process on purpose: it owns the roaster and the
//! safety layer, so a crash of the window does not take them down. The app
//! reuses a daemon that is already running (for example one started by hand)
//! and only stops the one it started itself.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Bundle identifier, also the name of the data directory (as in `roastd`).
const APP_ID: &str = "com.andreasbauer.roastmcp";

/// The port the app asks for; the web UI's dev server proxies to it.
const PORT: u16 = 7878;

/// How long a starting daemon may take to answer.
const START_TIMEOUT: Duration = Duration::from_secs(15);

/// Contents of `daemon.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct DaemonInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

/// A running daemon.
pub struct Daemon {
    pub info: DaemonInfo,
    /// The process, when this app started it.
    child: Option<Child>,
}

/// The daemon's data directory.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_ID)
}

/// Where the daemon's output goes when the app starts it.
pub fn log_path() -> PathBuf {
    data_dir().join("logs").join("daemon.log")
}

fn read_info() -> Option<DaemonInfo> {
    let text = std::fs::read_to_string(data_dir().join("daemon.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .into()
}

impl DaemonInfo {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn get(&self, path: &str) -> Result<serde_json::Value, String> {
        agent()
            .get(&self.url(path))
            .header("Authorization", format!("Bearer {}", self.token))
            .call()
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_json()
            .map_err(|e| e.to_string())
    }

    fn post(&self, path: &str, body: &serde_json::Value) -> Result<(), String> {
        agent()
            .post(&self.url(path))
            .header("Authorization", format!("Bearer {}", self.token))
            .send_json(body)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn alive(&self) -> bool {
        self.get("/api/status").is_ok()
    }
}

/// What the app needs to know before quitting.
pub struct Activity {
    pub recording: bool,
    pub burner_on: bool,
}

impl Daemon {
    /// Reuses a running daemon, or starts one and waits until it answers.
    pub fn ensure(sidecar: &Path, ui_dir: &Path) -> Result<Daemon, String> {
        if let Some(info) = read_info()
            && info.alive()
        {
            return Ok(Daemon { info, child: None });
        }
        Self::start(sidecar, ui_dir)
    }

    fn start(sidecar: &Path, ui_dir: &Path) -> Result<Daemon, String> {
        if !sidecar.exists() {
            return Err(format!(
                "The daemon binary is missing: {}\nBuild it with scripts/build-sidecar.sh.",
                sidecar.display()
            ));
        }
        let log = log_path();
        if let Some(dir) = log.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // The daemon prints its token; keep the log private like daemon.json.
        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            opts.mode(0o600);
        }
        let out = opts
            .open(&log)
            .map_err(|e| format!("cannot open {}: {e}", log.display()))?;
        let err = out.try_clone().map_err(|e| e.to_string())?;
        let mut child = Command::new(sidecar)
            .arg("daemon")
            .arg("--port")
            .arg(PORT.to_string())
            .arg("--ui-dir")
            .arg(ui_dir)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", sidecar.display()))?;

        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!(
                    "The daemon stopped right away ({status}).\nDetails: {}",
                    log.display()
                ));
            }
            if let Some(info) = read_info()
                && info.pid == child.id()
                && info.alive()
            {
                return Ok(Daemon {
                    info,
                    child: Some(child),
                });
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return Err(format!(
                    "The daemon did not answer within {} s.\nDetails: {}",
                    START_TIMEOUT.as_secs(),
                    log.display()
                ));
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    /// Whether this app started the daemon (and so stops it on quit).
    pub fn owned(&self) -> bool {
        self.child.is_some()
    }

    /// Whether a daemon this app started has exited.
    pub fn exited(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(Some(_))))
    }

    /// The UI's address with the token in the fragment. `base` replaces the
    /// daemon's own address (the Vite dev server during development).
    pub fn ui_url(&self, base: Option<&str>) -> String {
        let base = base.map_or_else(
            || format!("http://127.0.0.1:{}", self.info.port),
            |b| b.trim_end_matches('/').to_owned(),
        );
        format!("{base}/#token={}", self.info.token)
    }

    /// A running recording and the burner, as far as the daemon can tell.
    pub fn activity(&self) -> Activity {
        let Ok(s) = self.info.get("/api/status") else {
            return Activity {
                recording: false,
                burner_on: false,
            };
        };
        Activity {
            recording: !s["recording"].is_null(),
            burner_on: s["safety"]["actuators"]["burner"]
                .as_f64()
                .is_some_and(|b| b > 0.0),
        }
    }

    /// Emergency stop: burner off, air and drum to the safe values, latched.
    pub fn emergency_stop(&self, reason: &str) -> Result<(), String> {
        self.info.post(
            "/api/emergency-stop",
            &serde_json::json!({ "reason": reason }),
        )
    }

    /// Stops a daemon this app started: the daemon puts the roaster into the
    /// safe state if the burner is on and saves a running recording first.
    pub fn shutdown(mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if self
            .info
            .post("/api/shutdown", &serde_json::json!({}))
            .is_err()
        {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // The daemon sends the safe state before it waits for clients, so a
        // daemon that hangs after that can be stopped hard.
        let _ = child.kill();
    }
}
