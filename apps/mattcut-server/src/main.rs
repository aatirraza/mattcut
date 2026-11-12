//! MattCut team server.
//!
//! Run it on one always-on machine (a small cloud server is best; an office PC works too) and
//! put HTTPS in front of it (Caddy or a Cloudflare Tunnel, see docs/server.md). Editors sign in
//! from anywhere with Google or with email + password; only emails on the team list get in.
//!
//! ```text
//! mattcut-server                                   start the server
//! mattcut-server users list
//! mattcut-server users add EMAIL [--name NAME] [--admin] [--generate-password | --password PW]
//! mattcut-server users remove EMAIL
//! mattcut-server users password EMAIL [--password PW]     (new random password when omitted)
//! mattcut-server google CLIENT_ID CLIENT_SECRET           turn on "Continue with Google"
//! --config PATH                                    use another config file (any command)
//! ```

use std::collections::HashMap;
use std::io::Read;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use mattcut_office::{
    AUTH_ERROR_PREFIX, DEFAULT_PORT, Login, MAX_BODY, MAX_PROJECT_BYTES, Member, OnlineUser, PROTOCOL_VERSION, ProjectEntry, Request, Response, Role,
    ServerInfo, decode_bytes, encode_bytes, is_private_ip, normalize_email, random_token, sha256_hex, url_encode, valid_project_name,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_IN_FLIGHT: usize = 64;
/// Previous versions kept per project in `<storage>/.versions`.
const KEEP_VERSIONS: usize = 20;
/// Failed sign-ins allowed per address in [`FAIL_WINDOW`].
const MAX_FAILS: u32 = 10;
const FAIL_WINDOW: Duration = Duration::from_secs(15 * 60);
/// "Online" = called the server this recently.
const ONLINE: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    /// Shown on the sign-in screen.
    team_name: String,
    /// Where the HTTP listener binds (put HTTPS in front of it).
    #[serde(default = "default_listen")]
    listen: String,
    /// The address editors type, e.g. `https://mattcut.mattrics.com` (shown in the log).
    #[serde(default)]
    public_url: Option<String>,
    /// Shared projects and their `.versions`.
    storage_dir: PathBuf,
    /// Where the team keeps footage (shown to editors), e.g. a Google Drive / NAS folder.
    #[serde(default)]
    media_share: Option<String>,
    /// Offline grace: how long an app keeps working without reaching the server.
    #[serde(default = "default_lease")]
    lease_seconds: u64,
    /// How long "remember me" keeps a computer signed in.
    #[serde(default = "default_session_days")]
    session_days: u64,
    /// "Continue with Google" (an OAuth client of type "Desktop app"; see docs/server.md).
    #[serde(default)]
    google_client_id: Option<String>,
    #[serde(default)]
    google_client_secret: Option<String>,
    /// Trust `X-Forwarded-For` / `CF-Connecting-IP` from a proxy on this machine or network.
    #[serde(default = "yes")]
    trust_proxy: bool,
}

fn default_listen() -> String {
    format!("0.0.0.0:{DEFAULT_PORT}")
}
fn default_lease() -> u64 {
    12 * 3600
}
fn default_session_days() -> u64 {
    30
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct User {
    email: String,
    name: String,
    #[serde(default)]
    role: Role,
    /// Argon2id PHC string; None = Google sign-in only.
    #[serde(default)]
    password_hash: Option<String>,
    #[serde(default)]
    added: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    email: String,
    host: String,
    /// Unix seconds.
    expires: i64,
    created: String,
}

struct Live {
    last_seen: Instant,
    editing: Option<String>,
}

#[derive(Default)]
struct State {
    users: Vec<User>,
    users_stamp: Option<SystemTime>,
    /// sha256(token) -> session
    sessions: HashMap<String, StoredSession>,
    live: HashMap<String, Live>,
    /// project -> sha256(token) holding its edit lock
    locks: HashMap<String, String>,
    fails: HashMap<IpAddr, (u32, Instant)>,
}

struct Server {
    cfg: Config,
    dir: PathBuf,
    state: Mutex<State>,
    http: ureq::Agent,
}

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

fn log(msg: &str) {
    println!("{}  {msg}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
}

// ---- persistence --------------------------------------------------------------------------------

fn users_path(dir: &Path) -> PathBuf {
    dir.join("users.json")
}
fn sessions_path(dir: &Path) -> PathBuf {
    dir.join("sessions.json")
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    {
        use std::io::Write;
        let mut f = opts.open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(tmp, path)
}

fn load_users(dir: &Path) -> Vec<User> {
    std::fs::read_to_string(users_path(dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_users(dir: &Path, users: &[User]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(users).map_err(|e| e.to_string())?;
    write_private(&users_path(dir), &text).map_err(|e| format!("users.json: {e}"))
}

fn hash_password(pw: &str) -> Result<String, String> {
    if pw.chars().count() < 8 {
        return Err("passwords need at least 8 characters".into());
    }
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| e.to_string())?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| e.to_string())?;
    argon2::Argon2::default().hash_password(pw.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| e.to_string())
}

fn verify_password(pw: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| argon2::Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
}

/// A readable random password: 4 groups of 4 letters/digits.
fn generate_password() -> Result<String, String> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut raw = [0u8; 16];
    getrandom::fill(&mut raw).map_err(|e| e.to_string())?;
    let chars: Vec<char> = raw.iter().map(|b| ALPHABET.get(usize::from(*b) % ALPHABET.len()).copied().unwrap_or(b'x') as char).collect();
    Ok(chars.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-"))
}

impl Server {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Pick up users.json edits made by `mattcut-server users …` while running.
    fn refresh_users(&self, st: &mut State) {
        let stamp = std::fs::metadata(users_path(&self.dir)).and_then(|m| m.modified()).ok();
        if stamp != st.users_stamp {
            st.users = load_users(&self.dir);
            st.users_stamp = stamp;
        }
    }

    fn save_sessions(&self, st: &State) {
        if let Ok(text) = serde_json::to_string(&st.sessions)
            && let Err(e) = write_private(&sessions_path(&self.dir), &text)
        {
            log(&format!("sessions.json: {e}"));
        }
    }

    fn info(&self) -> ServerInfo {
        ServerInfo {
            team: self.cfg.team_name.clone(),
            protocol: PROTOCOL_VERSION,
            version: env!("CARGO_PKG_VERSION").into(),
            google_client_id: self.cfg.google_client_id.clone().filter(|_| self.cfg.google_client_secret.is_some()),
            media_share: self.cfg.media_share.clone(),
        }
    }

    fn lease(&self) -> u64 {
        self.cfg.lease_seconds.clamp(60, 30 * 86_400)
    }

    fn too_many_fails(&self, st: &mut State, ip: IpAddr) -> bool {
        match st.fails.get(&ip) {
            Some((n, since)) if since.elapsed() < FAIL_WINDOW => *n >= MAX_FAILS,
            _ => false,
        }
    }

    fn note_fail(&self, st: &mut State, ip: IpAddr) {
        let e = st.fails.entry(ip).or_insert((0, Instant::now()));
        if e.1.elapsed() >= FAIL_WINDOW {
            *e = (0, Instant::now());
        }
        e.0 += 1;
    }

    /// Create a session for `user`.
    fn start_session(&self, st: &mut State, user: &User, host: &str) -> Result<Value, String> {
        let token = random_token(32).map_err(|e| e.to_string())?;
        let key = sha256_hex(token.as_bytes());
        let days = i64::try_from(self.cfg.session_days.clamp(1, 365)).unwrap_or(30);
        st.sessions.insert(key.clone(), StoredSession { email: user.email.clone(), host: host.into(), expires: now_unix() + days * 86_400, created: chrono::Utc::now().to_rfc3339() });
        st.live.insert(key, Live { last_seen: Instant::now(), editing: None });
        self.save_sessions(st);
        log(&format!("{} signed in from {host}", user.email));
        self.login_reply(user, token)
    }

    fn login_reply(&self, user: &User, token: String) -> Result<Value, String> {
        to_value(&Login {
            session: token,
            email: user.email.clone(),
            name: user.name.clone(),
            role: user.role,
            team: self.cfg.team_name.clone(),
            lease_seconds: self.lease(),
            media_share: self.cfg.media_share.clone(),
        })
    }

    fn handle(&self, req: Request, ip: IpAddr) -> Response {
        let id = req.id;
        match self.dispatch(&req, ip) {
            Ok(v) => Response::ok(id, v),
            Err(e) => Response::err(id, e),
        }
    }

    fn dispatch(&self, req: &Request, ip: IpAddr) -> Result<Value, String> {
        let p = &req.params;
        let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
        let host = s("host").unwrap_or_else(|| ip.to_string()).chars().take(80).collect::<String>();
        match req.method.as_str() {
            "hello" => to_value(&self.info()),
            "login.password" => {
                let mut st = self.state();
                if self.too_many_fails(&mut st, ip) {
                    return Err("too many failed sign-ins: wait 15 minutes".into());
                }
                self.refresh_users(&mut st);
                let email = s("email").and_then(|e| normalize_email(&e)).unwrap_or_default();
                let pw = s("password").unwrap_or_default();
                let user = st.users.iter().find(|u| u.email == email).cloned();
                let ok = match user.as_ref().and_then(|u| u.password_hash.as_deref()) {
                    Some(h) => verify_password(&pw, h),
                    None => {
                        // same work for unknown emails, so timing does not reveal who exists
                        let _ = verify_password(&pw, "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$Jc5yXzFvPCiG0g3IP4X2RA7NAmdDeA3aW0XQ5w0yx7k");
                        false
                    }
                };
                match user {
                    Some(u) if ok => self.start_session(&mut st, &u, &host),
                    Some(u) if u.password_hash.is_none() => Err(format!("{} signs in with Google", u.email)),
                    _ => {
                        self.note_fail(&mut st, ip);
                        log(&format!("failed password sign-in for `{email}` from {ip}"));
                        Err("wrong email or password".into())
                    }
                }
            }
            "login.google" => {
                {
                    let mut st = self.state();
                    if self.too_many_fails(&mut st, ip) {
                        return Err("too many failed sign-ins: wait 15 minutes".into());
                    }
                }
                let email = self.google_email(&s("code").unwrap_or_default(), &s("verifier").unwrap_or_default(), &s("redirect_uri").unwrap_or_default())?;
                let mut st = self.state();
                self.refresh_users(&mut st);
                match st.users.iter().find(|u| u.email == email).cloned() {
                    Some(u) => self.start_session(&mut st, &u, &host),
                    None => {
                        self.note_fail(&mut st, ip);
                        log(&format!("refused Google sign-in for {email}: not on the team list"));
                        Err(format!("{email} is not on the {} team. Ask your admin to add you.", self.cfg.team_name))
                    }
                }
            }
            _ => {
                let token = req.session.clone().ok_or_else(|| format!("{AUTH_ERROR_PREFIX}."))?;
                let key = sha256_hex(token.as_bytes());
                let mut st = self.state();
                self.refresh_users(&mut st);
                let Some(sess) = st.sessions.get(&key).cloned() else { return Err(format!("{AUTH_ERROR_PREFIX}: you were signed out.")) };
                if sess.expires < now_unix() {
                    st.sessions.remove(&key);
                    self.save_sessions(&st);
                    return Err(format!("{AUTH_ERROR_PREFIX}: your sign-in expired."));
                }
                let Some(user) = st.users.iter().find(|u| u.email == sess.email).cloned() else {
                    st.sessions.remove(&key);
                    st.locks.retain(|_, h| *h != key);
                    self.save_sessions(&st);
                    return Err(format!("{AUTH_ERROR_PREFIX}: your account is no longer on this team."));
                };
                st.live.entry(key.clone()).or_insert(Live { last_seen: Instant::now(), editing: None }).last_seen = Instant::now();
                if req.method == "session.resume" {
                    if let Some(sx) = st.sessions.get_mut(&key) {
                        sx.host = host;
                    }
                    return self.login_reply(&user, token);
                }
                self.authed(&req.method, p, &key, &user, &mut st)
            }
        }
    }

    /// Swap a Google authorization code for the account's verified email.
    fn google_email(&self, code: &str, verifier: &str, redirect_uri: &str) -> Result<String, String> {
        let (Some(id), Some(secret)) = (&self.cfg.google_client_id, &self.cfg.google_client_secret) else {
            return Err("Google sign-in is not set up on this server".into());
        };
        if code.is_empty() || verifier.is_empty() || !redirect_uri.starts_with("http://127.0.0.1:") {
            return Err("bad Google sign-in request".into());
        }
        let form = format!(
            "code={}&client_id={}&client_secret={}&redirect_uri={}&grant_type=authorization_code&code_verifier={}",
            url_encode(code),
            url_encode(id),
            url_encode(secret),
            url_encode(redirect_uri),
            url_encode(verifier)
        );
        let mut resp = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send(form)
            .map_err(|e| format!("Google: {e}"))?;
        let body: Value = serde_json::from_str(&resp.body_mut().read_to_string().map_err(|e| format!("Google: {e}"))?).map_err(|e| format!("Google: {e}"))?;
        let id_token = body.get("id_token").and_then(Value::as_str).ok_or_else(|| {
            format!("Google refused the sign-in: {}", body.get("error_description").or(body.get("error")).and_then(Value::as_str).unwrap_or("no token"))
        })?;
        // Google checks the token's signature and expiry for us here.
        let mut info = self
            .http
            .get(format!("https://oauth2.googleapis.com/tokeninfo?id_token={}", url_encode(id_token)))
            .call()
            .map_err(|e| format!("Google: {e}"))?;
        let claims: Value = serde_json::from_str(&info.body_mut().read_to_string().map_err(|e| format!("Google: {e}"))?).map_err(|e| format!("Google: {e}"))?;
        let aud_ok = claims.get("aud").and_then(Value::as_str) == Some(id.as_str());
        let iss_ok = matches!(claims.get("iss").and_then(Value::as_str), Some("accounts.google.com" | "https://accounts.google.com"));
        let verified = matches!(claims.get("email_verified"), Some(Value::String(v)) if v == "true") || claims.get("email_verified") == Some(&Value::Bool(true));
        if !aud_ok || !iss_ok || !verified {
            return Err("Google sign-in could not be verified".into());
        }
        claims.get("email").and_then(Value::as_str).and_then(normalize_email).ok_or_else(|| "Google did not share an email address".into())
    }

    fn authed(&self, method: &str, p: &Value, key: &str, user: &User, st: &mut State) -> Result<Value, String> {
        let name = || -> Result<String, String> {
            let n = p.get("name").and_then(Value::as_str).ok_or("missing project name")?;
            if valid_project_name(n) { Ok(n.to_string()) } else { Err(format!("`{n}` is not a valid project name")) }
        };
        let admin = || -> Result<(), String> { if user.role == Role::Admin { Ok(()) } else { Err("only team admins can do that".into()) } };
        let who = |st: &State, holder: &str| st.sessions.get(holder).and_then(|s| st.users.iter().find(|u| u.email == s.email)).map_or_else(|| "someone else".to_string(), |u| u.name.clone());
        match method {
            "heartbeat" => {
                let editing = p.get("editing").and_then(Value::as_str).map(str::to_string);
                if let Some(l) = st.live.get_mut(key) {
                    l.editing = editing;
                }
                Ok(json!({"lease_seconds": self.lease()}))
            }
            "logout" => {
                st.sessions.remove(key);
                st.live.remove(key);
                st.locks.retain(|_, h| h != key);
                self.save_sessions(st);
                log(&format!("{} signed out", user.email));
                Ok(Value::Null)
            }
            "password.change" => {
                let old = p.get("old").and_then(Value::as_str).unwrap_or_default();
                let new = p.get("new").and_then(Value::as_str).unwrap_or_default();
                if let Some(h) = &user.password_hash
                    && !verify_password(old, h)
                {
                    return Err("your current password is not right".into());
                }
                let hash = hash_password(new)?;
                if let Some(u) = st.users.iter_mut().find(|u| u.email == user.email) {
                    u.password_hash = Some(hash);
                }
                save_users(&self.dir, &st.users)?;
                st.users_stamp = std::fs::metadata(users_path(&self.dir)).and_then(|m| m.modified()).ok();
                Ok(Value::Null)
            }
            "users.online" => {
                let users: Vec<OnlineUser> = st
                    .live
                    .iter()
                    .filter(|(_, l)| l.last_seen.elapsed() < ONLINE)
                    .filter_map(|(k, l)| {
                        let s = st.sessions.get(k)?;
                        let name = st.users.iter().find(|u| u.email == s.email).map_or_else(|| s.email.clone(), |u| u.name.clone());
                        Some(OnlineUser { user: name, host: s.host.clone(), editing: l.editing.clone() })
                    })
                    .collect();
                to_value(&users)
            }
            "projects.list" => {
                let mut out = Vec::new();
                let dir = std::fs::read_dir(&self.cfg.storage_dir).map_err(|e| format!("storage: {e}"))?;
                for entry in dir.flatten() {
                    let path = entry.path();
                    let Some(stem) = project_stem(&path) else { continue };
                    let meta = entry.metadata().map_err(|e| e.to_string())?;
                    let locked_by = st.locks.get(&stem).map(|h| who(st, h));
                    out.push(ProjectEntry {
                        modified: meta.modified().map(|m| chrono::DateTime::<chrono::Utc>::from(m).to_rfc3339()).unwrap_or_default(),
                        modified_by: read_author(&self.cfg.storage_dir, &stem),
                        size: meta.len(),
                        locked_by,
                        name: stem,
                    });
                }
                out.sort_by(|a, b| b.modified.cmp(&a.modified));
                to_value(&out)
            }
            "projects.download" => {
                let n = name()?;
                let data = std::fs::read(self.project_path(&n)).map_err(|e| format!("{n}: {e}"))?;
                Ok(json!({"name": n, "data": encode_bytes(&data)}))
            }
            "projects.upload" => {
                let n = name()?;
                if let Some(holder) = st.locks.get(&n)
                    && holder != key
                {
                    return Err(format!("{n} is being edited by {}; save a copy under another name", who(st, holder)));
                }
                let data = decode_bytes(p.get("data").and_then(Value::as_str).unwrap_or_default()).map_err(|e| e.to_string())?;
                if data.is_empty() || data.len() > MAX_PROJECT_BYTES {
                    return Err(format!("project must be 1 byte to {} MB", MAX_PROJECT_BYTES / (1024 * 1024)));
                }
                let path = self.project_path(&n);
                self.keep_version(&n, &path);
                atomic_write(&path, &data).map_err(|e| format!("saving {n}: {e}"))?;
                let _ = std::fs::write(self.cfg.storage_dir.join(".authors").join(format!("{n}.txt")), &user.name);
                log(&format!("{} saved {n} ({} KB)", user.email, data.len() / 1024));
                to_value(&ProjectEntry {
                    name: n,
                    size: data.len() as u64,
                    modified: chrono::Utc::now().to_rfc3339(),
                    modified_by: Some(user.name.clone()),
                    locked_by: Some(user.name.clone()),
                })
            }
            "projects.lock" => {
                let n = name()?;
                let force = p.get("force").and_then(Value::as_bool).unwrap_or(false);
                // a lock held by a session that went quiet is free
                if let Some(holder) = st.locks.get(&n).cloned()
                    && holder != key
                    && !force
                    && st.live.get(&holder).is_some_and(|l| l.last_seen.elapsed() < Duration::from_secs(self.lease()))
                {
                    return Err(format!("locked by {}", who(st, &holder)));
                }
                st.locks.insert(n.clone(), key.into());
                Ok(Value::Null)
            }
            "projects.unlock" => {
                let n = name()?;
                if st.locks.get(&n).is_some_and(|h| h == key) {
                    st.locks.remove(&n);
                }
                Ok(Value::Null)
            }
            "team.list" => {
                admin()?;
                let list: Vec<Member> = st.users.iter().map(|u| Member { email: u.email.clone(), name: u.name.clone(), role: u.role, has_password: u.password_hash.is_some() }).collect();
                to_value(&list)
            }
            "team.add" => {
                admin()?;
                let email = p.get("email").and_then(Value::as_str).and_then(normalize_email).ok_or("enter a valid email")?;
                let name = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| email.split('@').next().unwrap_or("").to_string(), str::to_string);
                let role: Role = p.get("role").cloned().and_then(|r| serde_json::from_value(r).ok()).unwrap_or_default();
                let password = if p.get("generate_password").and_then(Value::as_bool).unwrap_or(false) { Some(generate_password()?) } else { None };
                let hash = password.as_deref().map(hash_password).transpose()?;
                match st.users.iter_mut().find(|u| u.email == email) {
                    Some(u) => {
                        u.name = name;
                        u.role = role;
                        if hash.is_some() {
                            u.password_hash = hash;
                        }
                    }
                    None => st.users.push(User { email: email.clone(), name, role, password_hash: hash, added: chrono::Utc::now().to_rfc3339() }),
                }
                save_users(&self.dir, &st.users)?;
                st.users_stamp = std::fs::metadata(users_path(&self.dir)).and_then(|m| m.modified()).ok();
                log(&format!("{} added {email} to the team", user.email));
                Ok(json!({"password": password}))
            }
            "team.remove" => {
                admin()?;
                let email = p.get("email").and_then(Value::as_str).and_then(normalize_email).ok_or("enter a valid email")?;
                if email == user.email {
                    return Err("you can't remove yourself".into());
                }
                st.users.retain(|u| u.email != email);
                let gone: Vec<String> = st.sessions.iter().filter(|(_, s)| s.email == email).map(|(k, _)| k.clone()).collect();
                for k in &gone {
                    st.sessions.remove(k);
                    st.live.remove(k);
                }
                st.locks.retain(|_, h| !gone.contains(h));
                save_users(&self.dir, &st.users)?;
                st.users_stamp = std::fs::metadata(users_path(&self.dir)).and_then(|m| m.modified()).ok();
                self.save_sessions(st);
                log(&format!("{} removed {email} from the team", user.email));
                Ok(Value::Null)
            }
            "team.resetPassword" => {
                admin()?;
                let email = p.get("email").and_then(Value::as_str).and_then(normalize_email).ok_or("enter a valid email")?;
                let pw = generate_password()?;
                let hash = hash_password(&pw)?;
                let u = st.users.iter_mut().find(|u| u.email == email).ok_or("no such team member")?;
                u.password_hash = Some(hash);
                save_users(&self.dir, &st.users)?;
                st.users_stamp = std::fs::metadata(users_path(&self.dir)).and_then(|m| m.modified()).ok();
                Ok(json!({"password": pw}))
            }
            other => Err(format!("unknown method `{other}`")),
        }
    }

    fn project_path(&self, name: &str) -> PathBuf {
        self.cfg.storage_dir.join(format!("{name}.fcproj"))
    }

    /// Copy the current file to `.versions/<name>@<time>.fcproj` and keep the newest few.
    fn keep_version(&self, name: &str, path: &Path) {
        if !path.exists() {
            return;
        }
        let dir = self.cfg.storage_dir.join(".versions");
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        if std::fs::copy(path, dir.join(format!("{name}@{stamp}.fcproj"))).is_err() {
            return;
        }
        let prefix = format!("{name}@");
        let mut old: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.starts_with(&prefix))).collect())
            .unwrap_or_default();
        old.sort();
        let excess = old.len().saturating_sub(KEEP_VERSIONS);
        for p in old.into_iter().take(excess) {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Drop expired sessions (hourly).
    fn sweep(&self) {
        let mut st = self.state();
        let now = now_unix();
        let before = st.sessions.len();
        st.sessions.retain(|_, s| s.expires > now);
        if st.sessions.len() != before {
            let keys: Vec<String> = st.sessions.keys().cloned().collect();
            st.live.retain(|k, _| keys.contains(k));
            self.save_sessions(&st);
        }
        st.fails.retain(|_, (_, since)| since.elapsed() < FAIL_WINDOW);
    }
}

fn to_value<T: Serialize>(v: &T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

fn project_stem(path: &Path) -> Option<String> {
    if path.extension().and_then(|e| e.to_str()) != Some("fcproj") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?.to_string();
    valid_project_name(&stem).then_some(stem)
}

fn read_author(storage: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(storage.join(".authors").join(format!("{name}.txt"))).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("fcproj.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

// ---- HTTP -------------------------------------------------------------------------------------------

fn header(name: &str, value: &str) -> Option<tiny_http::Header> {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

/// The client's address: the proxy's forwarded address when the proxy is local and trusted.
fn client_ip(req: &tiny_http::Request, trust_proxy: bool) -> IpAddr {
    let peer = req.remote_addr().map_or(IpAddr::from([0, 0, 0, 0]), |a| a.ip());
    if trust_proxy && is_private_ip(peer) {
        for h in req.headers() {
            let f = h.field.as_str().as_str().to_ascii_lowercase();
            if (f == "cf-connecting-ip" || f == "x-forwarded-for")
                && let Some(ip) = h.value.as_str().split(',').next().and_then(|v| v.trim().parse().ok())
            {
                return ip;
            }
        }
    }
    peer
}

fn serve(server: &Server, mut req: tiny_http::Request) {
    let json_type = header("Content-Type", "application/json");
    let url = req.url().to_string();
    let method = req.method().clone();
    match (method, url.as_str()) {
        (tiny_http::Method::Post, "/api") => {
            if req.body_length().is_some_and(|n| n as u64 > MAX_BODY) {
                let _ = req.respond(tiny_http::Response::from_string("too large").with_status_code(413));
                return;
            }
            let ip = client_ip(&req, server.cfg.trust_proxy);
            let mut body = String::new();
            if req.as_reader().take(MAX_BODY).read_to_string(&mut body).is_err() {
                let _ = req.respond(tiny_http::Response::from_string("bad body").with_status_code(400));
                return;
            }
            let resp = match serde_json::from_str::<Request>(&body) {
                Ok(r) => server.handle(r, ip),
                Err(e) => Response::err(0, format!("bad request: {e}")),
            };
            let text = serde_json::to_string(&resp).unwrap_or_else(|_| r#"{"id":0,"ok":false,"error":"encode"}"#.into());
            let mut r = tiny_http::Response::from_string(text);
            if let Some(h) = json_type {
                r = r.with_header(h);
            }
            let _ = req.respond(r);
        }
        (tiny_http::Method::Get, "/health") => {
            let _ = req.respond(tiny_http::Response::from_string("ok"));
        }
        (tiny_http::Method::Get, "/") => {
            let page = format!(
                "<!doctype html><meta charset=utf-8><title>MattCut server</title><body style=\"font-family:-apple-system,Segoe UI,sans-serif;background:#0b0b0d;color:#eee;display:grid;place-items:center;height:100vh;margin:0\"><div style=\"text-align:center\"><div style=\"width:56px;height:56px;border-radius:14px;background:#e4583a;margin:0 auto 18px\"></div><h2 style=\"font-weight:500\">MattCut server — {}</h2><p style=\"color:#999\">This address goes into the MattCut app's sign-in screen.</p></div>",
                html_escape(&server.cfg.team_name)
            );
            let mut r = tiny_http::Response::from_string(page);
            if let Some(h) = header("Content-Type", "text/html; charset=utf-8") {
                r = r.with_header(h);
            }
            let _ = req.respond(r);
        }
        _ => {
            let _ = req.respond(tiny_http::Response::from_string("not found").with_status_code(404));
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ---- config / CLI -----------------------------------------------------------------------------------

fn default_config_path() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("PROGRAMDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support")).unwrap_or_else(|| PathBuf::from("."))
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")).unwrap_or_else(|| PathBuf::from("."))
    };
    base.join("MattCut Server").join("mattcut-server.json")
}

fn load_or_create(path: &Path) -> Result<Config, String> {
    if let Ok(text) = std::fs::read_to_string(path) {
        return serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()));
    }
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let cfg = Config {
        team_name: "Mattrics".into(),
        listen: default_listen(),
        public_url: None,
        storage_dir: dir.join("Projects"),
        media_share: None,
        lease_seconds: default_lease(),
        session_days: default_session_days(),
        google_client_id: None,
        google_client_secret: None,
        trust_proxy: true,
    };
    save_config(path, &cfg)?;
    log(&format!("created {} (edit it to set the team name and address)", path.display()));
    Ok(cfg)
}

fn save_config(path: &Path, cfg: &Config) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    write_private(path, &text).map_err(|e| format!("{}: {e}", path.display()))
}

fn users_cmd(dir: &Path, args: &[String]) -> Result<(), String> {
    let mut users = load_users(dir);
    let opt = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let email = || args.get(1).and_then(|e| normalize_email(e)).ok_or_else(|| "give an email address".to_string());
    match args.first().map(String::as_str) {
        Some("list") | None => {
            if users.is_empty() {
                println!("No team members yet. Add yourself:\n  mattcut-server users add you@company.com --admin --generate-password");
            }
            for u in &users {
                println!("{:<36} {:<24} {:<7} {}", u.email, u.name, if u.role == Role::Admin { "admin" } else { "editor" }, if u.password_hash.is_some() { "password + Google" } else { "Google only" });
            }
        }
        Some("add") => {
            let email = email()?;
            let name = opt("--name").unwrap_or_else(|| email.split('@').next().unwrap_or("").to_string());
            let role = if flag("--admin") { Role::Admin } else { Role::Editor };
            let password = if let Some(p) = opt("--password") { Some(p) } else if flag("--generate-password") { Some(generate_password()?) } else { None };
            let hash = password.as_deref().map(hash_password).transpose()?;
            match users.iter_mut().find(|u| u.email == email) {
                Some(u) => {
                    u.name = name;
                    u.role = role;
                    if hash.is_some() {
                        u.password_hash = hash;
                    }
                }
                None => users.push(User { email: email.clone(), name, role, password_hash: hash, added: chrono::Utc::now().to_rfc3339() }),
            }
            save_users(dir, &users)?;
            println!("added {email}");
            if let Some(p) = password.filter(|_| opt("--password").is_none()) {
                println!("password: {p}   (share it privately; they can change it in MattCut)");
            }
        }
        Some("remove") => {
            let email = email()?;
            let before = users.len();
            users.retain(|u| u.email != email);
            if users.len() == before {
                return Err(format!("{email} is not on the team"));
            }
            save_users(dir, &users)?;
            println!("removed {email} (they are signed out at their next check-in)");
        }
        Some("password") => {
            let email = email()?;
            let pw = match opt("--password") {
                Some(p) => p,
                None => generate_password()?,
            };
            let hash = hash_password(&pw)?;
            let u = users.iter_mut().find(|u| u.email == email).ok_or("no such team member")?;
            u.password_hash = Some(hash);
            save_users(dir, &users)?;
            println!("new password for {email}: {pw}");
        }
        Some(other) => return Err(format!("unknown `users {other}` (list, add, remove, password)")),
    }
    Ok(())
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut config_path = default_config_path();
    if let Some(i) = args.iter().position(|a| a == "--config") {
        match args.get(i + 1) {
            Some(p) => config_path = PathBuf::from(p),
            None => fail("--config needs a path"),
        }
        args.drain(i..=i + 1);
    }
    if args.first().is_some_and(|a| a == "-h" || a == "--help") {
        println!("mattcut-server [--config PATH] [users list|add|remove|password …] [google CLIENT_ID CLIENT_SECRET]");
        return;
    }
    let mut cfg = load_or_create(&config_path).unwrap_or_else(|e| fail(&e));
    let dir = config_path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    match args.first().map(String::as_str) {
        Some("users") => {
            if let Err(e) = users_cmd(&dir, args.get(1..).unwrap_or_default()) {
                fail(&e);
            }
            return;
        }
        Some("google") => {
            let (Some(id), Some(secret)) = (args.get(1), args.get(2)) else { fail("usage: mattcut-server google CLIENT_ID CLIENT_SECRET") };
            cfg.google_client_id = Some(id.clone());
            cfg.google_client_secret = Some(secret.clone());
            save_config(&config_path, &cfg).unwrap_or_else(|e| fail(&e));
            println!("\"Continue with Google\" is on (restart the server)");
            return;
        }
        Some(other) => fail(&format!("unknown command `{other}` (try --help)")),
        None => {}
    }
    for sub in ["", ".versions", ".authors"] {
        if let Err(e) = std::fs::create_dir_all(cfg.storage_dir.join(sub)) {
            fail(&format!("storage {}: {e}", cfg.storage_dir.display()));
        }
    }
    let http = tiny_http::Server::http(&cfg.listen).unwrap_or_else(|e| fail(&format!("listening on {}: {e}", cfg.listen)));
    let sessions: HashMap<String, StoredSession> = std::fs::read_to_string(sessions_path(&dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let users = load_users(&dir);
    let server = Arc::new(Server {
        state: Mutex::new(State { users_stamp: std::fs::metadata(users_path(&dir)).and_then(|m| m.modified()).ok(), users, sessions, ..Default::default() }),
        dir,
        http: mattcut_office::http_agent(Duration::from_secs(20)),
        cfg,
    });

    {
        let st = server.state();
        println!("MattCut server {} — {}", env!("CARGO_PKG_VERSION"), server.cfg.team_name);
        println!("  config:   {}", config_path.display());
        println!("  projects: {}", server.cfg.storage_dir.display());
        println!("  listening on http://{}  (put HTTPS in front: see docs/server.md)", server.cfg.listen);
        if let Some(u) = &server.cfg.public_url {
            println!("  editors sign in at: {u}");
        }
        println!("  sign-in:  email + password{}", if server.info().google_client_id.is_some() { ", Google" } else { "   (Google: mattcut-server google CLIENT_ID CLIENT_SECRET)" });
        println!("  team:     {} member(s){}", st.users.len(), if st.users.is_empty() { " — add yourself: mattcut-server users add you@company.com --admin --generate-password" } else { "" });
        println!("  offline grace: {} h", server.lease() / 3600);
    }
    {
        let s = Arc::clone(&server);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(3600));
                s.sweep();
            }
        });
    }
    let in_flight = Arc::new(AtomicUsize::new(0));
    for req in http.incoming_requests() {
        if in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
            in_flight.fetch_sub(1, Ordering::SeqCst);
            let _ = req.respond(tiny_http::Response::from_string("busy").with_status_code(503));
            continue;
        }
        let s = Arc::clone(&server);
        let n = Arc::clone(&in_flight);
        let spawned = std::thread::Builder::new().name("req".into()).spawn(move || {
            serve(&s, req);
            n.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            in_flight.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("mattcut-server: {msg}");
    std::process::exit(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn passwords_hash_and_verify() {
        let h = hash_password("correct horse").unwrap();
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong horse!", &h));
        assert!(hash_password("short").is_err());
        let g = generate_password().unwrap();
        assert_eq!(g.len(), 19);
        assert!(verify_password(&g, &hash_password(&g).unwrap()));
    }

    #[test]
    fn dummy_hash_parses() {
        // the constant-time stand-in must be a valid PHC string, or unknown emails return faster
        assert!(PasswordHash::new("$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$Jc5yXzFvPCiG0g3IP4X2RA7NAmdDeA3aW0XQ5w0yx7k").is_ok());
    }
}
