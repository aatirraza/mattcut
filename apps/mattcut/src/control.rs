//! Local control port for the MattCut MCP server (and scripts): JSON lines on 127.0.0.1 only,
//! authenticated with a random token that the app writes to `<data dir>/control.json`
//! (owner-only). The protocol matches FilmCraft's, so FilmCraft's MCP bridge drives MattCut.
//!
//! Request:  `{"id": 1, "token": "<64 hex>", "method": "engine.execute", "params": {"command": "timeline.razor", "params": {...}}}`
//! Reply:    `{"id": 1, "ok": true, "result": ...}` or `{"id": 1, "ok": false, "error": "..."}`

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use mattcut_office::{CONTROL_FILE, ControlInfo, DEFAULT_CONTROL_PORT, app_data_dir, random_token, secret_eq};
use serde_json::{Value, json};

const MAX_LINE: u64 = 4 * 1024 * 1024;

pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    /// Send `{"ok":…, "result"|"error":…}`.
    pub reply: Sender<Value>,
}

pub fn ok(result: Value) -> Value {
    json!({"ok": true, "result": result})
}

pub fn err(e: impl std::fmt::Display) -> Value {
    json!({"ok": false, "error": e.to_string()})
}

/// Start listening (first free port from `port`), and publish the port and token.
pub fn start(port: Option<u16>, ctx: egui::Context) -> Option<(Receiver<ControlRequest>, ControlInfo)> {
    let first = port.unwrap_or(DEFAULT_CONTROL_PORT);
    let listener = (first..first.saturating_add(20)).find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok())?;
    let port = listener.local_addr().ok()?.port();
    let token = random_token(32).ok()?;
    let info = ControlInfo { port, token: token.clone(), pid: std::process::id() };
    publish(&info);
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let (tx, ctx, token) = (tx.clone(), ctx.clone(), token.clone());
            let _ = std::thread::Builder::new().name("control-conn".into()).spawn(move || serve(stream, &tx, &ctx, &token));
        }
    });
    if let Err(e) = spawned {
        log::warn!("control server: {e}");
        return None;
    }
    log::info!("control port 127.0.0.1:{port}");
    Some((rx, info))
}

fn publish(info: &ControlInfo) {
    let Some(dir) = app_data_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(CONTROL_FILE);
    let Ok(text) = serde_json::to_string(info) else {
        return;
    };
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    match opts.open(&path) {
        Ok(mut f) => {
            let _ = f.write_all(text.as_bytes());
        }
        Err(e) => log::warn!("{}: {e}", path.display()),
    }
}

/// Remove the control file on exit (only if it is still ours).
pub fn unpublish(info: &ControlInfo) {
    let Some(path) = app_data_dir().map(|d| d.join(CONTROL_FILE)) else {
        return;
    };
    let ours = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<ControlInfo>(&t).ok()).is_some_and(|c| c.pid == info.pid);
    if ours {
        let _ = std::fs::remove_file(path);
    }
}

fn serve(stream: TcpStream, tx: &Sender<ControlRequest>, ctx: &egui::Context, token: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(600)));
    let Ok(read) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read);
    let mut writer = stream;
    loop {
        let mut buf = Vec::new();
        match reader.by_ref().take(MAX_LINE + 1).read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if buf.len() as u64 > MAX_LINE {
            let _ = writeln!(writer, "{}", err("request too long"));
            return;
        }
        let text = String::from_utf8_lossy(&buf);
        if text.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(text.trim()) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(writer, "{}", err(format!("bad JSON: {e}")));
                return;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let given = msg.get("token").and_then(Value::as_str).unwrap_or_default();
        let mut reply = if !secret_eq(given, token) {
            err("bad or missing control token (read it from MattCut's control.json)")
        } else {
            let method = msg.get("method").and_then(Value::as_str).unwrap_or_default().to_string();
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let (rtx, rrx) = channel();
            if tx.send(ControlRequest { method, params, reply: rtx }).is_err() {
                return;
            }
            ctx.request_repaint();
            rrx.recv_timeout(Duration::from_secs(120)).unwrap_or_else(|_| err("timeout"))
        };
        if let Some(o) = reply.as_object_mut() {
            o.insert("id".into(), id);
        }
        if writeln!(writer, "{reply}").and_then(|()| writer.flush()).is_err() {
            return;
        }
    }
}
