//! The user's saved settings (`<data dir>/settings.json`, owner-only).

use std::path::PathBuf;

use mattcut_office::Login;
use serde::{Deserialize, Serialize};

/// Standalone build (no `team` feature): no sign-in, no server, projects stay on this computer.
pub const STANDALONE: bool = !cfg!(feature = "team");

/// The server new installs start with (set when building for the team: `MATTCUT_SERVER=…`).
pub const DEFAULT_SERVER: Option<&str> = option_env!("MATTCUT_SERVER");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// The team's MattCut server (`https://mattcut.example.com`).
    pub server: String,
    /// Last email used to sign in.
    pub email: String,
    /// Stay signed in on this computer.
    pub remember: bool,
    /// The saved session (when `remember`).
    pub session: Option<String>,
    /// The last sign-in, for working offline within the grace period.
    pub cached_login: Option<Login>,
    /// Unix time the server last confirmed the session.
    pub last_verified: i64,
    /// Local drafts.
    pub projects_dir: PathBuf,
    /// Where exports go by default.
    pub export_dir: PathBuf,
    pub timeline_height: f32,
    pub magnet: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let movies = if cfg!(target_os = "macos") { home.join("Movies") } else { home.join("Videos") };
        Self {
            server: DEFAULT_SERVER.unwrap_or_default().to_string(),
            email: String::new(),
            remember: true,
            session: None,
            cached_login: None,
            last_verified: 0,
            projects_dir: movies.join("MattCut Projects"),
            export_dir: movies.join("MattCut Exports"),
            timeline_height: 330.0,
            magnet: true,
        }
    }
}

impl AppConfig {
    fn path() -> Option<PathBuf> {
        mattcut_office::app_data_dir().map(|d| d.join("settings.json"))
    }

    pub fn load() -> Self {
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut copy = self.clone();
        if !copy.remember {
            copy.session = None;
            copy.cached_login = None;
        }
        let Ok(text) = serde_json::to_string_pretty(&copy) else {
            return;
        };
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        if let Ok(mut f) = opts.open(&path) {
            use std::io::Write;
            let _ = f.write_all(text.as_bytes());
        }
    }
}

/// This computer's name, shown to the team ("who is online").
pub fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| std::process::Command::new("hostname").output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).map(|s| s.trim().to_string()))
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "this computer".into())
}
