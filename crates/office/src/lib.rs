//! # mattcut-office
//!
//! The link between the MattCut desktop app and the MattCut team server.
//!
//! * **Transport:** HTTPS. Every call is `POST <server>/api` with a JSON [`Request`]; the reply is
//!   a JSON [`Response`]. Plain `http://` is allowed only for servers on a private network or
//!   this computer (testing, office-only setups).
//! * **Sign-in:** email + password, or Google ([`GoogleAuth`]: the desktop "loopback + PKCE"
//!   flow; the server swaps the code for the verified Google email). Only emails on the team
//!   list can sign in.
//! * **Lease:** a signed-in app renews its session with heartbeats. If the server cannot be
//!   reached, the app keeps working for the lease (the offline grace period), then pauses.

#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest;

/// Default port of the server's HTTP listener (put HTTPS in front of it: Caddy, Cloudflare Tunnel…).
pub const DEFAULT_PORT: u16 = 8780;
pub const PROTOCOL_VERSION: u32 = 2;
/// Largest request or reply body (projects travel base64-encoded).
pub const MAX_BODY: u64 = 256 * 1024 * 1024;
/// Largest project file the server stores.
pub const MAX_PROJECT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug)]
pub enum OfficeError {
    /// The server could not be reached.
    Network(String),
    /// The server answered with an error.
    Server(String),
    /// The reply could not be understood, or the address is not acceptable.
    Protocol(String),
}

impl std::fmt::Display for OfficeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(e) => write!(f, "can't reach the MattCut server ({e})"),
            Self::Server(e) => write!(f, "{e}"),
            Self::Protocol(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for OfficeError {}

impl OfficeError {
    /// The server rejected the session (signed out, expired, account removed).
    pub fn is_auth(&self) -> bool {
        matches!(self, Self::Server(m) if m.starts_with(AUTH_ERROR_PREFIX))
    }
}

/// Server errors that mean "sign in again" start with this.
pub const AUTH_ERROR_PREFIX: &str = "Please sign in again";

pub type Result<T> = std::result::Result<T, OfficeError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default)]
    pub result: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(id: u64, result: Value) -> Self {
        Self { id, ok: true, result, error: None }
    }
    pub fn err(id: u64, error: impl Into<String>) -> Self {
        Self { id, ok: false, result: Value::Null, error: Some(error.into()) }
    }
}

/// What a server says about itself (`hello`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerInfo {
    /// Team / company name shown on the sign-in screen.
    pub team: String,
    pub protocol: u32,
    pub version: String,
    /// The Google OAuth client id when "Continue with Google" is set up on the server.
    #[serde(default)]
    pub google_client_id: Option<String>,
    /// Where the team keeps shared footage (a NAS / drive path shown to editors), if set.
    #[serde(default)]
    pub media_share: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[default]
    Editor,
    Admin,
}

/// A successful sign-in (or a resumed session).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    /// Keep it: it signs this computer in for `session_days`.
    pub session: String,
    pub email: String,
    pub name: String,
    pub role: Role,
    pub team: String,
    /// Offline grace: how long the app keeps working without reaching the server.
    pub lease_seconds: u64,
    #[serde(default)]
    pub media_share: Option<String>,
}

/// A team member, as the admin sees them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Member {
    pub email: String,
    pub name: String,
    pub role: Role,
    /// Can sign in with a password (else Google only).
    pub has_password: bool,
}

/// A project stored on the server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectEntry {
    pub name: String,
    pub size: u64,
    /// RFC 3339 time of the last upload.
    pub modified: String,
    #[serde(default)]
    pub modified_by: Option<String>,
    /// Who has it open for editing (others can open a read-only copy).
    #[serde(default)]
    pub locked_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnlineUser {
    pub user: String,
    pub host: String,
    pub editing: Option<String>,
}

/// Loopback, RFC 1918, link-local and IPv6 unique-local / link-local addresses.
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_ip(IpAddr::V4(v4));
            }
            let first = v6.segments().first().copied().unwrap_or(0);
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// Project names are file names on the server: letters, digits, space, `-`, `_`, `.`, `(`, `)`;
/// 1–80 characters; no leading dot.
pub fn valid_project_name(name: &str) -> bool {
    let n = name.chars().count();
    (1..=80).contains(&n)
        && !name.starts_with('.')
        && name.trim() == name
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')'))
}

/// A plausible email address (normalised to lower case).
pub fn normalize_email(email: &str) -> Option<String> {
    let e = email.trim().to_lowercase();
    let (local, domain) = e.split_once('@')?;
    let ok = !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.') && e.len() <= 254 && !e.chars().any(char::is_whitespace);
    ok.then_some(e)
}

/// A random hex token of `bytes` bytes (sessions, keys).
pub fn random_token(bytes: usize) -> std::result::Result<String, OfficeError> {
    let mut buf = vec![0u8; bytes.min(256)];
    getrandom::fill(&mut buf).map_err(|e| OfficeError::Protocol(format!("random: {e}")))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// SHA-256 as lower-case hex.
pub fn sha256_hex(data: &[u8]) -> String {
    sha2::Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Constant-time string comparison for secrets.
pub fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn encode_bytes(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

pub fn decode_bytes(s: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(s).map_err(|e| OfficeError::Protocol(format!("base64: {e}")))
}

/// `application/x-www-form-urlencoded` / query-string escaping.
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Undo [`url_encode`] (`+` as space).
pub fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b'%') => {
                let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            Some(b'+') => {
                out.push(b' ');
                i += 1;
            }
            Some(&b) => {
                out.push(b);
                i += 1;
            }
            None => break,
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The HTTPS client used by the app and the server (pure-Rust TLS, the OS's trusted roots).
pub fn http_agent(timeout: Duration) -> ureq::Agent {
    let provider = Arc::new(rustls_rustcrypto::provider());
    ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::Rustls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .unversioned_rustls_crypto_provider(provider)
                .build(),
        )
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Turn what a person typed into the server's base URL. `https://` is assumed; plain `http://`
/// only for servers on a private network or this computer.
pub fn normalize_server_url(input: &str) -> Result<String> {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() {
        return Err(OfficeError::Protocol("enter your team's MattCut server address".into()));
    }
    let (scheme, rest) = match s.split_once("://") {
        Some((sc, r)) => (sc.to_lowercase(), r.to_string()),
        None => {
            let host = s.split([':', '/']).next().unwrap_or_default();
            let local = host == "localhost" || host.parse::<IpAddr>().is_ok_and(is_private_ip) || host.ends_with(".local");
            (if local { "http".into() } else { "https".into() }, s.to_string())
        }
    };
    if scheme != "https" && scheme != "http" {
        return Err(OfficeError::Protocol(format!("`{scheme}://` is not a web address")));
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let hostname = if host.starts_with('[') { host.split(']').next().unwrap_or(host).trim_start_matches('[') } else { host.split(':').next().unwrap_or(host) };
    if hostname.is_empty() {
        return Err(OfficeError::Protocol("the server address has no host name".into()));
    }
    if scheme == "http" {
        let local = hostname == "localhost" || hostname.parse::<IpAddr>().is_ok_and(is_private_ip) || hostname.ends_with(".local");
        if !local {
            return Err(OfficeError::Protocol("use https:// for a server on the internet (passwords would travel unencrypted)".into()));
        }
        let with_port = if host.contains(':') && !host.ends_with(']') { host.to_string() } else { format!("{host}:{DEFAULT_PORT}") };
        return Ok(format!("http://{with_port}"));
    }
    Ok(format!("https://{host}"))
}

/// A connection to the MattCut server.
pub struct OfficeClient {
    base: String,
    agent: ureq::Agent,
    session: Option<String>,
    next_id: u64,
}

impl OfficeClient {
    /// `server` as typed (`mattcut.example.com`, `https://…`, `192.168.1.20`).
    pub fn new(server: &str) -> Result<Self> {
        Ok(Self { base: normalize_server_url(server)?, agent: http_agent(Duration::from_secs(600)), session: None, next_id: 1 })
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub fn session(&self) -> Option<&str> {
        self.session.as_deref()
    }

    pub fn set_session(&mut self, session: Option<String>) {
        self.session = session;
    }

    /// Call `method`; returns the reply's `result`, or the server's error.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let req = Request { id, method: method.into(), params, session: self.session.clone() };
        let body = serde_json::to_string(&req).map_err(|e| OfficeError::Protocol(e.to_string()))?;
        let mut resp = self
            .agent
            .post(format!("{}/api", self.base))
            .header("Content-Type", "application/json")
            .header("User-Agent", concat!("MattCut/", env!("CARGO_PKG_VERSION")))
            .send(body)
            .map_err(|e| OfficeError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().with_config().limit(MAX_BODY).read_to_string().map_err(|e| OfficeError::Network(e.to_string()))?;
        let parsed: Response = serde_json::from_str(&text).map_err(|_| {
            if status == 200 {
                OfficeError::Protocol("the server's reply was not understood".into())
            } else {
                OfficeError::Network(format!("HTTP {status} — is this the MattCut server address?"))
            }
        })?;
        if parsed.ok { Ok(parsed.result) } else { Err(OfficeError::Server(parsed.error.unwrap_or_else(|| "error".into()))) }
    }

    fn typed<T: for<'de> Deserialize<'de>>(&mut self, method: &str, params: Value) -> Result<T> {
        let v = self.call(method, params)?;
        serde_json::from_value(v).map_err(|e| OfficeError::Protocol(format!("{method}: {e}")))
    }

    pub fn hello(&mut self) -> Result<ServerInfo> {
        self.typed("hello", json!({"protocol": PROTOCOL_VERSION}))
    }

    fn keep(&mut self, login: Login) -> Login {
        self.session = Some(login.session.clone());
        login
    }

    pub fn login_password(&mut self, email: &str, password: &str, host: &str) -> Result<Login> {
        let l = self.typed("login.password", json!({"email": email, "password": password, "host": host}))?;
        Ok(self.keep(l))
    }

    /// Finish "Continue with Google": the code from the browser and the PKCE verifier.
    pub fn login_google(&mut self, code: &str, verifier: &str, redirect_uri: &str, host: &str) -> Result<Login> {
        let l = self.typed("login.google", json!({"code": code, "verifier": verifier, "redirect_uri": redirect_uri, "host": host}))?;
        Ok(self.keep(l))
    }

    /// Resume a saved session (app start).
    pub fn resume(&mut self, session: &str, host: &str) -> Result<Login> {
        self.session = Some(session.to_string());
        let l = self.typed("session.resume", json!({"host": host}))?;
        Ok(self.keep(l))
    }

    /// Renew the lease; returns its length in seconds.
    pub fn heartbeat(&mut self, editing: Option<&str>) -> Result<u64> {
        let v = self.call("heartbeat", json!({"editing": editing}))?;
        v.get("lease_seconds").and_then(Value::as_u64).ok_or_else(|| OfficeError::Protocol("heartbeat: no lease".into()))
    }

    pub fn logout(&mut self) -> Result<()> {
        self.call("logout", json!({}))?;
        self.session = None;
        Ok(())
    }

    pub fn change_password(&mut self, old: &str, new: &str) -> Result<()> {
        self.call("password.change", json!({"old": old, "new": new})).map(|_| ())
    }

    pub fn list_projects(&mut self) -> Result<Vec<ProjectEntry>> {
        self.typed("projects.list", json!({}))
    }

    pub fn download(&mut self, name: &str) -> Result<Vec<u8>> {
        let v = self.call("projects.download", json!({"name": name}))?;
        decode_bytes(v.get("data").and_then(Value::as_str).unwrap_or_default())
    }

    pub fn upload(&mut self, name: &str, data: &[u8]) -> Result<ProjectEntry> {
        self.typed("projects.upload", json!({"name": name, "data": encode_bytes(data)}))
    }

    /// Take the edit lock. `force` takes it from someone else (their next save is refused).
    pub fn lock(&mut self, name: &str, force: bool) -> Result<()> {
        self.call("projects.lock", json!({"name": name, "force": force})).map(|_| ())
    }

    pub fn unlock(&mut self, name: &str) -> Result<()> {
        self.call("projects.unlock", json!({"name": name})).map(|_| ())
    }

    pub fn online(&mut self) -> Result<Vec<OnlineUser>> {
        self.typed("users.online", json!({}))
    }

    // ---- admin ----

    pub fn members(&mut self) -> Result<Vec<Member>> {
        self.typed("team.list", json!({}))
    }

    /// Add (or update) a member. With `password: None` they sign in with Google, or get a
    /// password from the admin later. Returns a generated password when `generate_password`.
    pub fn add_member(&mut self, email: &str, name: &str, role: Role, generate_password: bool) -> Result<Option<String>> {
        let v = self.call("team.add", json!({"email": email, "name": name, "role": role, "generate_password": generate_password}))?;
        Ok(v.get("password").and_then(Value::as_str).map(str::to_string))
    }

    pub fn remove_member(&mut self, email: &str) -> Result<()> {
        self.call("team.remove", json!({"email": email})).map(|_| ())
    }

    /// A new random password for a member (shown once to the admin).
    pub fn reset_password(&mut self, email: &str) -> Result<String> {
        let v = self.call("team.resetPassword", json!({"email": email}))?;
        v.get("password").and_then(Value::as_str).map(str::to_string).ok_or_else(|| OfficeError::Protocol("no password".into()))
    }
}

/// "Continue with Google" for a desktop app (RFC 8252 loopback redirect + PKCE).
pub struct GoogleAuth {
    pub auth_url: String,
    pub redirect_uri: String,
    pub verifier: String,
    state: String,
    listener: std::net::TcpListener,
}

impl GoogleAuth {
    /// Listen on a free loopback port and build the Google sign-in URL for `client_id`.
    pub fn start(client_id: &str, login_hint: Option<&str>) -> Result<Self> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| OfficeError::Network(e.to_string()))?;
        let port = listener.local_addr().map_err(|e| OfficeError::Network(e.to_string()))?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}");
        let verifier = random_token(32)?;
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
        let state = random_token(16)?;
        let mut auth_url = format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}&prompt=select_account",
            url_encode(client_id),
            url_encode(&redirect_uri),
            url_encode("openid email profile"),
        );
        if let Some(h) = login_hint.filter(|h| !h.is_empty()) {
            auth_url.push_str(&format!("&login_hint={}", url_encode(h)));
        }
        Ok(Self { auth_url, redirect_uri, verifier, state, listener })
    }

    /// Wait (up to `timeout`) for the browser to come back; returns the authorization code.
    pub fn wait_for_code(&self, timeout: Duration) -> Result<String> {
        use std::io::{BufRead, BufReader, Read, Write};
        self.listener.set_nonblocking(true).map_err(|e| OfficeError::Network(e.to_string()))?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if std::time::Instant::now() > deadline {
                return Err(OfficeError::Protocol("Google sign-in timed out".into()));
            }
            let (mut stream, _) = match self.listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(e) => return Err(OfficeError::Network(e.to_string())),
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut line = String::new();
            let Ok(clone) = stream.try_clone() else { continue };
            if BufReader::new(clone).take(8192).read_line(&mut line).is_err() {
                continue;
            }
            // GET /?state=…&code=… HTTP/1.1
            let target = line.split_whitespace().nth(1).unwrap_or_default();
            let query = target.split_once('?').map(|(_, q)| q).unwrap_or_default();
            let param = |k: &str| query.split('&').find_map(|kv| kv.split_once('=').filter(|(key, _)| *key == k).map(|(_, v)| url_decode(v)));
            if param("state").is_none() && param("error").is_none() {
                // favicon or another request: ignore
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            }
            let result = match (param("error"), param("code"), param("state")) {
                (Some(e), _, _) => Err(OfficeError::Protocol(format!("Google sign-in was cancelled ({e})"))),
                (None, Some(code), Some(state)) if secret_eq(&state, &self.state) => Ok(code),
                _ => Err(OfficeError::Protocol("Google sign-in reply did not match this request".into())),
            };
            let (title, msg) = if result.is_ok() { ("Signed in", "You're signed in to MattCut. You can close this tab and go back to the app.") } else { ("Sign-in failed", "Go back to MattCut and try again.") };
            let page = format!(
                "<!doctype html><meta charset=utf-8><title>MattCut</title><body style=\"font-family:-apple-system,Segoe UI,sans-serif;background:#0b0b0d;color:#eee;display:grid;place-items:center;height:100vh;margin:0\"><div style=\"text-align:center\"><div style=\"width:56px;height:56px;border-radius:14px;background:#e4583a;margin:0 auto 18px\"></div><h2 style=\"font-weight:500\">{title}</h2><p style=\"color:#999\">{msg}</p></div>"
            );
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
            return result;
        }
    }
}

/// Open `url` in the default browser.
pub fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(windows)]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    r.map(|_| ())
}

/// MattCut's per-user data folder (settings, recovery, the control-port file).
/// `MATTCUT_DATA_DIR` overrides it.
pub fn app_data_dir() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(d) = env("MATTCUT_DATA_DIR") {
        return Some(d);
    }
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support/MattCut"))
    } else if cfg!(windows) {
        env("APPDATA").map(|h| h.join("MattCut"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share"))).map(|d| d.join("MattCut"))
    }
}

/// Written by the running desktop app to `<data dir>/control.json` (owner-only) so the MattCut MCP
/// server can find and authenticate to it. Loopback only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlInfo {
    pub port: u16,
    /// 64 hex characters; sent with every control request.
    pub token: String,
    pub pid: u32,
}

pub const CONTROL_FILE: &str = "control.json";
pub const DEFAULT_CONTROL_PORT: u16 = 9877;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ranges() {
        for ip in ["127.0.0.1", "10.1.2.3", "172.16.0.9", "192.168.1.40", "169.254.3.3", "::1", "fd12::1", "fe80::1", "::ffff:192.168.0.2"] {
            assert!(is_private_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "172.32.0.1", "2001:4860::8888", "::ffff:8.8.8.8"] {
            assert!(!is_private_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn project_names() {
        assert!(valid_project_name("Client Promo (v2)"));
        assert!(valid_project_name("reel_01.final"));
        for bad in ["", "../etc", "a/b", ".hidden", " pad", "x\\y", &"n".repeat(81)] {
            assert!(!valid_project_name(bad), "{bad}");
        }
    }

    #[test]
    fn emails() {
        assert_eq!(normalize_email(" Sara@Mattrics.com ").as_deref(), Some("sara@mattrics.com"));
        for bad in ["", "sara", "@x.com", "a@b", "a b@c.com", "a@.com"] {
            assert!(normalize_email(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn server_urls() {
        assert_eq!(normalize_server_url("mattcut.example.com").unwrap(), "https://mattcut.example.com");
        assert_eq!(normalize_server_url("https://mattcut.example.com/").unwrap(), "https://mattcut.example.com");
        assert_eq!(normalize_server_url("192.168.1.20").unwrap(), "http://192.168.1.20:8780");
        assert_eq!(normalize_server_url("localhost:9000").unwrap(), "http://localhost:9000");
        assert!(normalize_server_url("http://mattcut.example.com").is_err());
        assert!(normalize_server_url("ftp://x").is_err());
        assert!(normalize_server_url("").is_err());
    }

    #[test]
    fn url_coding() {
        assert_eq!(url_encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(url_decode("a%20b%26c+d%2F%C3%A9"), "a b&c d/é");
        assert_eq!(url_decode("100%"), "100%");
    }

    #[test]
    fn secrets() {
        assert!(secret_eq("abc", "abc"));
        assert!(!secret_eq("abc", "abd"));
        assert!(!secret_eq("abc", "abcd"));
        assert_eq!(random_token(16).unwrap().len(), 32);
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
