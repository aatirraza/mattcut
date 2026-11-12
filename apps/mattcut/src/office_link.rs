//! The app's link to the MattCut server, on a background thread: sign-in (email or Google),
//! the session heartbeat and offline grace, shared-project transfers and team admin. The UI sends
//! [`Cmd`]s and reads [`Reply`]s.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use mattcut_office::{GoogleAuth, Login, Member, OfficeClient, OfficeError, OnlineUser, ProjectEntry, Role, ServerInfo};

const HEARTBEAT: Duration = Duration::from_secs(30);
const GOOGLE_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub enum Cmd {
    Hello {
        server: String,
    },
    LoginPassword {
        server: String,
        email: String,
        password: String,
        host: String,
    },
    LoginGoogle {
        server: String,
        host: String,
        hint: Option<String>,
    },
    /// Start with a saved session; `cached` keeps the app usable offline within the grace period.
    Resume {
        server: String,
        session: String,
        host: String,
        cached: Option<(Login, i64)>,
    },
    ListProjects,
    Download {
        name: String,
    },
    Upload {
        name: String,
        data: Vec<u8>,
    },
    Lock {
        name: String,
        force: bool,
    },
    Unlock {
        name: String,
    },
    Online,
    Editing(Option<String>),
    ChangePassword {
        old: String,
        new: String,
    },
    Members,
    AddMember {
        email: String,
        name: String,
        role: Role,
        password: bool,
    },
    RemoveMember {
        email: String,
    },
    ResetPassword {
        email: String,
    },
    Logout,
}

pub enum Reply {
    Hello(Result<ServerInfo, String>),
    /// A browser window opened for Google; waiting for it.
    GoogleWaiting,
    SignedIn {
        login: Login,
        info: Option<ServerInfo>,
        server: String,
        offline: bool,
    },
    SignInFailed(String),
    /// The server ended the session (signed out elsewhere, removed from the team, expired).
    SignedOut(String),
    Projects(Vec<ProjectEntry>),
    Downloaded {
        name: String,
        data: Vec<u8>,
    },
    Uploaded(ProjectEntry),
    Locked {
        name: String,
    },
    LockFailed {
        name: String,
        error: String,
    },
    Online(Vec<OnlineUser>),
    Members(Vec<Member>),
    /// A generated password to show the admin once.
    NewPassword {
        email: String,
        password: Option<String>,
    },
    Done(String),
    Error(String),
}

#[derive(Default, Clone)]
pub struct LinkStatus {
    /// Last time the server answered.
    pub last_ok: Option<Instant>,
    /// Offline grace.
    pub lease: Duration,
    pub login: Option<Login>,
    pub info: Option<ServerInfo>,
    pub server: Option<String>,
    pub last_error: Option<String>,
}

pub struct OfficeLink {
    tx: Sender<Cmd>,
    rx: Receiver<Reply>,
    status: Arc<Mutex<LinkStatus>>,
}

impl OfficeLink {
    pub fn start(ctx: egui::Context) -> Self {
        let (tx, worker_rx) = channel::<Cmd>();
        let (worker_tx, rx) = channel::<Reply>();
        let status = Arc::new(Mutex::new(LinkStatus::default()));
        let st = Arc::clone(&status);
        let spawned = std::thread::Builder::new().name("server-link".into()).spawn(move || worker(worker_rx, worker_tx, st, ctx));
        if let Err(e) = spawned {
            log::error!("server link thread: {e}");
        }
        Self { tx, rx, status }
    }

    pub fn send(&self, cmd: Cmd) {
        if crate::config::STANDALONE {
            return;
        }
        let _ = self.tx.send(cmd);
    }

    pub fn poll(&self) -> Vec<Reply> {
        self.rx.try_iter().collect()
    }

    pub fn status(&self) -> LinkStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Offline grace left (None: not signed in).
    pub fn lease_left(&self) -> Option<Duration> {
        let st = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        st.login.as_ref()?;
        let last = st.last_ok?;
        Some(st.lease.saturating_sub(last.elapsed()))
    }

    /// Signed in, and the server answered within the grace period.
    pub fn licensed(&self) -> bool {
        crate::config::STANDALONE || self.lease_left().is_some_and(|d| !d.is_zero())
    }

    /// The server answered recently.
    pub fn online_now(&self) -> bool {
        let st = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        st.last_ok.is_some_and(|t| t.elapsed() < HEARTBEAT * 2) && st.last_error.is_none()
    }
}

fn worker(rx: Receiver<Cmd>, tx: Sender<Reply>, status: Arc<Mutex<LinkStatus>>, ctx: egui::Context) {
    let mut client: Option<OfficeClient> = None;
    let mut editing: Option<String> = None;
    let mut last_beat = Instant::now();
    let set = |f: &dyn Fn(&mut LinkStatus)| {
        let mut st = status.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut st);
    };
    let mark_ok = || {
        set(&|s| {
            s.last_ok = Some(Instant::now());
            s.last_error = None;
        })
    };
    let send = |r: Reply| {
        let ok = tx.send(r).is_ok();
        ctx.request_repaint();
        ok
    };
    // the server ended the session: forget it and tell the UI
    let signed_out = |e: &OfficeError, client: &mut Option<OfficeClient>| -> bool {
        if e.is_auth() {
            *client = None;
            set(&|s| {
                s.login = None;
                s.last_ok = None;
            });
            let _ = tx.send(Reply::SignedOut(e.to_string()));
            ctx.request_repaint();
            true
        } else {
            false
        }
    };
    let signed_in = |c: &mut OfficeClient, login: Login, info: Option<ServerInfo>, offline: bool, last_ok: Instant| {
        let server = c.base_url().to_string();
        let (l2, i2, s2) = (login.clone(), info.clone(), server.clone());
        set(&|s| {
            s.lease = Duration::from_secs(l2.lease_seconds.max(60));
            s.login = Some(l2.clone());
            if i2.is_some() {
                s.info = i2.clone();
            }
            s.server = Some(s2.clone());
            s.last_ok = Some(last_ok);
            s.last_error = if offline { Some("offline".into()) } else { None };
        });
        Reply::SignedIn { login, info, server, offline }
    };
    loop {
        let wait = HEARTBEAT.saturating_sub(last_beat.elapsed()).max(Duration::from_millis(50));
        let cmd = match rx.recv_timeout(wait) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if last_beat.elapsed() >= HEARTBEAT {
            last_beat = Instant::now();
            if let Some(c) = client.as_mut() {
                match c.heartbeat(editing.as_deref()) {
                    Ok(lease) => {
                        mark_ok();
                        set(&|s| s.lease = Duration::from_secs(lease.max(60)));
                    }
                    Err(e) => {
                        if !signed_out(&e, &mut client) {
                            set(&|s| s.last_error = Some(e.to_string()));
                        }
                    }
                }
                ctx.request_repaint();
            }
        }
        let Some(cmd) = cmd else { continue };
        let reply = match cmd {
            Cmd::Hello { server } => Reply::Hello(OfficeClient::new(&server).and_then(|mut c| c.hello()).map_err(|e| e.to_string())),
            Cmd::LoginPassword { server, email, password, host } => {
                let r = (|| {
                    let mut c = OfficeClient::new(&server)?;
                    let info = c.hello().ok();
                    let login = c.login_password(&email, &password, &host)?;
                    Ok::<_, OfficeError>((c, login, info))
                })();
                match r {
                    Ok((mut c, login, info)) => {
                        let reply = signed_in(&mut c, login, info, false, Instant::now());
                        client = Some(c);
                        last_beat = Instant::now();
                        reply
                    }
                    Err(e) => Reply::SignInFailed(e.to_string()),
                }
            }
            Cmd::LoginGoogle { server, host, hint } => {
                let r = (|| {
                    let mut c = OfficeClient::new(&server)?;
                    let info = c.hello()?;
                    let id = info.google_client_id.clone().ok_or_else(|| OfficeError::Protocol("Google sign-in is not set up on this server yet".into()))?;
                    let auth = GoogleAuth::start(&id, hint.as_deref())?;
                    mattcut_office::open_browser(&auth.auth_url).map_err(|e| OfficeError::Protocol(format!("could not open the browser: {e}")))?;
                    let _ = tx.send(Reply::GoogleWaiting);
                    ctx.request_repaint();
                    let code = auth.wait_for_code(GOOGLE_TIMEOUT)?;
                    let login = c.login_google(&code, &auth.verifier, &auth.redirect_uri, &host)?;
                    Ok::<_, OfficeError>((c, login, info))
                })();
                match r {
                    Ok((mut c, login, info)) => {
                        let reply = signed_in(&mut c, login, Some(info), false, Instant::now());
                        client = Some(c);
                        last_beat = Instant::now();
                        reply
                    }
                    Err(e) => Reply::SignInFailed(e.to_string()),
                }
            }
            Cmd::Resume { server, session, host, cached } => match OfficeClient::new(&server) {
                Err(e) => Reply::SignInFailed(e.to_string()),
                Ok(mut c) => match c.resume(&session, &host) {
                    Ok(login) => {
                        let info = c.hello().ok();
                        let reply = signed_in(&mut c, login, info, false, Instant::now());
                        client = Some(c);
                        last_beat = Instant::now();
                        reply
                    }
                    Err(e) if e.is_auth() => Reply::SignInFailed(e.to_string()),
                    Err(e) => match cached {
                        // the server is unreachable: keep working offline within the grace period
                        Some((login, verified_unix)) => {
                            let age = Duration::from_secs(u64::try_from(chrono::Utc::now().timestamp().saturating_sub(verified_unix)).unwrap_or(u64::MAX));
                            if age < Duration::from_secs(login.lease_seconds) {
                                let last_ok = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
                                c.set_session(Some(session));
                                let reply = signed_in(&mut c, login, None, true, last_ok);
                                client = Some(c);
                                last_beat = Instant::now();
                                reply
                            } else {
                                Reply::SignInFailed(format!("{e}. You have been offline too long; connect to the internet and sign in."))
                            }
                        }
                        None => Reply::SignInFailed(e.to_string()),
                    },
                },
            },
            Cmd::Logout => {
                if let Some(c) = client.as_mut() {
                    let _ = c.logout();
                }
                client = None;
                set(&|s| *s = LinkStatus::default());
                Reply::Done(String::new())
            }
            other => {
                let Some(c) = client.as_mut() else {
                    send(Reply::Error("not signed in".into()));
                    continue;
                };
                if let Cmd::Editing(name) = &other {
                    editing = name.clone();
                }
                match run(c, other, editing.as_deref()) {
                    Ok(reply) => {
                        mark_ok();
                        reply
                    }
                    Err(e) => {
                        if signed_out(&e, &mut client) {
                            continue;
                        }
                        let msg = e.to_string();
                        if matches!(e, OfficeError::Network(_)) {
                            set(&|s| s.last_error = Some(msg.clone()));
                        }
                        Reply::Error(msg)
                    }
                }
            }
        };
        if !send(reply) {
            return;
        }
    }
}

/// One server request for `cmd`.
fn run(c: &mut OfficeClient, cmd: Cmd, editing: Option<&str>) -> Result<Reply, OfficeError> {
    match cmd {
        Cmd::ListProjects => c.list_projects().map(Reply::Projects),
        Cmd::Download { name } => c.download(&name).map(|data| Reply::Downloaded { name, data }),
        Cmd::Upload { name, data } => c.upload(&name, &data).map(Reply::Uploaded),
        Cmd::Lock { name, force } => match c.lock(&name, force) {
            Ok(()) => Ok(Reply::Locked { name }),
            Err(OfficeError::Server(error)) if !error.starts_with(mattcut_office::AUTH_ERROR_PREFIX) => Ok(Reply::LockFailed { name, error }),
            Err(e) => Err(e),
        },
        Cmd::Unlock { name } => {
            c.unlock(&name)?;
            c.list_projects().map(Reply::Projects)
        }
        Cmd::Online => c.online().map(Reply::Online),
        Cmd::Editing(_) => {
            c.heartbeat(editing)?;
            c.list_projects().map(Reply::Projects)
        }
        Cmd::ChangePassword { old, new } => c.change_password(&old, &new).map(|()| Reply::Done("Password changed".into())),
        Cmd::Members => c.members().map(Reply::Members),
        Cmd::AddMember { email, name, role, password } => {
            let pw = c.add_member(&email, &name, role, password)?;
            Ok(Reply::NewPassword { email, password: pw })
        }
        Cmd::RemoveMember { email } => {
            c.remove_member(&email)?;
            c.members().map(Reply::Members)
        }
        Cmd::ResetPassword { email } => c.reset_password(&email).map(|p| Reply::NewPassword { email, password: Some(p) }),
        Cmd::Hello { .. } | Cmd::LoginPassword { .. } | Cmd::LoginGoogle { .. } | Cmd::Resume { .. } | Cmd::Logout => Ok(Reply::Done(String::new())),
    }
}
