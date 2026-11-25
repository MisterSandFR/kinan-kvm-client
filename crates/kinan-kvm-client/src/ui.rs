//! eframe integration: login screen, port grid, viewer, settings dialog.
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Color32, ColorImage, Context, Key, TextureHandle, TextureOptions};
use kinan_protocol::Modifier;

use crate::config::{save_settings, Settings, Theme};
use crate::i18n::I18n;
use crate::kinan_web::{KinanWeb, PortInfo};
use crate::profiles::{self, is_local_hostname, Profile, Store};
use crate::session::{self, Command, Event, Handle};
use crate::theme;

enum Screen {
    Privacy,
    OpenH264Consent,
    Loading,
    Login,
    Edit(usize), // index into store.profiles; usize::MAX for new
    Grid(usize),
    Viewer { profile: usize, port: u8 },
    Settings,
}

enum H264State {
    Loading {
        downloaded: Arc<std::sync::atomic::AtomicU64>,
        total: Arc<Mutex<Option<u64>>>,
    },
    Ready(std::path::PathBuf),
    Failed(String),
}

pub struct App {
    settings: Settings,
    store: Store,
    i18n: I18n,
    h264: Arc<Mutex<H264State>>,
    screen: Screen,
    local_path_buf: String,
    local_path_error: Option<String>,
    status: String,
    status_at: Instant,
    ports_cache: Arc<Mutex<PortsCache>>,
    web: Option<Arc<KinanWeb>>,
    session: Option<Handle>,
    frame_tex: Option<TextureHandle>,
    frame_size: (u32, u32),
    resolution: (u16, u16),
    fps: f32,
    frames_since: (Instant, u32),
    no_signal: bool,
    input_on: bool,
    input_allowed: bool,
    want_capture: bool,
    confirm_input: bool,
    pressed_mods: HashSet<Modifier>,
    temp_profile: Profile,
    temp_input_ports_str: String,
    temp_port_labels_edit: HashMap<u8, String>,
}

#[derive(Default)]
struct PortsCache {
    last: Vec<PortInfo>,
    error: Option<String>,
    at: Option<Instant>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = crate::config::load_settings();
        let store = profiles::load();
        let mut i18n = I18n::new();
        if settings.language == "auto" {
            i18n.auto_detect();
        } else {
            i18n.set_language(&settings.language);
        }
        theme::apply(&cc.egui_ctx, settings.theme);
        // The library may already be cached on disk from a previous run: try the
        // offline paths first so we only prompt for consent once.
        let h264 = Arc::new(Mutex::new(initial_h264_state(&settings)));
        let starting_screen = if !settings.privacy_warning_seen {
            Screen::Privacy
        } else if matches!(&*h264.lock().unwrap(), H264State::Ready(_)) {
            Screen::Login
        } else {
            Screen::OpenH264Consent
        };
        let local_path_buf = settings
            .openh264_local_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        Self {
            settings,
            store,
            i18n,
            h264,
            screen: starting_screen,
            local_path_buf,
            local_path_error: None,
            status: String::new(),
            status_at: Instant::now(),
            ports_cache: Arc::new(Mutex::new(PortsCache::default())),
            web: None,
            session: None,
            frame_tex: None,
            frame_size: (0, 0),
            resolution: (1024, 768),
            fps: 0.0,
            frames_since: (Instant::now(), 0),
            no_signal: false,
            input_on: false,
            input_allowed: false,
            want_capture: false,
            confirm_input: false,
            pressed_mods: HashSet::new(),
            temp_profile: Profile::default(),
            temp_input_ports_str: String::new(),
            temp_port_labels_edit: HashMap::new(),
        }
    }

    fn t(&self, k: &str) -> String {
        self.i18n.t(k).to_string()
    }

    fn t_fmt(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let mut s = self.t(key);
        for (k, v) in vars {
            s = s.replace(&format!("{{{}}}", k), v);
        }
        s
    }

    fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.status_at = Instant::now();
    }

    fn save_everything(&self) {
        let _ = save_settings(&self.settings);
        let _ = profiles::save(&self.store);
    }

    fn spawn_ports_refresher(&mut self, profile_idx: usize) {
        let profile = self.store.profiles[profile_idx].clone();
        let web = Arc::new(KinanWeb::new(
            format!("http://{}:{}", profile.host, profile.web_port),
            profile.web_user.clone(),
            profile.web_pass.clone(),
        ));
        self.web = Some(web.clone());
        let cache = self.ports_cache.clone();
        std::thread::Builder::new()
            .name("kinan-ports-refresh".into())
            .spawn(move || loop {
                let r = web.ports();
                {
                    let mut c = cache.lock().unwrap();
                    c.at = Some(Instant::now());
                    match r {
                        Ok(v) => {
                            c.last = v;
                            c.error = None;
                        }
                        Err(e) => c.error = Some(format!("{e:#}")),
                    }
                }
                std::thread::sleep(Duration::from_secs(5));
            })
            .expect("spawn ports refresher");
    }

    fn drain_events(&mut self, ctx: &Context) {
        let mut events: Vec<Event> = Vec::new();
        let mut ended = false;
        if let Some(h) = &self.session {
            for _ in 0..64 {
                match h.events.try_recv() {
                    Ok(ev) => {
                        let e = matches!(ev, Event::Ended);
                        events.push(ev);
                        if e {
                            ended = true;
                            break;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        ended = true;
                        break;
                    }
                }
            }
        }
        for ev in events {
            match ev {
                Event::Status(s) => self.set_status(s),
                Event::Resolution(w, h) => self.resolution = (w, h),
                Event::Frame { w, h, rgb } => {
                    self.frame_size = (w, h);
                    let img = ColorImage::from_rgb([w as usize, h as usize], &rgb);
                    if let Some(tex) = self.frame_tex.as_mut() {
                        tex.set(img, TextureOptions::LINEAR);
                    } else {
                        self.frame_tex = Some(ctx.load_texture("kvm", img, TextureOptions::LINEAR));
                    }
                    self.frames_since.1 = self.frames_since.1.saturating_add(1);
                    let elapsed = self.frames_since.0.elapsed();
                    if elapsed >= Duration::from_secs(1) {
                        self.fps = self.frames_since.1 as f32 / elapsed.as_secs_f32();
                        self.frames_since = (Instant::now(), 0);
                    }
                }
                Event::NoSignal(v) => self.no_signal = v,
                Event::Error(e) => self.set_status(self.t_fmt("status.error", &[("msg", &e)])),
                Event::Ended => {}
            }
        }
        if ended {
            self.session = None;
            self.input_on = false;
            self.want_capture = false;
            self.frame_tex = None;
            self.frame_size = (0, 0);
        }
    }

    fn start_session(&mut self, profile_idx: usize, port: u8) {
        let path = match &*self.h264.lock().unwrap() {
            H264State::Ready(p) => p.clone(),
            _ => return,
        };
        self.stop_session();
        let profile = self.store.profiles[profile_idx].clone();
        self.input_allowed = profile.input_ports.contains(&port);
        self.frame_tex = None;
        self.frame_size = (0, 0);
        self.no_signal = false;
        let h = session::spawn(profile, port, path);
        self.session = Some(h);
        self.screen = Screen::Viewer {
            profile: profile_idx,
            port,
        };
        let st = self.t_fmt("status.connecting", &[("port", &port.to_string())]);
        self.set_status(st);
    }

    fn stop_session(&mut self) {
        if let Some(h) = self.session.take() {
            h.stop();
        }
        self.input_on = false;
        self.want_capture = false;
        self.pressed_mods.clear();
    }

    fn send(&self, cmd: Command) {
        if let Some(h) = &self.session {
            let _ = h.commands.send(cmd);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.drain_events(ctx);
        if self.session.is_some() {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else if matches!(self.screen, Screen::Loading) {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        // Transition out of the loading screen once OpenH264 is ready.
        if matches!(self.screen, Screen::Loading) {
            let ready = matches!(&*self.h264.lock().unwrap(), H264State::Ready(_));
            if ready {
                self.screen = Screen::Login;
            }
        }

        match self.screen {
            Screen::Privacy => draw_privacy(self, ctx),
            Screen::OpenH264Consent => draw_openh264_consent(self, ctx),
            Screen::Loading => draw_loading(self, ctx),
            Screen::Login => draw_login(self, ctx),
            Screen::Edit(idx) => draw_edit(self, ctx, idx),
            Screen::Grid(idx) => draw_grid(self, ctx, idx),
            Screen::Viewer { profile, port } => draw_viewer(self, ctx, profile, port),
            Screen::Settings => draw_settings(self, ctx),
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_session();
        self.save_everything();
    }
}

fn top_bar(app: &mut App, ctx: &Context) {
    egui::TopBottomPanel::top("top").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading(app.t("app.title"));
            ui.add_space(16.0);
            if ui.button(app.t("menu.profiles")).clicked() {
                app.screen = Screen::Login;
            }
            if ui.button(app.t("menu.settings")).clicked() {
                app.screen = Screen::Settings;
            }
        });
    });
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if !app.status.is_empty() {
                ui.label(&app.status);
                ui.weak(format!(" ({}s)", app.status_at.elapsed().as_secs()));
            }
        });
    });
}

fn draw_privacy(app: &mut App, ctx: &Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading(app.t("privacy.title"));
            ui.add_space(12.0);
            ui.label(app.t("privacy.body"));
            ui.add_space(24.0);
            if ui.button(app.t("privacy.ack")).clicked() {
                app.settings.privacy_warning_seen = true;
                let _ = save_settings(&app.settings);
                // After privacy, either go straight to Login (library already
                // cached / local path set) or to the OpenH264 consent screen.
                app.screen = if matches!(&*app.h264.lock().unwrap(), H264State::Ready(_)) {
                    Screen::Login
                } else {
                    Screen::OpenH264Consent
                };
            }
        });
    });
}

fn draw_login(app: &mut App, ctx: &Context) {
    top_bar(app, ctx);
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading(app.t("login.title"));
        ui.separator();
        if app.store.profiles.is_empty() {
            ui.label(app.t("login.no_profiles"));
        } else {
            ui.label(app.t("login.saved_profiles"));
        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut connect_idx: Option<usize> = None;
            let mut edit_idx: Option<usize> = None;
            let mut delete_idx: Option<usize> = None;
            for (i, p) in app.store.profiles.iter().enumerate() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&p.name).strong());
                            ui.weak(format!("{}:{}", p.host, p.web_port));
                            if !p.host_is_local() {
                                ui.colored_label(Color32::LIGHT_YELLOW, app.t("privacy.public_host"));
                            }
                        });
                        ui.add_space(ui.available_width() - 220.0);
                        if ui.button(app.t("action.connect")).clicked() {
                            connect_idx = Some(i);
                        }
                        if ui.small_button("edit").clicked() {
                            edit_idx = Some(i);
                        }
                        if ui.small_button(app.t("action.delete")).clicked() {
                            delete_idx = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = delete_idx {
                app.store.profiles.remove(i);
                let _ = profiles::save(&app.store);
            }
            if let Some(i) = edit_idx {
                app.temp_profile = app.store.profiles[i].clone();
                app.temp_input_ports_str = app
                    .temp_profile
                    .input_ports
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                app.temp_port_labels_edit = app.temp_profile.port_labels.clone();
                app.screen = Screen::Edit(i);
            }
            if let Some(i) = connect_idx {
                app.store.last_used = Some(i);
                let _ = profiles::save(&app.store);
                {
                    let mut c = app.ports_cache.lock().unwrap();
                    c.last.clear();
                    c.at = None;
                    c.error = None;
                }
                app.spawn_ports_refresher(i);
                app.screen = Screen::Grid(i);
            }
        });
        ui.add_space(12.0);
        if ui.button(app.t("profile.new")).clicked() {
            app.temp_profile = Profile::default();
            app.temp_input_ports_str.clear();
            app.temp_port_labels_edit.clear();
            app.screen = Screen::Edit(usize::MAX);
        }
    });
}

fn draw_edit(app: &mut App, ctx: &Context, idx: usize) {
    top_bar(app, ctx);
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading(if idx == usize::MAX {
            app.t("profile.new")
        } else {
            app.temp_profile.name.clone()
        });
        ui.separator();
        egui::Grid::new("edit-grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label(app.t("profile.name"));
                ui.text_edit_singleline(&mut app.temp_profile.name);
                ui.end_row();
                ui.label(app.t("profile.host"));
                ui.text_edit_singleline(&mut app.temp_profile.host);
                ui.end_row();
                ui.label(app.t("profile.device_ip"));
                ui.text_edit_singleline(&mut app.temp_profile.device_ip);
                ui.end_row();
                edit_u16(ui, app.i18n.t("profile.web_port"), &mut app.temp_profile.web_port);
                edit_u16(
                    ui,
                    app.i18n.t("profile.switch_port"),
                    &mut app.temp_profile.switch_port,
                );
                edit_u16(
                    ui,
                    app.i18n.t("profile.video_port"),
                    &mut app.temp_profile.video_port,
                );
                edit_u16(ui, app.i18n.t("profile.km_port"), &mut app.temp_profile.km_port);
                ui.label(app.t("profile.user"));
                ui.text_edit_singleline(&mut app.temp_profile.web_user);
                ui.end_row();
                ui.label(app.t("profile.pass"));
                ui.add(egui::TextEdit::singleline(&mut app.temp_profile.web_pass).password(true));
                ui.end_row();
                ui.label(app.t("profile.input_ports"));
                ui.vertical(|ui| {
                    ui.text_edit_singleline(&mut app.temp_input_ports_str);
                    ui.weak(app.i18n.t("profile.input_ports.help"));
                });
                ui.end_row();
            });
        if !app.temp_profile.host.is_empty() && !is_local_hostname(&app.temp_profile.host) {
            ui.add_space(6.0);
            ui.colored_label(Color32::LIGHT_YELLOW, app.i18n.t("privacy.public_host"));
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button(app.t("action.save")).clicked() {
                let parsed: Vec<u8> = app
                    .temp_input_ports_str
                    .split(|c: char| !c.is_ascii_digit())
                    .filter_map(|s| s.parse::<u8>().ok())
                    .filter(|p| (1..=16).contains(p))
                    .collect();
                app.temp_profile.input_ports = parsed;
                app.temp_profile.port_labels = app.temp_port_labels_edit.clone();
                if idx == usize::MAX {
                    app.store.profiles.push(app.temp_profile.clone());
                } else {
                    app.store.profiles[idx] = app.temp_profile.clone();
                }
                let _ = profiles::save(&app.store);
                app.screen = Screen::Login;
            }
            if ui.button(app.t("action.cancel")).clicked() {
                app.screen = Screen::Login;
            }
        });
    });
}

fn edit_u16(ui: &mut egui::Ui, label: &str, v: &mut u16) {
    ui.label(label);
    let mut buf = v.to_string();
    if ui.text_edit_singleline(&mut buf).changed() {
        if let Ok(n) = buf.parse() {
            *v = n;
        }
    }
    ui.end_row();
}

fn draw_grid(app: &mut App, ctx: &Context, profile_idx: usize) {
    top_bar(app, ctx);
    let cache = app.ports_cache.lock().unwrap().clone();
    let profile = app.store.profiles[profile_idx].clone();
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading(format!("{} — {}", profile.name, app.t("grid.title")));
            if !profile.host_is_local() {
                ui.colored_label(Color32::LIGHT_YELLOW, app.t("privacy.public_host"));
            }
        });
        if let Some(e) = &cache.error {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        ui.separator();
        egui::Grid::new("port-grid")
            .num_columns(4)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                let mut to_open: Option<u8> = None;
                for num in 1..=16u8 {
                    let p = cache.last.iter().find(|p| p.num == num);
                    let allowed = profile.input_ports.contains(&num);
                    let label = profile
                        .port_labels
                        .get(&num)
                        .cloned()
                        .unwrap_or_else(|| p.map(|p| p.name.clone()).unwrap_or_default());
                    let (state, color) = match p {
                        Some(p) if p.state == "connect" => {
                            (app.t("grid.state.connected"), Color32::LIGHT_YELLOW)
                        }
                        Some(p) if p.online => (app.t("grid.state.online"), Color32::LIGHT_GREEN),
                        Some(_) => (app.t("grid.state.offline"), Color32::GRAY),
                        None => (app.t("grid.state.unknown"), Color32::DARK_GRAY),
                    };
                    ui.group(|ui| {
                        ui.set_min_size(egui::vec2(220.0, 92.0));
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(color, format!("p{num:02}"));
                                ui.weak(state);
                            });
                            ui.label(
                                egui::RichText::new(if label.is_empty() { "—" } else { &label }).strong(),
                            );
                            ui.horizontal(|ui| {
                                if allowed {
                                    ui.colored_label(Color32::LIGHT_BLUE, "input");
                                }
                                if ui.button(app.t("action.open")).clicked() {
                                    to_open = Some(num);
                                }
                            });
                        });
                    });
                    if num % 4 == 0 {
                        ui.end_row();
                    }
                }
                if let Some(n) = to_open {
                    app.start_session(profile_idx, n);
                }
            });
    });
}

fn draw_viewer(app: &mut App, ctx: &Context, profile_idx: usize, port: u8) {
    top_bar(app, ctx);
    egui::TopBottomPanel::top("viewer-tools").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(app.t_fmt("viewer.title", &[("port", &port.to_string())]));
            ui.separator();
            let input_label = if app.input_on {
                app.t("viewer.input.enabled")
            } else {
                app.t("viewer.input.disabled")
            };
            let btn = egui::Button::new(input_label).fill(if app.input_on {
                Color32::DARK_RED
            } else {
                Color32::DARK_GRAY
            });
            if ui.add_enabled(app.input_allowed, btn).clicked() {
                if app.input_on {
                    app.input_on = false;
                    app.send(Command::EnableInput(false));
                    app.want_capture = false;
                } else {
                    app.confirm_input = true;
                }
            }
            if !app.input_allowed {
                ui.colored_label(Color32::GRAY, app.t("viewer.input.not_allowed"));
            } else if app.input_on {
                let toggle_label = app.t("viewer.input.toggle");
                let cad_label = app.t("viewer.ctrl_alt_del");
                ui.checkbox(&mut app.want_capture, toggle_label);
                if ui.button(cad_label).clicked() {
                    app.send(Command::CtrlAltDel);
                }
            }
            ui.separator();
            ui.label(app.t_fmt(
                "viewer.resolution",
                &[
                    ("w", &app.resolution.0.to_string()),
                    ("h", &app.resolution.1.to_string()),
                ],
            ));
            ui.weak(app.t_fmt("viewer.fps", &[("fps", &format!("{:.0}", app.fps))]));
            if app.no_signal {
                ui.colored_label(Color32::LIGHT_RED, app.t("viewer.no_signal"));
            }
            if ui.button(app.t("action.disconnect")).clicked() {
                app.stop_session();
                app.screen = Screen::Grid(profile_idx);
            }
        });
    });

    egui::CentralPanel::default().show(ctx, |ui| {
        let avail = ui.available_size();
        let (rect, hovered_image) = if let Some(tex) = &app.frame_tex {
            let (w, h) = app.frame_size;
            let scale = (avail.x / w as f32).min(avail.y / h as f32).max(0.1);
            let size = egui::vec2(w as f32 * scale, h as f32 * scale);
            let (r, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
            ui.painter().image(
                tex.id(),
                r,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            (r, resp.hovered())
        } else {
            let (r, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
            ui.painter().rect_filled(r, 0.0, Color32::from_gray(10));
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                app.t("status.waiting_frame"),
                egui::FontId::proportional(20.0),
                Color32::LIGHT_GRAY,
            );
            (r, false)
        };
        poll_input(app, ctx, rect, hovered_image);
    });

    if app.confirm_input {
        let mut open = true;
        egui::Window::new(app.t("confirm.enable_input.title"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(app.t_fmt("confirm.enable_input.body", &[("port", &port.to_string())]));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(app.t("confirm.enable_input.yes")).clicked() {
                        app.input_on = true;
                        app.send(Command::EnableInput(true));
                        app.confirm_input = false;
                    }
                    if ui.button(app.t("action.cancel")).clicked() {
                        app.confirm_input = false;
                    }
                });
            });
        if !open {
            app.confirm_input = false;
        }
    }
}

fn poll_input(app: &mut App, ctx: &Context, image: egui::Rect, hovered: bool) {
    if !app.input_on || !app.want_capture || app.session.is_none() {
        return;
    }
    ctx.input(|i| {
        if hovered {
            if let Some(p) = i.pointer.hover_pos() {
                let x = ((p.x - image.min.x) / image.width().max(1.0)).clamp(0.0, 1.0);
                let y = ((p.y - image.min.y) / image.height().max(1.0)).clamp(0.0, 1.0);
                let mut btns = 0u8;
                if i.pointer.primary_down() {
                    btns |= 0x01;
                }
                if i.pointer.secondary_down() {
                    btns |= 0x02;
                }
                if i.pointer.middle_down() {
                    btns |= 0x04;
                }
                let dy = i.smooth_scroll_delta.y;
                let wheel = if dy >= 1.0 {
                    1
                } else if dy <= -1.0 {
                    -1
                } else {
                    0
                };
                app.send(Command::Mouse {
                    x_norm: x,
                    y_norm: y,
                    buttons: btns,
                    wheel,
                });
            }
        }
        // Modifier diff.
        let want = &[
            (Modifier::ControlLeft, i.modifiers.ctrl),
            (Modifier::ShiftLeft, i.modifiers.shift),
            (Modifier::AltLeft, i.modifiers.alt),
            (Modifier::MetaLeft, i.modifiers.command && !i.modifiers.ctrl),
        ];
        for (m, down) in want {
            let prev = app.pressed_mods.contains(m);
            if *down && !prev {
                app.pressed_mods.insert(*m);
                app.send(Command::Key {
                    down_usage: 0,
                    up_usage: 0,
                    add_mod: Some(*m),
                    remove_mod: None,
                });
            } else if !*down && prev {
                app.pressed_mods.remove(m);
                app.send(Command::Key {
                    down_usage: 0,
                    up_usage: 0,
                    add_mod: None,
                    remove_mod: Some(*m),
                });
            }
        }
        for ev in &i.events {
            if let egui::Event::Key {
                key,
                pressed,
                repeat: false,
                ..
            } = ev
            {
                let usage = key_to_hid(*key);
                if usage == 0 {
                    continue;
                }
                let (down, up) = if *pressed { (usage, 0) } else { (0, usage) };
                app.send(Command::Key {
                    down_usage: down,
                    up_usage: up,
                    add_mod: None,
                    remove_mod: None,
                });
            }
        }
    });
}

fn draw_settings(app: &mut App, ctx: &Context) {
    top_bar(app, ctx);
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading(app.t("settings.title"));
        ui.separator();
        egui::Grid::new("settings-grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label(app.t("settings.language"));
                let langs = app.i18n.available_languages();
                let current = app.settings.language.clone();
                egui::ComboBox::from_id_salt("lang")
                    .selected_text(if current == "auto" {
                        app.t("settings.language.auto")
                    } else {
                        current.clone()
                    })
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(current == "auto", app.t("settings.language.auto"))
                            .clicked()
                        {
                            app.settings.language = "auto".into();
                            app.i18n.auto_detect();
                            let _ = save_settings(&app.settings);
                        }
                        for tag in langs {
                            if ui.selectable_label(current == tag, tag).clicked() {
                                app.settings.language = tag.into();
                                app.i18n.set_language(tag);
                                let _ = save_settings(&app.settings);
                            }
                        }
                    });
                ui.end_row();

                ui.label(app.t("settings.theme"));
                let t_light = app.t("settings.theme.light");
                let t_dark = app.t("settings.theme.dark");
                let t_sys = app.t("settings.theme.system");
                egui::ComboBox::from_id_salt("theme")
                    .selected_text(match app.settings.theme {
                        Theme::Light => t_light.clone(),
                        Theme::Dark => t_dark.clone(),
                        Theme::System => t_sys.clone(),
                    })
                    .show_ui(ui, |ui| {
                        for (v, label) in [
                            (Theme::Light, t_light),
                            (Theme::Dark, t_dark),
                            (Theme::System, t_sys),
                        ] {
                            if ui.selectable_label(app.settings.theme == v, label).clicked() {
                                app.settings.theme = v;
                                theme::apply(ctx, v);
                                let _ = save_settings(&app.settings);
                            }
                        }
                    });
                ui.end_row();
            });
        ui.add_space(12.0);
        ui.separator();
        ui.label(app.t("about.unaffiliated"));
        ui.label(app.t("about.license"));
        ui.label(app.t("about.cisco_credit"));
    });
}

impl Clone for PortsCache {
    fn clone(&self) -> Self {
        Self {
            last: self.last.clone(),
            error: self.error.clone(),
            at: self.at,
        }
    }
}

fn key_to_hid(k: Key) -> u8 {
    match k {
        Key::A => 0x04,
        Key::B => 0x05,
        Key::C => 0x06,
        Key::D => 0x07,
        Key::E => 0x08,
        Key::F => 0x09,
        Key::G => 0x0A,
        Key::H => 0x0B,
        Key::I => 0x0C,
        Key::J => 0x0D,
        Key::K => 0x0E,
        Key::L => 0x0F,
        Key::M => 0x10,
        Key::N => 0x11,
        Key::O => 0x12,
        Key::P => 0x13,
        Key::Q => 0x14,
        Key::R => 0x15,
        Key::S => 0x16,
        Key::T => 0x17,
        Key::U => 0x18,
        Key::V => 0x19,
        Key::W => 0x1A,
        Key::X => 0x1B,
        Key::Y => 0x1C,
        Key::Z => 0x1D,
        Key::Num1 => 0x1E,
        Key::Num2 => 0x1F,
        Key::Num3 => 0x20,
        Key::Num4 => 0x21,
        Key::Num5 => 0x22,
        Key::Num6 => 0x23,
        Key::Num7 => 0x24,
        Key::Num8 => 0x25,
        Key::Num9 => 0x26,
        Key::Num0 => 0x27,
        Key::Enter => 0x28,
        Key::Escape => 0x29,
        Key::Backspace => 0x2A,
        Key::Tab => 0x2B,
        Key::Space => 0x2C,
        Key::Minus => 0x2D,
        Key::Equals => 0x2E,
        Key::OpenBracket => 0x2F,
        Key::CloseBracket => 0x30,
        Key::Backslash => 0x31,
        Key::Semicolon => 0x33,
        Key::Quote => 0x34,
        Key::Backtick => 0x35,
        Key::Comma => 0x36,
        Key::Period => 0x37,
        Key::Slash => 0x38,
        Key::F1 => 0x3A,
        Key::F2 => 0x3B,
        Key::F3 => 0x3C,
        Key::F4 => 0x3D,
        Key::F5 => 0x3E,
        Key::F6 => 0x3F,
        Key::F7 => 0x40,
        Key::F8 => 0x41,
        Key::F9 => 0x42,
        Key::F10 => 0x43,
        Key::F11 => 0x44,
        Key::F12 => 0x45,
        Key::Insert => 0x49,
        Key::Home => 0x4A,
        Key::PageUp => 0x4B,
        Key::Delete => 0x4C,
        Key::End => 0x4D,
        Key::PageDown => 0x4E,
        Key::ArrowRight => 0x4F,
        Key::ArrowLeft => 0x50,
        Key::ArrowDown => 0x51,
        Key::ArrowUp => 0x52,
        _ => 0,
    }
}

fn spawn_h264_fetch(
    state: Arc<Mutex<H264State>>,
    downloaded: Arc<std::sync::atomic::AtomicU64>,
    total: Arc<Mutex<Option<u64>>>,
    ctx: Context,
) {
    std::thread::Builder::new()
        .name("kinan-h264-fetch".into())
        .spawn(move || {
            let total_for_cb = total.clone();
            let downloaded_for_cb = downloaded.clone();
            let ctx_for_cb = ctx.clone();
            let progress: kinan_h264::ProgressFn = Arc::new(move |d: u64, t: Option<u64>| {
                downloaded_for_cb.store(d, std::sync::atomic::Ordering::SeqCst);
                if let Some(tt) = t {
                    *total_for_cb.lock().unwrap() = Some(tt);
                }
                ctx_for_cb.request_repaint();
            });
            let result = kinan_h264::ensure_available(Some(progress));
            let mut s = state.lock().unwrap();
            *s = match result {
                Ok(p) => H264State::Ready(p),
                Err(e) => H264State::Failed(format!("{e:#}")),
            };
            ctx.request_repaint();
        })
        .expect("spawn h264 fetch thread");
}

fn draw_loading(app: &mut App, ctx: &Context) {
    // Snapshot the state to release the mutex before drawing.
    let snap = match &*app.h264.lock().unwrap() {
        H264State::Loading { downloaded, total } => {
            let d = downloaded.load(std::sync::atomic::Ordering::SeqCst);
            let t = *total.lock().unwrap();
            Some((d, t, None::<String>))
        }
        H264State::Ready(_) => None,
        H264State::Failed(e) => Some((0, None, Some(e.clone()))),
    };
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.heading(app.t("app.title"));
            ui.add_space(24.0);
            match snap {
                Some((downloaded, total, None)) => {
                    ui.label(app.t("openh264.fetching"));
                    ui.weak(app.t("openh264.explain"));
                    ui.add_space(12.0);
                    let frac = match total {
                        Some(t) if t > 0 => (downloaded as f32 / t as f32).clamp(0.0, 1.0),
                        _ => 0.0,
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(360.0)
                            .text(format!("{} KB", downloaded / 1024)),
                    );
                }
                Some((_, _, Some(err))) => {
                    ui.colored_label(Color32::LIGHT_RED, app.t("openh264.failed"));
                    ui.weak(err);
                    ui.add_space(12.0);
                    if ui.button(app.t("action.refresh")).clicked() {
                        // Restart the fetch.
                        let downloaded = Arc::new(std::sync::atomic::AtomicU64::new(0));
                        let total = Arc::new(Mutex::new(None));
                        *app.h264.lock().unwrap() = H264State::Loading {
                            downloaded: downloaded.clone(),
                            total: total.clone(),
                        };
                        spawn_h264_fetch(app.h264.clone(), downloaded, total, ctx.clone());
                    }
                }
                None => {
                    ui.spinner();
                }
            }
        });
    });
}

fn initial_h264_state(settings: &Settings) -> H264State {
    // 1. User-provided offline path overrides everything.
    if let Some(p) = &settings.openh264_local_path {
        if p.is_file() && kinan_h264::verify_any(p).is_ok() {
            return H264State::Ready(p.clone());
        }
    }
    // 2. Cached download from a previous launch.
    if let Ok(p) = kinan_h264::cache_path() {
        if p.is_file() && kinan_h264::verify(&p).is_ok() {
            return H264State::Ready(p);
        }
    }
    // 3. Nothing available yet. Waiting for consent.
    H264State::Loading {
        downloaded: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        total: Arc::new(Mutex::new(None)),
    }
}

fn draw_openh264_consent(app: &mut App, ctx: &Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.heading(app.t("openh264.consent.title"));
            ui.add_space(12.0);
            ui.label(app.t("openh264.consent.body"));
            ui.add_space(6.0);
            ui.weak(app.t("openh264.consent.license"));
            ui.add_space(6.0);
            ui.label(app.t("openh264.consent.credit"));
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.add_space(ui.available_width() / 2.0 - 180.0);
                if ui.button(app.t("openh264.consent.accept")).clicked() {
                    app.settings.openh264_consent = true;
                    let _ = save_settings(&app.settings);
                    kick_off_download(app, ctx.clone());
                    app.screen = Screen::Loading;
                }
                if ui.button(app.t("action.cancel")).clicked() {
                    std::process::exit(0);
                }
            });
            ui.add_space(24.0);
            ui.separator();
            ui.add_space(12.0);
            ui.heading(app.t("openh264.local.title"));
            ui.label(app.t("openh264.local.body"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut app.local_path_buf);
                if ui.button(app.t("openh264.local.use")).clicked() {
                    let p = std::path::PathBuf::from(app.local_path_buf.trim());
                    match kinan_h264::verify_any(&p) {
                        Ok(()) => {
                            app.settings.openh264_local_path = Some(p.clone());
                            let _ = save_settings(&app.settings);
                            *app.h264.lock().unwrap() = H264State::Ready(p);
                            app.local_path_error = None;
                            app.screen = Screen::Login;
                        }
                        Err(e) => app.local_path_error = Some(format!("{e:#}")),
                    }
                }
            });
            if let Some(e) = &app.local_path_error {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
        });
    });
}

fn kick_off_download(app: &App, ctx: Context) {
    let downloaded = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let total = Arc::new(Mutex::new(None));
    *app.h264.lock().unwrap() = H264State::Loading {
        downloaded: downloaded.clone(),
        total: total.clone(),
    };
    spawn_h264_fetch(app.h264.clone(), downloaded, total, ctx);
}
