//! The MattCut window: sign-in, the project home, and the editor.

pub mod theme;

mod icons;

mod editor;
mod export;
mod home;
mod inspector;
mod library;
mod timeline;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use filmcraft_engine::{Event, Session};
use mattcut_office::{ControlInfo, OnlineUser, ProjectEntry, valid_project_name};
use serde_json::{Value, json};

use crate::audio::Player;
use crate::config::{AppConfig, host_name};
use crate::control::{self, ControlRequest};
use crate::model::{MediaItem, SeqView, media_items};
use crate::office_link::{Cmd, OfficeLink, Reply};
use crate::preview::{self, Preview};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Connect,
    Home,
    Editor,
}

/// The project open in the editor.
#[derive(Clone, Debug)]
pub struct Doc {
    pub name: String,
    /// Stored on the office server (else a local draft).
    pub office: bool,
    /// The working file on this computer.
    pub path: PathBuf,
    /// Someone else holds the office edit lock: saving needs a new name.
    pub read_only: bool,
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// Dragged from the library onto the timeline.
#[derive(Clone, Debug)]
pub struct MediaDrag {
    pub item: u64,
    pub audio: bool,
    pub duration: i64,
    pub name: String,
}

pub struct LocalProject {
    pub name: String,
    pub path: PathBuf,
    pub modified: SystemTime,
}

pub struct MattCut {
    pub session: Session,
    pub cfg: AppConfig,
    pub office: OfficeLink,
    pub screen: Screen,
    pub host: String,
    pub connect: home::ConnectForm,
    pub office_projects: Vec<ProjectEntry>,
    pub online: Vec<OnlineUser>,
    pub local_projects: Vec<LocalProject>,
    pub doc: Option<Doc>,
    pub new_project: Option<home::NewProject>,
    pub save_copy: Option<String>,
    pub team: Option<home::TeamDialog>,
    pub account: Option<home::AccountDialog>,
    pub view: Option<SeqView>,
    view_key: Option<(u64, u64)>,
    pub media: Vec<MediaItem>,
    pub lib: library::LibraryState,
    pub insp: inspector::InspectorState,
    pub tl: timeline::TimelineState,
    pub preview: Preview,
    pub tex: Option<egui::TextureHandle>,
    pub thumbs: crate::thumbs::Thumbs,
    pub logo: Option<egui::TextureHandle>,
    pub player: Player,
    pub export: Option<export::ExportDialog>,
    pub toasts: Vec<Toast>,
    pub show_shortcuts: bool,
    control: Option<(Receiver<ControlRequest>, ControlInfo)>,
    last_local_save: Instant,
    last_upload: Instant,
    uploaded_revision: u64,
    last_online_poll: Instant,
}

impl MattCut {
    pub fn new(cc: &eframe::CreationContext<'_>, control_port: Option<u16>, server: Option<String>) -> Self {
        theme::install(&cc.egui_ctx);
        let mut cfg = AppConfig::load();
        if let Some(s) = server {
            cfg.server = s;
        }
        let mut session = Session::default();
        if let Some(dir) = mattcut_office::app_data_dir() {
            session.export_presets.set_dir(&dir);
            if let Err(e) = session.start_autosave(filmcraft_engine::autosave::AutosaveConfig::new(dir.join("Recovery"))) {
                log::warn!("crash recovery off: {e}");
            }
        }
        let office = OfficeLink::start(cc.egui_ctx.clone());
        let control = control::start(control_port, cc.egui_ctx.clone());
        let connect = home::ConnectForm::from_config(&cfg);
        let mut app = Self {
            session,
            office,
            screen: Screen::Connect,
            host: host_name(),
            connect,
            office_projects: Vec::new(),
            online: Vec::new(),
            local_projects: Vec::new(),
            doc: None,
            new_project: None,
            save_copy: None,
            team: None,
            account: None,
            view: None,
            view_key: None,
            media: Vec::new(),
            lib: library::LibraryState::default(),
            insp: inspector::InspectorState::default(),
            tl: timeline::TimelineState::default(),
            preview: Preview::start(cc.egui_ctx.clone()),
            tex: None,
            thumbs: crate::thumbs::Thumbs::start(cc.egui_ctx.clone()),
            logo: theme::logo_rgba().map(|(rgba, w, h)| {
                cc.egui_ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba), egui::TextureOptions::LINEAR)
            }),
            player: Player::default(),
            export: None,
            toasts: Vec::new(),
            show_shortcuts: false,
            control,
            last_local_save: Instant::now(),
            last_upload: Instant::now(),
            uploaded_revision: 0,
            last_online_poll: Instant::now(),
            cfg,
        };
        if crate::config::STANDALONE {
            app.screen = Screen::Home;
            app.scan_local_projects();
            return app;
        }
        // A saved sign-in: resume it (or work offline within the grace period).
        match (app.cfg.remember, app.cfg.session.clone()) {
            (true, Some(session)) if !app.cfg.server.is_empty() => {
                app.connect.auto = true;
                app.connect.busy = true;
                let cached = app.cfg.cached_login.clone().map(|l| (l, app.cfg.last_verified));
                app.office.send(Cmd::Resume { server: app.cfg.server.clone(), session, host: app.host.clone(), cached });
            }
            _ => app.connect.check_server(&app.office),
        }
        app
    }

    // ---- engine helpers -------------------------------------------------------------------

    /// Run an engine command; errors become a toast.
    pub fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        if !self.office.licensed() {
            self.error("MattCut is paused: it can't reach your team's server");
            return None;
        }
        match self.session.execute(id, params) {
            Ok(v) => Some(v),
            Err(e) => {
                self.error(format!("{e}"));
                None
            }
        }
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), error: false, at: Instant::now() });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::warn!("{text}");
        self.toasts.push(Toast { text, error: true, at: Instant::now() });
    }

    pub fn playhead(&self) -> i64 {
        self.session.playhead().0
    }

    pub fn set_playhead(&mut self, t: i64) {
        let end = self.view.as_ref().map_or(0, |v| v.duration);
        self.session.set_playhead(filmcraft_time::Tick(t.clamp(0, end.max(0))));
    }

    pub fn selection(&self) -> Vec<u64> {
        self.view.as_ref().map(|v| v.selection.clone()).unwrap_or_default()
    }

    /// Selected clips, or the clip on the main track under the playhead.
    pub fn target_clips(&self) -> Vec<u64> {
        let sel = self.selection();
        if !sel.is_empty() {
            return sel;
        }
        let t = self.playhead();
        self.view
            .as_ref()
            .and_then(|v| v.main_track())
            .and_then(|tr| tr.clips.iter().find(|c| c.start <= t && t < c.end))
            .map(|c| vec![c.id])
            .unwrap_or_default()
    }

    fn refresh_views(&mut self) {
        let key = (self.session.revision, self.session.document_epoch);
        if self.view_key == Some(key) {
            // selection is editor state (no revision bump): keep it live
            let sel: Vec<u64> = self.session.state.selection.iter().map(|c| c.0).collect();
            if let Some(v) = self.view.as_mut()
                && v.selection != sel
            {
                v.selection = sel;
            }
            return;
        }
        self.view_key = Some(key);
        self.view = SeqView::load(&mut self.session);
        self.media = media_items(&mut self.session);
    }

    pub fn toggle_play(&mut self) {
        if self.player.playing {
            self.player.stop();
            return;
        }
        let Some(seq) = self.session.state.active_sequence else {
            return;
        };
        let end = self.view.as_ref().map_or(0, |v| v.duration);
        if end <= 0 {
            return;
        }
        if self.playhead() >= end - self.view.as_ref().map_or(1, SeqView::frame_ticks) {
            self.set_playhead(0);
        }
        self.player.play(&self.session, seq, self.session.playhead());
        if let Some(n) = self.player.note.clone() {
            self.toast(n);
        }
    }

    pub fn stop_playback(&mut self) {
        if self.player.playing {
            self.player.stop();
        }
    }

    // ---- office -----------------------------------------------------------------------------

    pub fn sign_in_password(&mut self) {
        let f = &self.connect;
        self.office.send(Cmd::LoginPassword {
            server: f.server.trim().into(),
            email: f.email.trim().into(),
            password: f.password.clone(),
            host: self.host.clone(),
        });
        self.connect.busy = true;
        self.connect.error = None;
    }

    pub fn sign_in_google(&mut self) {
        let f = &self.connect;
        let hint = Some(f.email.trim().to_string()).filter(|e| !e.is_empty());
        self.office.send(Cmd::LoginGoogle { server: f.server.trim().into(), host: self.host.clone(), hint });
        self.connect.busy = true;
        self.connect.error = None;
    }

    /// Try the saved session again (after the server was unreachable).
    pub fn reconnect(&mut self) {
        match self.cfg.session.clone() {
            Some(session) => {
                let cached = self.cfg.cached_login.clone().map(|l| (l, self.cfg.last_verified));
                self.office.send(Cmd::Resume { server: self.cfg.server.clone(), session, host: self.host.clone(), cached });
            }
            None => self.screen = Screen::Connect,
        }
    }

    pub fn sign_out(&mut self) {
        if self.doc.is_some() {
            self.close_project();
        }
        self.office.send(Cmd::Logout);
        self.cfg.session = None;
        self.cfg.cached_login = None;
        self.cfg.save();
        self.connect.password.clear();
        self.connect.auto = false;
        self.connect.busy = false;
        self.screen = Screen::Connect;
    }

    fn handle_office(&mut self) {
        for reply in self.office.poll() {
            match reply {
                Reply::Hello(r) => {
                    self.connect.checking = false;
                    match r {
                        Ok(info) => {
                            self.connect.info = Some(info);
                            self.connect.server_error = None;
                        }
                        Err(e) => {
                            self.connect.info = None;
                            self.connect.server_error = Some(e);
                        }
                    }
                }
                Reply::GoogleWaiting => self.connect.google_waiting = true,
                Reply::SignedIn { login, info, server, offline } => {
                    self.connect.busy = false;
                    self.connect.google_waiting = false;
                    self.connect.password.clear();
                    if let Some(i) = info {
                        self.connect.info = Some(i);
                    }
                    self.cfg.server = server;
                    self.cfg.email = login.email.clone();
                    self.cfg.remember = self.connect.remember;
                    self.cfg.session = Some(login.session.clone());
                    self.cfg.cached_login = Some(login.clone());
                    if !offline {
                        self.cfg.last_verified = chrono::Utc::now().timestamp();
                    }
                    self.cfg.save();
                    if self.screen == Screen::Connect {
                        self.screen = Screen::Home;
                    }
                    self.scan_local_projects();
                    if offline {
                        let h = self.office.lease_left().map_or(0, |d| d.as_secs() / 3600);
                        self.toast(format!(
                            "Working offline: can't reach the MattCut server. Local projects work for about {h} more hours; team projects need the internet."
                        ));
                    } else {
                        self.office.send(Cmd::ListProjects);
                        self.office.send(Cmd::Online);
                        self.toast(format!("Welcome, {}", login.name));
                    }
                }
                Reply::SignInFailed(e) => {
                    self.connect.busy = false;
                    self.connect.google_waiting = false;
                    self.connect.auto = false;
                    self.connect.error = Some(e);
                    if self.screen != Screen::Connect && !self.office.licensed() {
                        self.screen = Screen::Connect;
                    }
                }
                Reply::SignedOut(reason) => {
                    if self.doc.is_some() {
                        self.close_project();
                    }
                    self.cfg.session = None;
                    self.cfg.cached_login = None;
                    self.cfg.save();
                    self.connect.error = Some(reason);
                    self.connect.auto = false;
                    self.connect.busy = false;
                    self.screen = Screen::Connect;
                }
                Reply::Projects(list) => {
                    if !list.is_empty() || self.office_projects.len() <= 1 {
                        self.office_projects = list;
                    }
                }
                Reply::Downloaded { name, data } => self.open_downloaded(&name, &data),
                Reply::Uploaded(entry) => {
                    self.toast(format!("Saved \"{}\" to the team", entry.name));
                    self.office.send(Cmd::ListProjects);
                }
                Reply::Locked { name } => {
                    if let Some(d) = self.doc.as_mut().filter(|d| d.name == name) {
                        d.read_only = false;
                    }
                    self.office.send(Cmd::Editing(Some(name)));
                }
                Reply::LockFailed { name, error } => {
                    if let Some(d) = self.doc.as_mut().filter(|d| d.name == name) {
                        d.read_only = true;
                    }
                    self.toast(format!("\"{name}\" is {error}: opened as a read-only copy. Use File ▸ Save a Copy to keep your changes."));
                }
                Reply::Online(users) => self.online = users,
                Reply::Members(list) => {
                    if let Some(t) = self.team.as_mut() {
                        t.members = list;
                        t.loading = false;
                    }
                }
                Reply::NewPassword { email, password } => {
                    if let Some(t) = self.team.as_mut() {
                        t.notice = Some(match password {
                            Some(p) => {
                                format!("{email} can sign in with Google, or with this password: {p}\nShare it privately; they can change it in MattCut.")
                            }
                            None => format!("{email} was added. They sign in with \"Continue with Google\" using that address."),
                        });
                    }
                    self.office.send(Cmd::Members);
                }
                Reply::Done(msg) => {
                    if !msg.is_empty() {
                        self.toast(msg);
                    }
                }
                Reply::Error(e) if !e.is_empty() => {
                    if let Some(t) = self.team.as_mut() {
                        t.loading = false;
                    }
                    self.error(e);
                }
                Reply::Error(_) => {}
            }
        }
        if self.screen != Screen::Connect && self.last_online_poll.elapsed() > Duration::from_secs(30) {
            self.last_online_poll = Instant::now();
            self.office.send(Cmd::Online);
            if self.screen == Screen::Home {
                self.office.send(Cmd::ListProjects);
            }
            // remember when the server last vouched for us (offline grace after a restart)
            if self.office.online_now() && self.cfg.session.is_some() {
                self.cfg.last_verified = chrono::Utc::now().timestamp();
                self.cfg.save();
            }
        }
    }

    // ---- projects ---------------------------------------------------------------------------

    fn office_cache_dir() -> PathBuf {
        mattcut_office::app_data_dir().unwrap_or_else(std::env::temp_dir).join("Office Projects")
    }

    pub fn scan_local_projects(&mut self) {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.cfg.projects_dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("fcproj")
                    && let Some(name) = p.file_stem().and_then(|s| s.to_str())
                {
                    let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                    out.push(LocalProject { name: name.to_string(), path: p.clone(), modified });
                }
            }
        }
        out.sort_by(|a, b| b.modified.cmp(&a.modified));
        self.local_projects = out;
    }

    fn after_open(&mut self, doc: Doc) {
        self.stop_playback();
        self.doc = Some(doc);
        self.view_key = None;
        self.preview.invalidate();
        self.tex = None;
        self.tl.fit_next = true;
        self.screen = Screen::Editor;
        self.uploaded_revision = self.session.revision;
        self.refresh_views();
    }

    pub fn open_local(&mut self, path: &Path) {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Project").to_string();
        if self.run("file.open", json!({"path": path.to_string_lossy()})).is_some() {
            self.after_open(Doc { name, office: false, path: path.to_path_buf(), read_only: false });
        }
    }

    pub fn open_office(&mut self, name: &str) {
        self.office.send(Cmd::Download { name: name.to_string() });
        self.toast(format!("Opening \"{name}\"…"));
    }

    fn open_downloaded(&mut self, name: &str, data: &[u8]) {
        let dir = Self::office_cache_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{name}.fcproj"));
        if let Err(e) = std::fs::write(&path, data) {
            self.error(format!("{}: {e}", path.display()));
            return;
        }
        if self.run("file.open", json!({"path": path.to_string_lossy()})).is_some() {
            self.after_open(Doc { name: name.to_string(), office: true, path, read_only: false });
            self.office.send(Cmd::Lock { name: name.to_string(), force: false });
        }
    }

    /// A new project with one sequence of the given shape, saved straight away.
    pub fn create_project(&mut self, np: &home::NewProject) {
        let name = np.name.trim().to_string();
        if !valid_project_name(&name) {
            self.error("Project names can use letters, numbers, spaces, - _ . ( )");
            return;
        }
        let (w, h) = np.size();
        let path = if np.office { Self::office_cache_dir().join(format!("{name}.fcproj")) } else { self.cfg.projects_dir.join(format!("{name}.fcproj")) };
        if !np.office && path.exists() {
            self.error(format!("A project called \"{name}\" already exists"));
            return;
        }
        if np.office && self.office_projects.iter().any(|p| p.name == name) {
            self.error(format!("The team already has a project called \"{name}\""));
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if self.run("file.newProject", json!({"name": name})).is_none() {
            return;
        }
        if self.run("file.newSequence", json!({"name": name, "width": w, "height": h, "fps": np.fps, "video": 4, "audio": 4})).is_none() {
            return;
        }
        if self.run("file.save", json!({"path": path.to_string_lossy()})).is_none() {
            return;
        }
        self.after_open(Doc { name: name.clone(), office: np.office, path: path.clone(), read_only: false });
        if np.office {
            if let Ok(data) = std::fs::read(&path) {
                self.office.send(Cmd::Upload { name: name.clone(), data });
            }
            self.office.send(Cmd::Lock { name, force: false });
        }
    }

    /// Save (Cmd+S): the working file, and the office copy for office projects.
    pub fn save(&mut self) {
        let Some(doc) = self.doc.clone() else { return };
        if self.run("file.save", json!({"path": doc.path.to_string_lossy()})).is_none() {
            return;
        }
        self.last_local_save = Instant::now();
        if doc.office {
            if doc.read_only {
                self.save_copy = Some(format!("{} (copy)", doc.name));
                return;
            }
            self.upload_current(&doc);
        } else {
            self.toast(format!("Saved \"{}\"", doc.name));
        }
    }

    fn upload_current(&mut self, doc: &Doc) {
        match std::fs::read(&doc.path) {
            Ok(data) => {
                self.office.send(Cmd::Upload { name: doc.name.clone(), data });
                self.last_upload = Instant::now();
                self.uploaded_revision = self.session.revision;
            }
            Err(e) => self.error(format!("{}: {e}", doc.path.display())),
        }
    }

    /// Save the open project under a new name (office projects stay on the office server).
    pub fn save_as_copy(&mut self, new_name: &str) {
        let Some(doc) = self.doc.clone() else { return };
        let name = new_name.trim().to_string();
        if !valid_project_name(&name) {
            self.error("Project names can use letters, numbers, spaces, - _ . ( )");
            return;
        }
        let path = if doc.office { Self::office_cache_dir().join(format!("{name}.fcproj")) } else { self.cfg.projects_dir.join(format!("{name}.fcproj")) };
        if self.run("file.save", json!({"path": path.to_string_lossy()})).is_none() {
            return;
        }
        if doc.office && !doc.read_only {
            self.office.send(Cmd::Unlock { name: doc.name.clone() });
        }
        let new_doc = Doc { name: name.clone(), office: doc.office, path, read_only: false };
        self.doc = Some(new_doc.clone());
        if doc.office {
            self.upload_current(&new_doc);
            self.office.send(Cmd::Lock { name, force: false });
        } else {
            self.toast(format!("Saved \"{name}\""));
        }
    }

    pub fn close_project(&mut self) {
        self.stop_playback();
        if let Some(doc) = self.doc.clone() {
            if self.session.is_dirty() && !doc.read_only {
                self.save();
            }
            if doc.office && !doc.read_only {
                self.office.send(Cmd::Unlock { name: doc.name.clone() });
            }
        }
        self.office.send(Cmd::Editing(None));
        self.doc = None;
        self.screen = Screen::Home;
        self.scan_local_projects();
        self.office.send(Cmd::ListProjects);
    }

    /// Keep work safe: the working file every minute, the office copy every three.
    fn background_saves(&mut self) {
        let Some(doc) = self.doc.clone() else { return };
        if !self.office.licensed() {
            return;
        }
        if self.session.is_dirty() && self.last_local_save.elapsed() > Duration::from_secs(60) {
            self.last_local_save = Instant::now();
            let _ = self.session.execute("file.save", json!({"path": doc.path.to_string_lossy()}));
        }
        if doc.office && !doc.read_only && self.session.revision != self.uploaded_revision && self.last_upload.elapsed() > Duration::from_secs(180) {
            let _ = self.session.execute("file.save", json!({"path": doc.path.to_string_lossy()}));
            self.upload_current(&doc);
        }
    }

    pub fn import_dialog(&mut self) {
        let files = rfd::FileDialog::new()
            .set_title("Import media")
            .add_filter(
                "Media",
                &[
                    "mp4", "mov", "m4v", "mkv", "webm", "avi", "mxf", "mts", "m2ts", "mp3", "wav", "aif", "aiff", "flac", "ogg", "opus", "m4a", "aac", "png",
                    "jpg", "jpeg", "gif", "webp", "tif", "tiff", "bmp", "heic", "srt", "vtt",
                ],
            )
            .add_filter("All files", &["*"])
            .pick_files();
        let Some(files) = files else { return };
        self.import_paths(files);
    }

    pub fn import_paths(&mut self, files: Vec<PathBuf>) {
        let paths: Vec<String> = files.iter().map(|p| p.to_string_lossy().to_string()).collect();
        if paths.is_empty() {
            return;
        }
        let (captions, media): (Vec<String>, Vec<String>) =
            paths.into_iter().partition(|p| p.to_lowercase().ends_with(".srt") || p.to_lowercase().ends_with(".vtt"));
        for c in captions {
            self.run("captions.import", json!({"path": c}));
        }
        if !media.is_empty() && self.run("file.import", json!({"paths": media})).is_some() {
            self.toast(format!("Imported {} file{}", media.len(), if media.len() == 1 { "" } else { "s" }));
        }
    }

    // ---- MCP / control channel --------------------------------------------------------------

    fn handle_control(&mut self, ctx: &egui::Context) {
        let Some((rx, _)) = &self.control else { return };
        let requests: Vec<ControlRequest> = rx.try_iter().collect();
        for req in requests {
            let p = &req.params;
            let reply = match req.method.as_str() {
                "ping" => control::ok(json!({"app": "MattCut", "version": env!("CARGO_PKG_VERSION")})),
                "engine.execute" => {
                    if self.office.licensed() {
                        let id = p.get("command").and_then(Value::as_str).unwrap_or_default().to_string();
                        let params = p.get("params").cloned().unwrap_or_else(|| json!({}));
                        let r = self.session.execute(&id, params);
                        if self.screen != Screen::Editor && self.session.state.active_sequence.is_some() && self.doc.is_none() {
                            // an agent opened or made a project: show it
                            if let Some(path) = self.session.path.clone() {
                                let pb = PathBuf::from(&path);
                                let name = pb.file_stem().and_then(|s| s.to_str()).unwrap_or("Project").to_string();
                                self.after_open(Doc { name, office: false, path: pb, read_only: false });
                            }
                        }
                        match r {
                            Ok(v) => control::ok(v),
                            Err(e) => control::err(e),
                        }
                    } else {
                        control::err("MattCut is paused: it can't reach the team server, or nobody is signed in")
                    }
                }
                "ui.inspect" => control::ok(self.ui_state()),
                "ui.screenshot" => {
                    let path = p.get("path").and_then(Value::as_str).map(PathBuf::from);
                    let panel = p.get("panel").and_then(Value::as_str).unwrap_or_default().to_lowercase();
                    match path {
                        None => control::err("ui.screenshot needs a path"),
                        Some(path) if matches!(panel.as_str(), "program" | "player" | "preview") => {
                            match preview::snapshot_png(&self.session, 1920).and_then(|png| std::fs::write(&path, png).map_err(|e| e.to_string())) {
                                Ok(()) => control::ok(json!({"path": path})),
                                Err(e) => control::err(e),
                            }
                        }
                        Some(path) => {
                            // the whole window, once the next frame is painted
                            let reply = req.reply.clone();
                            let cb = egui::ScreenshotCallback::new(move |img: Arc<egui::ColorImage>| {
                                let [w, h] = img.size;
                                let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                                let r = preview::encode_png(w as u32, h as u32, rgba).and_then(|png| std::fs::write(&path, png).map_err(|e| e.to_string()));
                                let _ = reply.send(match r {
                                    Ok(()) => control::ok(json!({"path": path})),
                                    Err(e) => control::err(e),
                                });
                            });
                            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
                            ctx.request_repaint();
                            continue;
                        }
                    }
                }
                "ui.playback" => {
                    match p.get("action").and_then(Value::as_str) {
                        Some("play") if !self.player.playing => self.toggle_play(),
                        Some("pause" | "stop") => self.stop_playback(),
                        Some("toggle") => self.toggle_play(),
                        _ => {}
                    }
                    control::ok(json!({"playing": self.player.playing}))
                }
                other => control::err(format!("MattCut does not support `{other}` (use engine.execute, ui.inspect, ui.screenshot, ui.playback)")),
            };
            let _ = req.reply.send(reply);
        }
    }

    fn ui_state(&self) -> Value {
        let st = self.office.status();
        json!({
            "app": "MattCut",
            "screen": format!("{:?}", self.screen),
            "project": self.doc.as_ref().map(|d| json!({"name": d.name, "team": d.office, "path": d.path, "readOnly": d.read_only})),
            "playing": self.player.playing,
            "playheadSeconds": crate::model::secs(self.playhead()),
            "selection": self.selection(),
            "account": {
                "signedIn": st.login.is_some(),
                "email": st.login.as_ref().map(|l| l.email.clone()),
                "name": st.login.as_ref().map(|l| l.name.clone()),
                "team": st.login.as_ref().map(|l| l.team.clone()),
                "server": st.server,
                "online": self.office.online_now(),
                "usable": self.office.licensed(),
                "offlineGraceSecondsLeft": self.office.lease_left().map(|d| d.as_secs()),
            },
            "teamProjects": self.office_projects.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        })
    }

    // ---- per frame --------------------------------------------------------------------------

    fn housekeeping(&mut self, ctx: &egui::Context) {
        self.handle_office();
        self.handle_control(ctx);
        self.thumbs.poll(ctx, &self.session);
        self.session.poll_persistence();
        for ev in self.session.drain_events() {
            match ev {
                Event::Toast { message, error } => {
                    if error {
                        self.error(message);
                    } else {
                        self.toast(message);
                    }
                }
                Event::OpenSequence(_) | Event::ProjectChanged { .. } => self.view_key = None,
                _ => {}
            }
        }
        if self.screen == Screen::Editor {
            self.refresh_views();
            if self.player.playing {
                let end = self.view.as_ref().map_or(0, |v| v.duration);
                let t = self.player.position().0;
                if t >= end {
                    self.player.stop();
                    self.set_playhead(end);
                } else {
                    self.session.set_playhead(filmcraft_time::Tick(t));
                }
                ctx.request_repaint();
            }
            if !self.session.jobs.is_empty() {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            self.background_saves();
        }
        // licence lease: show the lock as soon as it runs out
        if self.screen != Screen::Connect {
            ctx.request_repaint_after(Duration::from_secs(5));
            if !self.office.licensed() && self.player.playing {
                self.player.stop();
            }
        }
        self.toasts.retain(|t| t.at.elapsed() < Duration::from_secs(if t.error { 7 } else { 4 }));
        let title = match &self.doc {
            Some(d) => format!("{}{} — MattCut", d.name, if self.session.is_dirty() { " •" } else { "" }),
            None => "MattCut".into(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    fn lock_overlay(&mut self, ctx: &egui::Context) {
        if self.screen == Screen::Connect || self.office.licensed() {
            return;
        }
        egui::Modal::new(egui::Id::unique("server-lock")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading("Can't reach the MattCut server");
            ui.add_space(6.0);
            let grace = self.office.status().lease.as_secs() / 3600;
            ui.label(format!("MattCut checks in with your team's server at least every {grace} hours. Check your internet connection, then try again."));
            if let Some(e) = self.office.status().last_error.filter(|e| e != "offline") {
                ui.label(egui::RichText::new(e).color(theme::TEXT_DIM).small());
            }
            ui.label(egui::RichText::new("Your project was saved on this computer and nothing is lost.").color(theme::TEXT_DIM));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(theme::primary("Try again")).clicked() {
                    self.reconnect();
                }
                if ui.button("Sign out").clicked() {
                    self.sign_out();
                }
            });
        });
        // make sure work is on disk the moment the grace period runs out
        if let Some(doc) = self.doc.clone()
            && self.session.is_dirty()
        {
            let _ = self.session.execute("file.save", json!({"path": doc.path.to_string_lossy()}));
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::unique("toasts")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 56.0)).order(egui::Order::Tooltip).show(ctx, |ui| {
            for t in self.toasts.iter().rev().take(4) {
                egui::Frame::new()
                    .fill(if t.error { egui::Color32::from_rgb(0x4a, 0x1d, 0x22) } else { theme::PANEL_3 })
                    .stroke(egui::Stroke::new(1.0, if t.error { theme::ERR } else { theme::BORDER }))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);
                        ui.label(egui::RichText::new(&t.text).color(theme::TEXT));
                    });
                ui.add_space(4.0);
            }
        });
    }

    fn dropped_files(&mut self, ctx: &egui::Context) {
        let files: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if files.is_empty() {
            return;
        }
        match self.screen {
            Screen::Editor => self.import_paths(files),
            Screen::Home => {
                if let Some(p) = files.iter().find(|p| p.extension().and_then(|e| e.to_str()) == Some("fcproj")) {
                    let p = p.clone();
                    self.open_local(&p);
                }
            }
            Screen::Connect => {}
        }
    }
}

impl eframe::App for MattCut {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.housekeeping(&ctx);
        self.dropped_files(&ctx);
        match self.screen {
            Screen::Connect => home::connect_screen(self, ui),
            Screen::Home => home::home_screen(self, ui),
            Screen::Editor => editor::show(self, ui),
        }
        home::dialogs(self, &ctx);
        if let Some(mut dlg) = self.export.take() {
            if export::show(self, &ctx, &mut dlg) {
                self.export = Some(dlg);
            }
        }
        if self.show_shortcuts {
            editor::shortcuts_window(self, &ctx);
        }
        self.lock_overlay(&ctx);
        self.toasts_ui(&ctx);
    }

    fn on_exit(&mut self) {
        self.player.stop();
        if let Some(doc) = self.doc.clone() {
            if self.session.is_dirty() && self.office.licensed() {
                let _ = self.session.execute("file.save", json!({"path": doc.path.to_string_lossy()}));
                if doc.office
                    && !doc.read_only
                    && let Ok(data) = std::fs::read(&doc.path)
                {
                    // best effort: the link thread may not finish before exit
                    if let Ok(mut c) = mattcut_office::OfficeClient::new(&self.cfg.server) {
                        let st = self.office.status();
                        c.set_session(st.login.map(|l| l.session));
                        let _ = c.upload(&doc.name, &data);
                        let _ = c.unlock(&doc.name);
                    }
                }
            }
        }
        if let Some((_, info)) = &self.control {
            control::unpublish(info);
        }
        self.session.shutdown();
    }
}
