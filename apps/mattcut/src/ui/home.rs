//! Sign-in (Google or email), the project home (team projects + local drafts), and the account /
//! team dialogs.

use egui::{Align, Color32, CornerRadius, Layout, RichText, Stroke, vec2};
use mattcut_office::{Member, Role, ServerInfo};

use super::{MattCut, Screen, icons, theme};
use crate::config::AppConfig;
use crate::office_link::Cmd;

#[derive(Default)]
pub struct ConnectForm {
    pub server: String,
    pub email: String,
    pub password: String,
    pub remember: bool,
    pub busy: bool,
    /// Signing in with saved details at start-up.
    pub auto: bool,
    pub google_waiting: bool,
    pub checking: bool,
    /// The server address the last `hello` was for.
    pub checked: String,
    pub info: Option<ServerInfo>,
    pub server_error: Option<String>,
    pub error: Option<String>,
    /// Show the server address field (hidden once it is known).
    pub edit_server: bool,
}

impl ConnectForm {
    pub fn from_config(cfg: &AppConfig) -> Self {
        Self { server: cfg.server.clone(), email: cfg.email.clone(), remember: cfg.remember, edit_server: cfg.server.is_empty(), ..Default::default() }
    }

    /// Ask the server for its name and sign-in options (when the address changed).
    pub fn check_server(&mut self, link: &crate::office_link::OfficeLink) {
        let s = self.server.trim().to_string();
        if s.is_empty() || s == self.checked {
            return;
        }
        self.checked = s.clone();
        self.checking = true;
        self.info = None;
        self.server_error = None;
        link.send(Cmd::Hello { server: s });
    }
}

#[derive(Default)]
pub struct TeamDialog {
    pub members: Vec<Member>,
    pub loading: bool,
    pub email: String,
    pub name: String,
    pub admin: bool,
    pub with_password: bool,
    pub notice: Option<String>,
    pub confirm_remove: Option<String>,
}

#[derive(Default)]
pub struct AccountDialog {
    pub old: String,
    pub new: String,
    pub again: String,
}

/// Aspect presets, as social platforms use them.
pub const RATIOS: [(&str, &str, u32, u32); 7] = [
    ("16:9", "YouTube, TV", 16, 9),
    ("9:16", "TikTok, Reels, Shorts", 9, 16),
    ("1:1", "Instagram post", 1, 1),
    ("4:5", "Instagram portrait", 4, 5),
    ("4:3", "Classic", 4, 3),
    ("3:4", "Portrait", 3, 4),
    ("21:9", "Cinema", 21, 9),
];

pub struct NewProject {
    pub name: String,
    pub ratio: usize,
    /// Short side in pixels.
    pub res: u32,
    pub fps: f64,
    pub office: bool,
}

impl NewProject {
    pub fn size(&self) -> (u32, u32) {
        ratio_size(self.ratio, self.res)
    }
}

/// Width and height for ratio preset `ratio` with short side `res` (even numbers).
pub fn ratio_size(ratio: usize, res: u32) -> (u32, u32) {
    let (_, _, rw, rh) = RATIOS.get(ratio).copied().unwrap_or(RATIOS[0]);
    let even = |x: f64| ((x / 2.0).round() as u32) * 2;
    if rw >= rh { (even(f64::from(res) * f64::from(rw) / f64::from(rh)), res) } else { (res, even(f64::from(res) * f64::from(rh) / f64::from(rw))) }
}

fn logo(app: &MattCut, ui: &mut egui::Ui, size: f32) {
    ui.horizontal(|ui| {
        super::editor::logo(app, ui, size);
        ui.add_space(size * 0.15);
        ui.label(RichText::new("MattCut").size(size * 0.6).color(theme::TEXT).strong());
    });
}

fn field(ui: &mut egui::Ui, label: &str) {
    ui.label(RichText::new(label).size(12.0).color(theme::TEXT_DIM));
}

fn text_input(ui: &mut egui::Ui, value: &mut String, hint: &str, password: bool) -> egui::Response {
    egui::Frame::new()
        .fill(theme::PANEL_2)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| ui.add(egui::TextEdit::singleline(value).hint_text(hint).password(password).frame(egui::Frame::NONE).desired_width(f32::INFINITY)))
        .inner
}

/// The "G" mark for the Google button (drawn; four-colour ring with the bar).
fn google_mark(p: &egui::Painter, c: egui::Pos2, r: f32) {
    use std::f32::consts::PI;
    let seg = |a0: f32, a1: f32, col: Color32| {
        let pts: Vec<egui::Pos2> = (0..=12)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / 12.0;
                c + vec2(r * a.cos(), r * a.sin())
            })
            .collect();
        p.add(egui::Shape::line(pts, Stroke::new(r * 0.42, col)));
    };
    seg(-0.25 * PI, 0.25 * PI, Color32::from_rgb(0x42, 0x85, 0xf4));
    seg(0.25 * PI, 0.75 * PI, Color32::from_rgb(0x34, 0xa8, 0x53));
    seg(0.75 * PI, 1.15 * PI, Color32::from_rgb(0xfb, 0xbc, 0x05));
    seg(1.15 * PI, 1.75 * PI, Color32::from_rgb(0xea, 0x43, 0x35));
    p.line_segment([c, c + vec2(r * 1.05, 0.0)], Stroke::new(r * 0.42, Color32::from_rgb(0x42, 0x85, 0xf4)));
}

pub fn connect_screen(app: &mut MattCut, ui: &mut egui::Ui) {
    egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG)).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.10).max(20.0));
            ui.allocate_ui(vec2(220.0, 48.0), |ui| logo(app, ui, 44.0));
            ui.add_space(4.0);
            let team = app.connect.info.as_ref().map(|i| i.team.clone());
            ui.label(
                RichText::new(match &team {
                    Some(t) => format!("Sign in to {t}"),
                    None => "Sign in to your team".into(),
                })
                .size(15.0)
                .color(theme::TEXT_DIM),
            );
            ui.add_space(20.0);
            theme::card().show(ui, |ui| {
                ui.set_width(380.0);
                if app.connect.auto && app.connect.busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Signing in as {}…", app.cfg.email));
                    });
                    return;
                }
                if app.connect.google_waiting {
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.add_space(6.0);
                        ui.label("Finish signing in with Google in your browser…");
                        ui.add_space(6.0);
                        if ui.add(theme::ghost("Cancel")).clicked() {
                            app.connect.google_waiting = false;
                            app.connect.busy = false;
                        }
                    });
                    return;
                }
                // server address (once; then a small "change" link)
                if app.connect.edit_server || app.connect.server.trim().is_empty() {
                    field(ui, "Team server");
                    let r = text_input(ui, &mut app.connect.server, "mattcut.yourcompany.com", false);
                    if r.lost_focus() {
                        app.connect.check_server(&app.office);
                    }
                    if let Some(e) = &app.connect.server_error {
                        ui.label(RichText::new(e).size(11.5).color(theme::ERR));
                    }
                    ui.add_space(10.0);
                } else {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(app.connect.server.trim()).size(11.5).color(theme::TEXT_FAINT));
                        if app.connect.checking {
                            ui.spinner();
                        }
                        if ui.link(RichText::new("change").size(11.5)).clicked() {
                            app.connect.edit_server = true;
                        }
                    });
                    if let Some(e) = &app.connect.server_error {
                        ui.label(RichText::new(e).size(11.5).color(theme::ERR));
                    }
                    ui.add_space(8.0);
                }
                app.connect.check_server(&app.office);
                let busy = app.connect.busy;
                let has_server = !app.connect.server.trim().is_empty();
                // Google
                let google = app.connect.info.as_ref().is_some_and(|i| i.google_client_id.is_some());
                let (gr, gresp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), egui::Sense::click());
                let enabled = google && !busy && has_server;
                let p = ui.painter();
                p.rect_filled(
                    gr,
                    CornerRadius::same(8),
                    if !enabled {
                        theme::PANEL_2
                    } else if gresp.hovered() {
                        Color32::from_rgb(0xf2, 0xf2, 0xf4)
                    } else {
                        Color32::WHITE
                    },
                );
                let label = "Continue with Google";
                let g = p.layout_no_wrap(
                    label.to_string(),
                    egui::FontId::proportional(14.0),
                    if enabled { Color32::from_rgb(0x1f, 0x1f, 0x1f) } else { theme::TEXT_FAINT },
                );
                let total = 18.0 + 10.0 + g.size().x;
                let x0 = gr.center().x - total / 2.0;
                google_mark(p, egui::pos2(x0 + 9.0, gr.center().y), 6.5);
                p.galley(egui::pos2(x0 + 28.0, gr.center().y - g.size().y / 2.0), g, Color32::BLACK);
                let gresp = if !google && app.connect.info.is_some() {
                    gresp.on_hover_text("Your admin hasn't turned on Google sign-in yet (docs/server.md)")
                } else {
                    gresp
                };
                if gresp.clicked() && enabled {
                    app.sign_in_google();
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 30.0) / 2.0;
                    let (l, _) = ui.allocate_exact_size(vec2(w, 12.0), egui::Sense::hover());
                    ui.painter().line_segment([l.left_center(), l.right_center()], Stroke::new(1.0, theme::BORDER));
                    ui.label(RichText::new("or").size(11.5).color(theme::TEXT_FAINT));
                    let (r, _) = ui.allocate_exact_size(vec2(w, 12.0), egui::Sense::hover());
                    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, theme::BORDER));
                });
                ui.add_space(8.0);
                field(ui, "Email");
                text_input(ui, &mut app.connect.email, "you@company.com", false);
                ui.add_space(6.0);
                field(ui, "Password");
                let pw = text_input(ui, &mut app.connect.password, "", true);
                ui.add_space(4.0);
                ui.checkbox(&mut app.connect.remember, RichText::new("Keep me signed in on this computer").size(12.5));
                if let Some(e) = &app.connect.error {
                    ui.add_space(4.0);
                    ui.label(RichText::new(e).size(12.0).color(theme::ERR));
                }
                ui.add_space(10.0);
                let ready = has_server && !app.connect.email.trim().is_empty() && !app.connect.password.is_empty() && !busy;
                let enter = pw.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    let btn = theme::primary("Sign in").min_size(vec2(ui.available_width(), 38.0));
                    if ui.add_enabled(ready, btn).clicked() || (enter && ready) {
                        app.sign_in_password();
                    }
                });
                if busy {
                    ui.vertical_centered(|ui| ui.spinner());
                }
            });
            ui.add_space(14.0);
            ui.label(RichText::new("Only people your admin has added to the team can sign in.").size(11.5).color(theme::TEXT_FAINT));
        });
    });
}

/// The connection dot's tooltip: online, or how much offline time is left.
pub fn status_tip(app: &MattCut) -> (Color32, String) {
    if app.office.online_now() {
        (theme::OK, "Connected to your team's MattCut server".into())
    } else {
        let left = app.office.lease_left().map_or(0, |d| d.as_secs());
        (theme::WARN, format!("Offline — can't reach the MattCut server. Editing keeps working for {}h {}m.", left / 3600, (left % 3600) / 60))
    }
}

/// The account menu (avatar): change password, team (admins), sign out.
pub fn account_menu(app: &mut MattCut, ui: &mut egui::Ui) {
    let st = app.office.status();
    let Some(l) = st.login else { return };
    let initial: String = l.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    ui.menu_button(RichText::new(initial).size(13.0).color(Color32::WHITE), |ui| {
        ui.set_min_width(240.0);
        ui.label(RichText::new(&l.name).size(13.5).color(theme::TEXT));
        ui.label(RichText::new(&l.email).size(11.5).color(theme::TEXT_DIM));
        ui.label(RichText::new(format!("{} · {}", l.team, if l.role == Role::Admin { "admin" } else { "editor" })).size(11.5).color(theme::TEXT_FAINT));
        ui.separator();
        if l.role == Role::Admin && ui.button("Team members…").clicked() {
            app.team = Some(TeamDialog { loading: true, ..Default::default() });
            app.office.send(Cmd::Members);
            ui.close();
        }
        if ui.button("Change password…").clicked() {
            app.account = Some(AccountDialog::default());
            ui.close();
        }
        if ui.button("Sign out").clicked() {
            ui.close();
            app.sign_out();
        }
    });
}

/// "Update to X" in the top bars once the update check found a newer release.
pub fn update_button(app: &mut MattCut, ui: &mut egui::Ui) {
    let Some(v) = app.update.as_ref().map(|u| u.version.clone()) else { return };
    let btn = egui::Button::new(RichText::new(format!("Update to {v}")).size(12.5).color(Color32::WHITE)).fill(theme::ACCENT).corner_radius(6);
    if ui.add(btn).on_hover_text("A new version of MattCut is available").clicked() {
        app.update_dialog = true;
    }
    ui.add_space(6.0);
}

fn top_bar(app: &mut MattCut, ui: &mut egui::Ui) {
    egui::Panel::top("home-top")
        .exact_size(60.0)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(28, 12)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                logo(app, ui, 28.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    update_button(app, ui);
                    if crate::config::STANDALONE {
                        return;
                    }
                    account_menu(app, ui);
                    let st = app.office.status();
                    if let Some(l) = &st.login {
                        ui.label(RichText::new(format!("{}  ·  {}", l.name, l.team)).color(theme::TEXT));
                    }
                    let (dot, tip) = status_tip(app);
                    theme::status_dot(ui, dot).on_hover_text(tip);
                    if !app.online.is_empty() {
                        let names: Vec<String> = app
                            .online
                            .iter()
                            .map(|u| match &u.editing {
                                Some(p) => format!("{} — editing {p}", u.user),
                                None => u.user.clone(),
                            })
                            .collect();
                        ui.label(RichText::new(format!("{} online", app.online.len())).color(theme::TEXT_DIM)).on_hover_text(names.join("\n"));
                    }
                });
            });
        });
}

pub fn home_screen(app: &mut MattCut, ui: &mut egui::Ui) {
    top_bar(app, ui);
    egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(36, 20))).show(ui, |ui| {
        // Create
        ui.horizontal(|ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(260.0, 110.0), egui::Sense::click());
            let hovered = resp.hovered();
            let p = ui.painter();
            p.rect_filled(r, CornerRadius::same(14), if hovered { theme::ACCENT_HOVER } else { theme::ACCENT });
            p.circle_filled(r.center() - vec2(0.0, 14.0), 17.0, Color32::from_white_alpha(40));
            super::icons::draw(p, egui::Rect::from_center_size(r.center() - vec2(0.0, 14.0), vec2(20.0, 20.0)), super::icons::Icon::Plus, Color32::WHITE);
            p.text(r.center() + vec2(0.0, 24.0), egui::Align2::CENTER_CENTER, "New project", egui::FontId::proportional(15.0), Color32::WHITE);
            if resp.clicked() {
                app.new_project = Some(NewProject { name: next_name(app), ratio: 0, res: 1080, fps: 30.0, office: !crate::config::STANDALONE });
            }
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.add_space(14.0);
                ui.label(RichText::new("Start creating").size(22.0).color(theme::TEXT));
                ui.label(
                    RichText::new(if crate::config::STANDALONE {
                        "Projects are saved on this computer. Share one by sending its .fcproj file (with the footage)."
                    } else {
                        "Team projects are shared with everyone on your team and backed up on the MattCut server."
                    })
                    .color(theme::TEXT_DIM),
                );
                if let Some(share) = app.office.status().login.and_then(|l| l.media_share) {
                    ui.label(RichText::new(format!("Shared footage: {share}")).color(theme::TEXT_DIM));
                }
                ui.horizontal(|ui| {
                    if ui.button("Open a project file…").clicked()
                        && let Some(p) = rfd::FileDialog::new().add_filter("MattCut project", &["fcproj"]).pick_file()
                    {
                        app.open_local(&p);
                    }
                    if ui.button("Refresh").clicked() {
                        app.office.send(Cmd::ListProjects);
                        app.scan_local_projects();
                    }
                });
            });
        });
        ui.add_space(22.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            if !crate::config::STANDALONE {
                ui.label(RichText::new("Team projects").size(15.0).color(theme::TEXT));
                ui.add_space(6.0);
                if app.office_projects.is_empty() {
                    ui.label(RichText::new("No shared projects yet. New projects are saved to the team by default.").color(theme::TEXT_DIM));
                }
                let mut open = None;
                ui.horizontal_wrapped(|ui| {
                    for p in &app.office_projects {
                        let sub = format!(
                            "{}{}",
                            p.modified.get(..16).unwrap_or(&p.modified).replace('T', " "),
                            p.modified_by.as_ref().map(|u| format!(" · {u}")).unwrap_or_default()
                        );
                        let badge = p.locked_by.as_ref().map(|u| format!("✏ {u}"));
                        if project_card(ui, &p.name, &sub, badge.as_deref()).clicked() {
                            open = Some(p.name.clone());
                        }
                    }
                });
                if let Some(name) = open {
                    app.open_office(&name);
                }
                ui.add_space(18.0);
            }
            ui.label(RichText::new(if crate::config::STANDALONE { "Your projects" } else { "On this computer" }).size(15.0).color(theme::TEXT));
            ui.label(RichText::new(app.cfg.projects_dir.display().to_string()).small().color(theme::TEXT_DIM));
            ui.add_space(6.0);
            if app.local_projects.is_empty() {
                ui.label(
                    RichText::new(if crate::config::STANDALONE { "No projects yet. Click New project to start." } else { "No local drafts." })
                        .color(theme::TEXT_DIM),
                );
            }
            let mut open_local = None;
            ui.horizontal_wrapped(|ui| {
                for p in &app.local_projects {
                    let when = chrono::DateTime::<chrono::Local>::from(p.modified).format("%Y-%m-%d %H:%M").to_string();
                    if project_card(ui, &p.name, &when, None).clicked() {
                        open_local = Some(p.path.clone());
                    }
                }
            });
            if let Some(p) = open_local {
                app.open_local(&p);
            }
        });
    });
}

fn project_card(ui: &mut egui::Ui, name: &str, sub: &str, badge: Option<&str>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(200.0, 160.0), egui::Sense::click());
    let p = ui.painter();
    let thumb = egui::Rect::from_min_size(r.min, vec2(r.width(), 112.0));
    p.rect_filled(thumb, CornerRadius::same(10), theme::PANEL_2);
    let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    p.text(thumb.center(), egui::Align2::CENTER_CENTER, initial, egui::FontId::proportional(34.0), theme::TEXT_FAINT);
    if resp.hovered() {
        p.rect_stroke(thumb, CornerRadius::same(10), Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
    }
    let name_pos = egui::pos2(r.min.x + 2.0, thumb.max.y + 8.0);
    let galley = p.layout(name.to_string(), egui::FontId::proportional(13.5), theme::TEXT, r.width() - 20.0);
    p.galley(name_pos, galley, theme::TEXT);
    p.text(egui::pos2(r.min.x + 2.0, r.max.y - 8.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(11.0), theme::TEXT_FAINT);
    if let Some(b) = badge {
        let br = egui::Rect::from_min_size(thumb.min + vec2(10.0, 10.0), vec2(110.0, 20.0));
        p.rect_filled(br, CornerRadius::same(5), Color32::from_rgba_unmultiplied(0, 0, 0, 180));
        p.text(br.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, b, egui::FontId::proportional(11.0), theme::WARN);
    }
    resp.on_hover_text(name)
}

fn next_name(app: &MattCut) -> String {
    let base = chrono::Local::now().format("Project %b %d").to_string();
    let taken = |n: &str| app.office_projects.iter().any(|p| p.name == n) || app.local_projects.iter().any(|p| p.name == n);
    if !taken(&base) {
        return base;
    }
    (2..1000).map(|i| format!("{base} ({i})")).find(|n| !taken(n)).unwrap_or(base)
}

/// New-project and save-a-copy dialogs (any screen).
pub fn dialogs(app: &mut MattCut, ctx: &egui::Context) {
    team_dialog(app, ctx);
    account_dialog(app, ctx);
    if let Some(mut np) = app.new_project.take() {
        let mut keep = true;
        let mut create = false;
        egui::Modal::new(egui::Id::unique("new-project")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading("New project");
            ui.add_space(8.0);
            ui.label(RichText::new("Name").small().color(theme::TEXT_DIM));
            ui.add(egui::TextEdit::singleline(&mut np.name).desired_width(f32::INFINITY));
            ui.add_space(8.0);
            ui.label(RichText::new("Aspect ratio").small().color(theme::TEXT_DIM));
            ui.horizontal_wrapped(|ui| {
                for (i, (label, hint, rw, rh)) in RATIOS.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(58.0, 66.0), egui::Sense::click());
                    let sel = np.ratio == i;
                    let p = ui.painter();
                    p.rect_filled(r, CornerRadius::same(8), if sel { theme::ACCENT_SOFT } else { theme::PANEL_3 });
                    if sel {
                        p.rect_stroke(r, CornerRadius::same(8), Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
                    }
                    let k = 26.0 / (*rw.max(rh) as f32);
                    let box_r = egui::Rect::from_center_size(r.center() - vec2(0.0, 8.0), vec2(*rw as f32 * k, *rh as f32 * k));
                    p.rect_stroke(box_r, CornerRadius::same(2), Stroke::new(1.5, theme::TEXT), egui::StrokeKind::Inside);
                    p.text(r.center_bottom() - vec2(0.0, 10.0), egui::Align2::CENTER_CENTER, *label, egui::FontId::proportional(11.5), theme::TEXT);
                    if resp.on_hover_text(*hint).clicked() {
                        np.ratio = i;
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Resolution");
                egui::ComboBox::new("np-res", "").selected_text(format!("{}p", np.res)).show_ui(ui, |ui| {
                    for r in [480, 720, 1080, 1440, 2160] {
                        ui.selectable_value(&mut np.res, r, format!("{r}p"));
                    }
                });
                ui.label("Frame rate");
                egui::ComboBox::new("np-fps", "").selected_text(format!("{}", np.fps)).show_ui(ui, |ui| {
                    for f in [23.976, 24.0, 25.0, 29.97, 30.0, 50.0, 60.0] {
                        ui.selectable_value(&mut np.fps, f, format!("{f}"));
                    }
                });
            });
            let (w, h) = np.size();
            ui.label(RichText::new(format!("{w} × {h}")).small().color(theme::TEXT_DIM));
            ui.add_space(8.0);
            if !crate::config::STANDALONE {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut np.office, true, "Team (shared)");
                    ui.radio_value(&mut np.office, false, "This computer only");
                });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add(theme::primary("Create")).clicked() {
                    create = true;
                }
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
        });
        if create {
            app.create_project(&np);
            keep = false;
            if app.screen != Screen::Editor {
                keep = true;
            }
        }
        if keep {
            app.new_project = Some(np);
        }
    }
    if let Some(mut name) = app.save_copy.take() {
        let mut keep = true;
        let mut save = false;
        egui::Modal::new(egui::Id::unique("save-copy")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Save a copy");
            ui.label(RichText::new("Someone else is editing this project, so your changes go into a copy.").color(theme::TEXT_DIM));
            ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                if ui.add(theme::primary("Save copy")).clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
        });
        if save {
            app.save_as_copy(&name);
            keep = false;
        }
        if keep {
            app.save_copy = Some(name);
        }
    }
}

fn team_dialog(app: &mut MattCut, ctx: &egui::Context) {
    let Some(mut t) = app.team.take() else { return };
    let mut keep = true;
    let me = app.office.status().login.map(|l| l.email).unwrap_or_default();
    egui::Modal::new(egui::Id::unique("team")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.horizontal(|ui| {
            ui.heading("Team members");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, icons::Icon::Close, "Close", false).clicked() {
                    keep = false;
                }
            });
        });
        ui.label(RichText::new("Only these people can sign in to MattCut. Removing someone signs them out within a minute.").size(12.0).color(theme::TEXT_DIM));
        ui.add_space(10.0);
        if t.loading {
            ui.spinner();
        }
        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            egui::Grid::new("members").num_columns(4).striped(true).spacing(vec2(14.0, 8.0)).show(ui, |ui| {
                for m in t.members.clone() {
                    ui.label(RichText::new(&m.name).color(theme::TEXT));
                    ui.label(RichText::new(&m.email).size(12.0).color(theme::TEXT_DIM));
                    ui.label(
                        RichText::new(format!(
                            "{}{}",
                            if m.role == Role::Admin { "Admin" } else { "Editor" },
                            if m.has_password { " · password" } else { " · Google" }
                        ))
                        .size(11.5)
                        .color(theme::TEXT_FAINT),
                    );
                    ui.horizontal(|ui| {
                        if ui.small_button("New password").clicked() {
                            app.office.send(Cmd::ResetPassword { email: m.email.clone() });
                        }
                        if m.email != me {
                            if t.confirm_remove.as_deref() == Some(m.email.as_str()) {
                                if ui.add(egui::Button::new(RichText::new("Confirm remove").color(Color32::WHITE).size(11.5)).fill(theme::ERR)).clicked() {
                                    app.office.send(Cmd::RemoveMember { email: m.email.clone() });
                                    t.confirm_remove = None;
                                }
                            } else if ui.small_button("Remove").clicked() {
                                t.confirm_remove = Some(m.email.clone());
                            }
                        }
                    });
                    ui.end_row();
                }
            });
        });
        ui.add_space(12.0);
        ui.label(RichText::new("Add someone").size(13.5).color(theme::TEXT));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut t.email).hint_text("email@company.com").desired_width(220.0));
            ui.add(egui::TextEdit::singleline(&mut t.name).hint_text("Name").desired_width(140.0));
            ui.checkbox(&mut t.admin, "Admin");
        });
        ui.checkbox(&mut t.with_password, "Also give them a password (for people without Google)");
        ui.horizontal(|ui| {
            let ok = mattcut_office::normalize_email(&t.email).is_some();
            if ui.add_enabled(ok, theme::primary("Add to team")).clicked() {
                app.office.send(Cmd::AddMember {
                    email: t.email.trim().to_string(),
                    name: t.name.trim().to_string(),
                    role: if t.admin { Role::Admin } else { Role::Editor },
                    password: t.with_password,
                });
                t.email.clear();
                t.name.clear();
                t.admin = false;
            }
        });
        if let Some(n) = &t.notice {
            ui.add_space(8.0);
            egui::Frame::new().fill(theme::ACCENT_SOFT).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(n).color(theme::TEXT).size(12.5)).selectable(true));
            });
        }
    });
    if keep {
        app.team = Some(t);
    }
}

fn account_dialog(app: &mut MattCut, ctx: &egui::Context) {
    let Some(mut a) = app.account.take() else {
        return;
    };
    let mut keep = true;
    egui::Modal::new(egui::Id::unique("account")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.heading("Change password");
        ui.label(RichText::new("Leave \"current\" empty if you've only used Google so far.").size(12.0).color(theme::TEXT_DIM));
        ui.add_space(8.0);
        field(ui, "Current password");
        text_input(ui, &mut a.old, "", true);
        field(ui, "New password (8+ characters)");
        text_input(ui, &mut a.new, "", true);
        field(ui, "New password again");
        text_input(ui, &mut a.again, "", true);
        let ok = a.new.chars().count() >= 8 && a.new == a.again;
        if !a.again.is_empty() && a.new != a.again {
            ui.label(RichText::new("The new passwords don't match").size(12.0).color(theme::ERR));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(ok, theme::primary("Change password")).clicked() {
                app.office.send(Cmd::ChangePassword { old: a.old.clone(), new: a.new.clone() });
                keep = false;
            }
            if ui.button("Cancel").clicked() {
                keep = false;
            }
        });
    });
    if keep {
        app.account = Some(a);
    }
}
