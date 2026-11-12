//! `mattcut-mcp` — connect Claude to MattCut over MCP (stdio).
//!
//! * **Live mode (default):** when the MattCut app is running, every tool drives the open project
//!   in the app: Claude's edits show up on your timeline, and screenshots show your window. The
//!   app's control port and token are read from `<MattCut data dir>/control.json`, which the app
//!   writes on start, so there is nothing to configure.
//! * **Headless mode:** when the app is not running (or with `--headless`), tools work on an
//!   in-process project instead: open one with `--project PATH`, and save it with the
//!   `file.save` command.
//!
//! Add to Claude Code:  `claude mcp add mattcut -- /path/to/mattcut-mcp`
//! Add to Claude Desktop: see docs/mcp.md.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use mattcut_office::{CONTROL_FILE, ControlInfo, app_data_dir};
use serde_json::json;

/// The bridge client reads its credential from this variable (shared FilmCraft automation code).
const TOKEN_ENV: &str = "FILMCRAFT_CONTROL_TOKEN";

fn diag(msg: &str) {
    // stdout is the MCP channel: diagnostics go to stderr only.
    eprintln!("mattcut-mcp: {msg}");
}

fn control_info() -> Option<ControlInfo> {
    let path = app_data_dir()?.join(CONTROL_FILE);
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn app_listening(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(500)).is_ok()
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let opt = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    if flag("-h") || flag("--help") {
        eprintln!("mattcut-mcp [--headless [--project PATH]] [--port N]\n  MCP server on stdio for MattCut. Live mode drives the running app.");
        return;
    }

    let live = if flag("--headless") { None } else { control_info() };
    if let Some(info) = live {
        let port = opt("--port").and_then(|p| p.parse().ok()).unwrap_or(info.port);
        if app_listening(port) {
            // The shared bridge client reads the token from its environment: start ourselves again
            // with it set (no unsafe `set_var` in this process).
            if std::env::var(TOKEN_ENV).ok().as_deref() != Some(info.token.as_str()) {
                let exe = std::env::current_exe().unwrap_or_else(|_| "mattcut-mcp".into());
                let status = std::process::Command::new(exe).args(&args).env(TOKEN_ENV, &info.token).status();
                std::process::exit(status.ok().and_then(|s| s.code()).unwrap_or(1));
            }
            diag(&format!("driving the running MattCut app on 127.0.0.1:{port}"));
            match filmcraft_automation::FilmcraftMcp::bridge(&format!("127.0.0.1:{port}")) {
                Ok(server) => {
                    if let Err(e) = server.serve_stdio().await {
                        diag(&e.to_string());
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    diag(&e.to_string());
                    std::process::exit(1);
                }
            }
            return;
        }
        diag("MattCut app is not running: headless mode (edits stay in this process until you run file.save)");
    }

    let mut session = filmcraft_engine::Session::default();
    if let Some(path) = opt("--project") {
        if let Err(e) = session.execute("file.open", json!({"path": path})) {
            diag(&format!("opening {path}: {e}"));
            std::process::exit(1);
        }
    }
    if let Err(e) = filmcraft_automation::FilmcraftMcp::headless(session).serve_stdio().await {
        diag(&e.to_string());
        std::process::exit(1);
    }
}
