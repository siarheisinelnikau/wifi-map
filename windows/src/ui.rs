use crate::app::{PendingAction, Tool, Undo, UpdateState, WifiApp};
use egui::{Align2, Color32, FontId, RichText, Stroke, Vec2, pos2, vec2};
use std::time::Duration;
use wifi_map::{
    error::AppError,
    heatmap,
    i18n::{LANGUAGES, Text},
    model::*,
    preferences::Theme,
    updates,
};

#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub selected: Color32,
}
impl Palette {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                background: Color32::from_rgb(239, 244, 245),
                surface: Color32::from_rgb(250, 252, 252),
                text: Color32::from_rgb(25, 48, 62),
                muted: Color32::from_rgb(79, 102, 113),
                border: Color32::from_rgb(207, 220, 223),
                accent: Color32::from_rgb(11, 115, 111),
                selected: Color32::from_rgb(213, 239, 232),
            },
            Theme::Dark => Self {
                background: Color32::from_rgb(16, 28, 37),
                surface: Color32::from_rgb(24, 40, 50),
                text: Color32::from_rgb(230, 240, 240),
                muted: Color32::from_rgb(161, 183, 192),
                border: Color32::from_rgb(59, 80, 91),
                accent: Color32::from_rgb(54, 170, 151),
                selected: Color32::from_rgb(31, 78, 76),
            },
        }
    }
}
pub fn apply_style(ctx: &egui::Context, theme: Theme, reduced_motion: bool) {
    let p = Palette::for_theme(theme);
    let mut style = (*ctx.style()).clone();
    style.visuals = if theme == Theme::Dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.visuals.panel_fill = p.surface;
    style.visuals.window_fill = p.surface;
    style.visuals.extreme_bg_color = p.background;
    style.visuals.faint_bg_color = p.background;
    style.visuals.override_text_color = Some(p.text);
    style.visuals.weak_text_color = Some(p.muted);
    style.visuals.text_cursor.blink = false;
    style.visuals.selection.bg_fill = p.selected;
    style.visuals.selection.stroke = Stroke::new(1.5, p.accent);
    style.visuals.widgets.inactive.bg_fill = p.background;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
    style.visuals.widgets.hovered.bg_fill = p.selected;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.5, p.accent);
    style.visuals.widgets.active.bg_fill = p.selected;
    style.visuals.widgets.active.bg_stroke = Stroke::new(2.0, p.accent);
    style.visuals.window_corner_radius = egui::CornerRadius::same(12);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(8);
    style.spacing.item_spacing = vec2(8.0, 9.0);
    style.spacing.button_padding = vec2(12.0, 8.0);
    style.spacing.interact_size = vec2(32.0, 32.0);
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(24.0));
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
    style.animation_time = if reduced_motion { 0.0 } else { 0.14 };
    ctx.set_style(style);
}
fn section(ui: &mut egui::Ui, text: &str, p: Palette) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(12.0).strong().color(p.muted));
}
fn survey_name(project: &Project, index: usize, t: Text) -> String {
    if project.surveys[index].name.is_empty() {
        format!("{} {}", t.get("survey"), index + 1)
    } else {
        project.surveys[index].name.clone()
    }
}
impl WifiApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.drain_events(ctx);
        if !self.startup_check {
            self.startup_check = true;
            self.check_updates(false);
        }
        let t = self.text();
        let p = Palette::for_theme(self.prefs.theme);
        self.shortcuts(ctx);
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(20, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("WifiMap").size(25.0).strong());
                    ui.label(RichText::new(t.get("tagline")).color(p.muted));
                    if self.project.demo {
                        ui.label(
                            RichText::new(t.get("demo_badge"))
                                .color(p.accent)
                                .strong()
                                .size(12.0),
                        );
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(self.busy.is_none(), |ui| {
                        if ui.button(t.get("new")).clicked() {
                            self.request_action(PendingAction::New);
                        }
                        if ui.button(t.get("open")).clicked() {
                            self.request_action(PendingAction::Open);
                        }
                        if ui.button(t.get("save")).clicked() {
                            self.save(false);
                        }
                        ui.menu_button("▾", |ui| {
                            if ui.button(t.get("save_as")).clicked() {
                                self.save(true);
                                ui.close();
                            }
                        })
                        .response
                        .on_hover_text(t.get("save_as"));
                        ui.separator();
                        if ui
                            .button(t.get("import"))
                            .on_hover_text(t.get("import_note"))
                            .clicked()
                        {
                            self.request_action(PendingAction::Import);
                        }
                        if ui
                            .add_enabled(
                                self.ap.is_some() && !self.calculating,
                                egui::Button::new(t.get("export")),
                            )
                            .clicked()
                        {
                            self.export();
                        }
                    });
                    if ui.button(t.get("settings")).clicked() {
                        self.settings = true;
                    }
                    if matches!(
                        self.update,
                        UpdateState::Available(_) | UpdateState::Ready(_)
                    ) && ui
                        .button(RichText::new(t.get("update_available")).color(p.accent))
                        .clicked()
                    {
                        self.settings = true;
                    }
                });
            });
        egui::TopBottomPanel::bottom("legend")
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(20, 12)),
            )
            .show(ctx, |ui| {
                if let Some(error) = self.status.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            if self.prefs.theme == Theme::Dark {
                                Color32::from_rgb(255, 194, 128)
                            } else {
                                Color32::from_rgb(143, 68, 35)
                            },
                            t.get(error.key),
                        );
                        if ui.small_button(t.get("close")).clicked() {
                            self.status = None;
                        }
                    });
                    if !error.detail.is_empty() {
                        ui.collapsing(t.get("details"), |ui| {
                            ui.label(error.detail);
                        });
                    }
                } else if let Some(key) = self.notice {
                    ui.label(RichText::new(t.get(key)).color(p.accent));
                }
                self.legend(ui, t, p);
            });
        egui::SidePanel::left("survey-controls")
            .exact_width(278.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(p.surface).inner_margin(18))
            .show(ctx, |ui| {
                if self.busy == Some("working") {
                    ui.disable();
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.sidebar(ui, t, p));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.background).inner_margin(16))
            .show(ctx, |ui| {
                if self.busy == Some("working") {
                    ui.disable();
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.project.name)
                                .hint_text(t.get("untitled"))
                                .char_limit(100)
                                .desired_width(260.0),
                        )
                        .on_hover_text(t.get("plan_name"))
                        .changed()
                    {
                        self.mark_changed();
                    }
                    if self.dirty {
                        ui.label(RichText::new("●").color(p.accent));
                    }
                    ui.label(format!(
                        "{} {}",
                        self.project.surveys[self.survey].samples.len(),
                        t.get("points")
                    ));
                });
                ui.horizontal_wrapped(|ui| {
                    for (tool, key) in [
                        (Tool::Point, "point_tool"),
                        (Tool::Wall, "wall_tool"),
                        (Tool::Scale, "scale_tool"),
                        (Tool::Router, "router_tool"),
                    ] {
                        if ui.selectable_label(self.tool == tool, t.get(key)).clicked() {
                            self.tool = tool;
                            self.anchor = None;
                            self.calibration = None;
                        }
                    }
                    if ui.button(t.get("fit")).clicked() {
                        self.fit_pending = true;
                    }
                    if ui
                        .add_enabled(self.undo.is_some(), egui::Button::new(t.get("undo")))
                        .clicked()
                    {
                        self.undo_edit();
                    }
                });
                if self.project.plan.image.is_none()
                    && self.project.plan.walls.is_empty()
                    && self.point.is_none()
                    && self.tool == Tool::Point
                {
                    ui.add_space(8.0);
                    ui.heading(t.get("welcome"));
                    ui.label(t.get("welcome_detail"));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(t.get("import")).clicked() {
                            self.request_action(PendingAction::Import);
                        }
                        if ui.button(t.get("demo")).clicked() {
                            self.request_action(PendingAction::Demo);
                        }
                    });
                    ui.add_space(8.0);
                }
                match self.tool {
                    Tool::Wall => {
                        ui.label(t.get("wall_hint"));
                    }
                    Tool::Scale => {
                        ui.label(t.get("scale_hint"));
                    }
                    _ => {
                        ui.label(RichText::new(t.get("pan_hint")).small().color(p.muted));
                    }
                }
                self.canvas(ui, ctx, t, p);
            });
        if self.settings {
            self.settings_window(ctx, t, p);
        }
        if self.confirming {
            egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
                ui.set_max_width(440.0);
                ui.heading(t.get("unsaved"));
                ui.label(t.get("unsaved_detail"));
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(self.busy.is_none(), egui::Button::new(t.get("save")))
                        .clicked()
                    {
                        self.confirming = false;
                        self.save(false);
                    }
                    if ui.button(t.get("discard")).clicked()
                        && let Some(action) = self.pending.take()
                    {
                        self.perform_action(action);
                    }
                    if ui.button(t.get("cancel")).clicked() {
                        self.pending = None;
                        self.confirming = false;
                    }
                });
            });
        }
        if self.drag.is_none() {
            self.queue_heat();
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.confirming || self.busy == Some("working") {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S)) {
            self.save(false);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::O)) {
            self.request_action(PendingAction::Open);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::N)) {
            self.request_action(PendingAction::New);
        }
        if !ctx.wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z))
        {
            self.undo_edit();
        }
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
            self.remove_point();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.anchor = None;
            self.calibration = None;
        }
    }
    fn sidebar(&mut self, ui: &mut egui::Ui, t: Text, p: Palette) {
        section(ui, t.get("surveys"), p);
        let previous = self.survey;
        egui::ComboBox::from_id_salt("survey")
            .width(ui.available_width())
            .selected_text(survey_name(&self.project, self.survey, t))
            .show_ui(ui, |ui| {
                for i in 0..self.project.surveys.len() {
                    ui.selectable_value(&mut self.survey, i, survey_name(&self.project, i, t));
                }
            });
        if previous != self.survey {
            self.selected = None;
            self.point = None;
            self.anchor = None;
            self.ap = self
                .project
                .access_points(self.survey)
                .first()
                .map(|(ap, _)| *ap)
                .or(self.ap);
            if self.baseline == self.survey {
                self.baseline = previous;
            }
            self.invalidate_heat();
        }
        if ui
            .add(
                egui::TextEdit::singleline(&mut self.project.surveys[self.survey].name)
                    .hint_text(t.get("survey_name"))
                    .desired_width(f32::INFINITY)
                    .char_limit(100),
            )
            .changed()
        {
            self.mark_changed();
        }
        if ui
            .add_enabled(self.busy.is_none(), egui::Button::new(t.get("add_survey")))
            .clicked()
        {
            let name = format!("{} {}", t.get("survey"), self.project.surveys.len() + 1);
            match self.project.add_survey(name) {
                Ok(index) => {
                    self.baseline = self.survey;
                    self.survey = index;
                    self.selected = None;
                    self.point = None;
                    self.mark_changed();
                    self.invalidate_heat();
                }
                Err(key) => self.status = Some(AppError::new(key)),
            }
        }
        if self.project.surveys.len() > 1 {
            let previous = (self.compare, self.baseline);
            ui.checkbox(&mut self.compare, t.get("compare"));
            if self.compare {
                ui.label(t.get("baseline"));
                egui::ComboBox::from_id_salt("baseline")
                    .width(ui.available_width())
                    .selected_text(survey_name(&self.project, self.baseline, t))
                    .show_ui(ui, |ui| {
                        for i in 0..self.project.surveys.len() {
                            if i != self.survey {
                                ui.selectable_value(
                                    &mut self.baseline,
                                    i,
                                    survey_name(&self.project, i, t),
                                );
                            }
                        }
                    });
            }
            if previous != (self.compare, self.baseline) {
                self.invalidate_heat();
            }
        }
        ui.separator();
        section(ui, t.get("coverage"), p);
        ui.label(t.get("access_point"));
        let mut list = self.project.access_points(self.survey);
        for observation in &self.access_points {
            if !list.iter().any(|(b, _)| *b == observation.bssid) {
                list.push((observation.bssid, observation.ssid_label()));
            }
        }
        let previous = self.ap;
        let label = self
            .ap
            .and_then(|ap| {
                list.iter()
                    .find(|(b, _)| *b == ap)
                    .map(|(_, name)| name.clone())
            })
            .unwrap_or_else(|| t.get("select_ap").into());
        egui::ComboBox::from_id_salt("ap")
            .width(ui.available_width())
            .selected_text(label)
            .show_ui(ui, |ui| {
                for (ap, name) in &list {
                    ui.selectable_value(&mut self.ap, Some(*ap), format!("{name} · {ap}"));
                }
            });
        if let Some(ap) = self.ap {
            ui.label(RichText::new(ap.to_string()).monospace().color(p.muted));
        }
        if previous != self.ap {
            self.selected = None;
            self.invalidate_heat();
        }
        ui.label(t.get("metric"));
        let previous = self.metric;
        egui::ComboBox::from_id_salt("metric")
            .width(ui.available_width())
            .selected_text(t.get(if self.metric == Metric::RssiDbm {
                "rssi"
            } else {
                "quality"
            }))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.metric, Metric::RssiDbm, t.get("rssi"));
                ui.selectable_value(
                    &mut self.metric,
                    Metric::LinkQualityPercent,
                    t.get("quality"),
                );
            });
        if previous != self.metric {
            self.invalidate_heat();
        }
        if self.project.plan.calibration.is_some() {
            ui.label(t.get("radius"));
            if ui
                .add(egui::Slider::new(&mut self.radius, 1.0..=10.0).step_by(0.5))
                .changed()
            {
                self.invalidate_heat();
            }
        } else {
            ui.label(RichText::new(t.get("scale_missing")).small().color(p.muted));
        }
        if let Some((a, b)) = self.calibration {
            ui.separator();
            ui.label(t.get("distance"));
            ui.add(
                egui::DragValue::new(&mut self.scale_m)
                    .range(0.01..=1000.0)
                    .speed(0.1)
                    .suffix(" m"),
            );
            if ui.button(t.get("apply_scale")).clicked() {
                let c = Calibration {
                    a,
                    b,
                    meters: self.scale_m,
                };
                if c.meters_per_pixel().is_some() {
                    self.project.plan.calibration = Some(c);
                    self.calibration = None;
                    self.tool = Tool::Point;
                    self.mark_changed();
                    self.invalidate_heat();
                }
            }
        }
        if self.tool == Tool::Wall
            && !self.project.plan.walls.is_empty()
            && ui.button(t.get("clear_wall")).clicked()
        {
            self.project.plan.walls.pop();
            self.undo = None;
            self.mark_changed();
        }
        ui.separator();
        if self.project.demo {
            if ui.button(t.get("new_real")).clicked() {
                self.request_action(PendingAction::New);
            }
            ui.label(RichText::new(t.get("demo_isolated")).small().color(p.muted));
        } else {
            ui.label(t.get("adapter"));
            if !self.adapters.is_empty() {
                let previous = self.adapter;
                egui::ComboBox::from_id_salt("adapter")
                    .width(ui.available_width())
                    .selected_text(&self.adapters[self.adapter].name)
                    .show_ui(ui, |ui| {
                        for (i, a) in self.adapters.iter().enumerate() {
                            ui.selectable_value(&mut self.adapter, i, &a.name);
                        }
                    });
                if previous != self.adapter {
                    self.access_points.clear();
                }
                ui.label(
                    RichText::new(t.get(if self.adapters[self.adapter].connected {
                        "connected"
                    } else {
                        "disconnected"
                    }))
                    .small()
                    .color(p.muted),
                );
            }
            if ui
                .add_enabled(
                    self.busy.is_none(),
                    egui::Button::new(t.get(if self.adapters.is_empty() {
                        "refresh"
                    } else {
                        "scan"
                    })),
                )
                .clicked()
            {
                if self.adapters.is_empty() {
                    self.find_adapters();
                } else {
                    self.scan(false);
                }
            }
            ui.add_space(4.0);
            let enabled = self.busy.is_none()
                && self.point.is_some()
                && !self.adapters.is_empty()
                && self.ap.is_some();
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(
                        RichText::new(t.get("measure"))
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(if self.prefs.theme == Theme::Dark {
                        Color32::from_rgb(15, 112, 104)
                    } else {
                        p.accent
                    })
                    .min_size(vec2(ui.available_width(), 44.0)),
                )
                .on_hover_text(t.get("measure_hint"))
                .clicked()
            {
                self.scan(true);
            }
            if self.point.is_none() {
                ui.label(RichText::new(t.get("pick_point")).small().color(p.muted));
            }
        }
        if let Some(label) = self.busy {
            ui.label(t.get(label));
            if matches!(label, "scanning" | "local_test") && ui.button(t.get("cancel")).clicked() {
                self.cancel.cancel();
            }
        }
        if let Some(point) = self.point {
            ui.label(
                RichText::new(format!("x {:.0} · y {:.0} px", point.x, point.y))
                    .small()
                    .color(p.muted),
            );
        }
        if let Some(sample) = self.selected_sample().cloned() {
            ui.separator();
            section(ui, t.get("point_details"), p);
            let o = &sample.observation;
            ui.label(RichText::new(o.ssid_label()).strong());
            ui.label(RichText::new(o.bssid.to_string()).monospace());
            for (key, value) in [
                ("rssi", o.rssi_dbm.map(|v| format!("{v} dBm"))),
                ("quality", o.link_quality_percent.map(|v| format!("{v} %"))),
                ("channel", o.channel.map(|v| v.to_string())),
                (
                    "frequency",
                    o.frequency_khz
                        .map(|v| format!("{:.0} MHz", v as f64 / 1000.0)),
                ),
            ] {
                ui.horizontal_wrapped(|ui| {
                    ui.label(t.get(key));
                    ui.label(
                        RichText::new(value.unwrap_or_else(|| t.get("unavailable").into()))
                            .strong(),
                    );
                });
            }
            ui.label(RichText::new(t.get("timestamp")).small().color(p.muted));
            ui.label(RichText::new(wifi_map::export::utc_time(o.observed_unix_ms)).monospace());
            ui.label(RichText::new(t.get("collected")).small().color(p.muted));
            ui.label(RichText::new(wifi_map::export::utc_time(o.collected_unix_ms)).monospace());
            if let Some(probe) = &sample.local_probe {
                ui.label(format!("{}: {:.2}", t.get("rtt"), probe.median_rtt_ms));
                ui.label(format!(
                    "{}: {:.2}",
                    t.get("throughput"),
                    probe.payload_mbps
                ));
                ui.label(RichText::new(&probe.peer).monospace());
            }
            if ui.button(t.get("remove_point")).clicked() {
                self.remove_point();
            }
        }
        ui.separator();
        ui.collapsing(t.get("measured"), |ui| {
            let samples: Vec<_> = self.project.surveys[self.survey]
                .samples
                .iter()
                .enumerate()
                .filter(|(_, s)| Some(s.observation.bssid) == self.ap)
                .map(|(i, s)| (i, s.id, s.position, s.observation.value(self.metric)))
                .collect();
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .id_salt("point-list")
                .show(ui, |ui| {
                    for (i, id, point, value) in samples {
                        let value = value
                            .map(|v| format!("{v:.0} {}", self.metric.unit()))
                            .unwrap_or_else(|| t.get("unavailable").into());
                        if ui
                            .selectable_label(
                                self.selected == Some(id),
                                format!("#{} · {value}", i + 1),
                            )
                            .clicked()
                        {
                            self.select(Some(id), point);
                        }
                    }
                });
        });
        if !self.project.demo {
            ui.collapsing(t.get("local_test"), |ui| {
                ui.label(RichText::new(t.get("peer_hint")).small());
                ui.label(RichText::new(t.get("tcp_route_note")).small());
                ui.label(t.get("peer_address_label"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.peer_address)
                        .hint_text("192.168.1.20:46820")
                        .char_limit(128)
                        .desired_width(f32::INFINITY),
                );
                ui.label(t.get("peer_token_label"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.peer_token)
                        .password(true)
                        .char_limit(32)
                        .desired_width(f32::INFINITY),
                );
                if ui
                    .add_enabled(
                        self.selected.is_some() && self.busy.is_none(),
                        egui::Button::new(t.get("run_peer")),
                    )
                    .clicked()
                {
                    self.run_probe();
                }
            });
        }
        ui.add_space(8.0);
        ui.label(RichText::new(t.get("api_note")).small().color(p.muted));
    }
    fn legend(&self, ui: &mut egui::Ui, t: Text, p: Palette) {
        let difference = self.current_baseline().is_some();
        let (min, max) = if difference {
            (-20.0, 20.0)
        } else {
            self.metric.range()
        };
        let unit = if difference {
            if self.metric == Metric::RssiDbm {
                "dB"
            } else {
                "pp"
            }
        } else {
            self.metric.unit()
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(t.get(if difference { "difference" } else { "legend" })).strong(),
            );
            ui.label(format!(
                "{} {min:.0}",
                t.get(if difference { "loss" } else { "weak" })
            ));
            let (rect, _) = ui.allocate_exact_size(vec2(160.0, 12.0), egui::Sense::hover());
            for i in 0..80 {
                let c =
                    heatmap::color(min + (max - min) * i as f32 / 79.0, self.metric, difference);
                let x = rect.left() + i as f32 * 2.0;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(pos2(x, rect.top()), vec2(2.2, 12.0)),
                    0,
                    Color32::from_rgb(c[0], c[1], c[2]),
                );
            }
            ui.label(format!(
                "{max:.0} {unit} {}",
                t.get(if difference { "gain" } else { "strong" })
            ));
            ui.label(format!(
                "● {}",
                t.get(if difference {
                    "measured_absolute"
                } else {
                    "measured"
                })
            ));
            ui.label(format!("○ {}", t.get("unmeasured")));
            if self.calculating {
                ui.label(t.get("working"));
            }
        });
        if !self.calculating
            && self
                .heat
                .as_ref()
                .is_none_or(|h| h.cells.iter().all(Option::is_none))
        {
            ui.label(RichText::new(t.get("no_data")).color(p.muted));
        }
        ui.label(RichText::new(t.get("estimate_note")).small().color(p.muted));
        ui.label(
            RichText::new(t.get(if difference {
                "compare_note"
            } else {
                "confidence_note"
            }))
            .small()
            .color(p.muted),
        );
        if difference && self.metric == Metric::LinkQualityPercent {
            ui.label(
                RichText::new(t.get("quality_delta_note"))
                    .small()
                    .color(p.muted),
            );
        }
    }
    fn settings_window(&mut self, ctx: &egui::Context, t: Text, p: Palette) {
        let before = self.prefs.clone();
        let mut open = self.settings;
        egui::Window::new(t.get("settings"))
            .id(egui::Id::new("settings"))
            .open(&mut open)
            .default_width(520.0)
            .min_width(360.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(580.0)
                    .show(ui, |ui| {
                        ui.heading(t.get("appearance"));
                        ui.horizontal_wrapped(|ui| {
                            ui.selectable_value(
                                &mut self.prefs.theme,
                                Theme::Light,
                                t.get("light"),
                            );
                            ui.selectable_value(&mut self.prefs.theme, Theme::Dark, t.get("dark"));
                        });
                        ui.label(t.get("language"));
                        egui::ComboBox::from_id_salt("language")
                            .selected_text(
                                LANGUAGES
                                    .iter()
                                    .find(|(id, _)| *id == self.prefs.language)
                                    .map(|(_, label)| *label)
                                    .unwrap_or("English"),
                            )
                            .show_ui(ui, |ui| {
                                for (id, label) in LANGUAGES {
                                    ui.selectable_value(&mut self.prefs.language, id.into(), label);
                                }
                            });
                        ui.checkbox(&mut self.prefs.reduced_motion, t.get("reduced_motion"));
                        ui.separator();
                        ui.heading(t.get("updates"));
                        ui.label(format!(
                            "{}: {}",
                            t.get("installed_version"),
                            updates::CURRENT_VERSION
                        ));
                        ui.checkbox(
                            &mut self.prefs.check_updates_on_startup,
                            t.get("startup_check"),
                        );
                        if ui
                            .add_enabled(
                                !matches!(
                                    self.update,
                                    UpdateState::Checking | UpdateState::Downloading(_)
                                ),
                                egui::Button::new(t.get("check_updates")),
                            )
                            .clicked()
                        {
                            self.check_updates(true);
                        }
                        match &self.update {
                            UpdateState::Idle => {}
                            UpdateState::Checking => {
                                ui.label(t.get("working"));
                            }
                            UpdateState::Current => {
                                ui.label(t.get("update_current"));
                            }
                            UpdateState::Available(u) => {
                                ui.label(
                                    RichText::new(format!(
                                        "{} · {}",
                                        t.get("update_available"),
                                        u.version
                                    ))
                                    .strong()
                                    .color(p.accent),
                                );
                                if ui.button(t.get("download_install")).clicked() {
                                    self.download_update();
                                }
                            }
                            UpdateState::Downloading(percent) => {
                                let percent = *percent;
                                ui.label(t.get("downloading"));
                                ui.add(
                                    egui::ProgressBar::new(percent as f32 / 100.0)
                                        .show_percentage(),
                                );
                                if ui.button(t.get("cancel")).clicked() {
                                    self.update_cancel.cancel();
                                }
                            }
                            UpdateState::Ready(_) => {
                                ui.label(t.get("update_ready"));
                                if ui
                                    .add_enabled(
                                        self.busy.is_none(),
                                        egui::Button::new(t.get("restart_install")),
                                    )
                                    .clicked()
                                {
                                    self.install_update();
                                }
                            }
                            UpdateState::Error(e) => {
                                ui.label(t.get(e.key));
                                if !e.detail.is_empty() {
                                    ui.collapsing(t.get("details"), |ui| {
                                        ui.label(&e.detail);
                                    });
                                }
                            }
                        }
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(t.get("update_privacy"))
                                .small()
                                .color(p.muted),
                        );
                        ui.separator();
                        ui.label("WifiMap · MIT · Windows 10/11 x64");
                        ui.collapsing(t.get("licenses"), |ui| {
                            ui.label(t.get("licenses_detail"));
                            if ui
                                .add_enabled(
                                    self.busy.is_none(),
                                    egui::Button::new(t.get("licenses_save")),
                                )
                                .clicked()
                            {
                                self.save_notices();
                            }
                        });
                    });
            });
        self.settings = open;
        if before != self.prefs {
            apply_style(ctx, self.prefs.theme, self.prefs.reduced_motion);
            if let Err(e) = self.prefs.save(&self.prefs_path) {
                self.status = Some(e);
            }
        }
    }
    fn canvas(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, t: Text, p: Palette) {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(vec2(100.0, 160.0)),
            egui::Sense::click_and_drag(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, t.get("coverage"))
        });
        let size = self.project.plan.size;
        if self.fit_pending || self.canvas_size != rect.size() {
            let zoom = ((rect.width() - 48.0) / size[0] as f32)
                .min((rect.height() - 48.0) / size[1] as f32)
                .max(0.02);
            self.transform.zoom = zoom;
            self.transform.origin = Point::new(
                (rect.width() - size[0] as f32 * zoom) / 2.0,
                (rect.height() - size[1] as f32 * zoom) / 2.0,
            );
            self.fit_pending = false;
            self.canvas_size = rect.size();
        }
        let local = |pos: egui::Pos2| Point::new(pos.x - rect.left(), pos.y - rect.top());
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| response.hover_pos());
        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0
                && let Some(pos) = pointer
            {
                self.transform.zoom_at(local(pos), (scroll * 0.002).exp());
            }
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            let delta = ui.input(|i| i.pointer.delta());
            self.transform.origin.x += delta.x;
            self.transform.origin.y += delta.y;
        }
        let project_point = pointer.map(|pos| self.transform.to_plan(local(pos)));
        if self.tool == Tool::Point {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(point) = project_point
            {
                let sample = self.project.surveys[self.survey]
                    .samples
                    .iter()
                    .filter(|s| Some(s.observation.bssid) == self.ap)
                    .min_by(|a, b| {
                        a.position
                            .distance(point)
                            .total_cmp(&b.position.distance(point))
                    })
                    .filter(|s| s.position.distance(point) * self.transform.zoom < 18.0)
                    .map(|s| (s.id, s.position));
                if let Some((id, old)) = sample {
                    self.drag = Some((id, old));
                    self.select(Some(id), old);
                    self.invalidate_heat();
                }
            }
            if response.dragged_by(egui::PointerButton::Primary)
                && let (Some((id, _)), Some(point)) = (self.drag, project_point)
            {
                let point = Point::new(
                    point.x.clamp(0.0, size[0] as f32),
                    point.y.clamp(0.0, size[1] as f32),
                );
                if let Some(sample) = self.project.surveys[self.survey]
                    .samples
                    .iter_mut()
                    .find(|s| s.id == id)
                {
                    sample.position = point;
                    self.point = Some(point);
                }
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some((id, old)) = self.drag.take()
            {
                self.undo = Some(Undo::Move(self.project.surveys[self.survey].id, id, old));
                self.mark_changed();
                self.invalidate_heat();
            }
        }
        if response.clicked_by(egui::PointerButton::Primary) {
            response.request_focus();
            if let Some(point) = project_point.filter(|p| p.inside(size)) {
                match self.tool {
                    Tool::Point => {
                        let selected = self.project.surveys[self.survey]
                            .samples
                            .iter()
                            .filter(|s| Some(s.observation.bssid) == self.ap)
                            .min_by(|a, b| {
                                a.position
                                    .distance(point)
                                    .total_cmp(&b.position.distance(point))
                            })
                            .filter(|s| s.position.distance(point) * self.transform.zoom < 16.0)
                            .map(|s| (s.id, s.position));
                        if let Some((id, p)) = selected {
                            self.select(Some(id), p);
                        } else {
                            self.select(None, point);
                        }
                    }
                    Tool::Router => {
                        let s = &mut self.project.surveys[self.survey];
                        self.undo = Some(Undo::Router(s.id, s.router));
                        s.router = Some(point);
                        self.mark_changed();
                    }
                    Tool::Wall => {
                        if let Some(a) = self.anchor.take() {
                            if a.distance(point) > 1.0 && self.project.plan.walls.len() < 10_000 {
                                self.project.plan.walls.push(Wall { a, b: point });
                                self.undo = Some(Undo::Wall);
                                self.mark_changed();
                            }
                        } else {
                            self.anchor = Some(point);
                        }
                    }
                    Tool::Scale => {
                        if let Some(a) = self.anchor.take() {
                            if a.distance(point) >= 1.0 {
                                self.calibration = Some((a, point));
                            }
                        } else {
                            self.anchor = Some(point);
                        }
                    }
                }
            }
        }
        if response.has_focus() && ui.is_enabled() && !self.confirming {
            let delta = ui.input(|i| {
                vec2(
                    f32::from(i.key_pressed(egui::Key::ArrowRight))
                        - f32::from(i.key_pressed(egui::Key::ArrowLeft)),
                    f32::from(i.key_pressed(egui::Key::ArrowDown))
                        - f32::from(i.key_pressed(egui::Key::ArrowUp)),
                )
            });
            if delta != Vec2::ZERO {
                let point = self
                    .point
                    .unwrap_or(Point::new(size[0] as f32 / 2.0, size[1] as f32 / 2.0));
                self.select(
                    None,
                    Point::new(
                        (point.x + delta.x * 10.0).clamp(0.0, size[0] as f32),
                        (point.y + delta.y * 10.0).clamp(0.0, size[1] as f32),
                    ),
                );
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter))
                && self.busy.is_none()
                && self.point.is_some()
            {
                self.scan(true);
            }
        }
        let painter = ui.painter_at(rect);
        let screen = |point: Point| {
            let p = self.transform.to_screen(point);
            pos2(rect.left() + p.x, rect.top() + p.y)
        };
        let plan_rect = egui::Rect::from_min_max(
            screen(Point::default()),
            screen(Point::new(size[0] as f32, size[1] as f32)),
        );
        painter.rect_filled(plan_rect, 6, p.surface);
        painter.rect_stroke(
            plan_rect,
            6,
            Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
        let uv = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        if let Some(texture) = &self.plan_texture {
            painter.image(texture.id(), plan_rect, uv, Color32::WHITE);
        } else {
            let step = 50.0 * self.transform.zoom;
            if step > 15.0 {
                let x0 = ((rect.left() - plan_rect.left()) / step).floor().max(0.0) as i32;
                let y0 = ((rect.top() - plan_rect.top()) / step).floor().max(0.0) as i32;
                let columns = (size[0] as f32 / 50.0) as i32;
                let rows = (size[1] as f32 / 50.0) as i32;
                for x in x0..=columns.min(x0 + (rect.width() / step) as i32 + 1) {
                    for y in y0..=rows.min(y0 + (rect.height() / step) as i32 + 1) {
                        painter.circle_filled(
                            pos2(
                                plan_rect.left() + x as f32 * step,
                                plan_rect.top() + y as f32 * step,
                            ),
                            1.0,
                            p.border,
                        );
                    }
                }
            }
        }
        if let Some(texture) = &self.heat_texture {
            painter.image(texture.id(), plan_rect, uv, Color32::WHITE);
        }
        for wall in &self.project.plan.walls {
            painter.line_segment(
                [screen(wall.a), screen(wall.b)],
                Stroke::new((6.0 * self.transform.zoom).clamp(2.0, 9.0), p.text),
            );
        }
        if let Some(router) = self.project.surveys[self.survey].router {
            let at = screen(router);
            painter.rect_filled(
                egui::Rect::from_center_size(at, vec2(22.0, 22.0)),
                5,
                p.accent,
            );
            painter.text(
                at,
                Align2::CENTER_CENTER,
                "R",
                FontId::proportional(14.0),
                Color32::WHITE,
            );
            painter.text(
                at + vec2(16.0, 0.0),
                Align2::LEFT_CENTER,
                t.get("router_tool"),
                FontId::proportional(13.0),
                p.text,
            );
        }
        for (index, sample) in self.project.surveys[self.survey]
            .samples
            .iter()
            .enumerate()
            .filter(|(_, s)| Some(s.observation.bssid) == self.ap)
        {
            let at = screen(sample.position);
            let c = sample
                .observation
                .value(self.metric)
                .map(|v| heatmap::color(v, self.metric, false))
                .unwrap_or([125, 140, 145]);
            let selected = self.selected == Some(sample.id);
            painter.circle_filled(
                at,
                if selected { 8.5 } else { 6.5 },
                Color32::from_rgb(c[0], c[1], c[2]),
            );
            painter.circle_stroke(
                at,
                if selected { 8.5 } else { 6.5 },
                Stroke::new(2.5, Color32::WHITE),
            );
            if selected {
                painter.circle_stroke(at, 12.0, Stroke::new(1.5, p.accent));
            }
            painter.text(
                at + vec2(11.0, -11.0),
                Align2::LEFT_BOTTOM,
                format!("{}", index + 1),
                FontId::proportional(13.0),
                p.text,
            );
        }
        if let Some(point) = self.point {
            let at = screen(point);
            let elapsed = self.selected_at.elapsed();
            let duration = Duration::from_millis(180);
            let animated = !self.prefs.reduced_motion && elapsed < duration;
            let progress = if animated {
                elapsed.as_secs_f32() / duration.as_secs_f32()
            } else {
                1.0
            };
            let radius = 13.0 + 5.0 * (1.0 - progress).powi(3);
            painter.circle_stroke(at, radius, Stroke::new(1.5, p.accent));
            if self.selected.is_none() {
                painter.line_segment(
                    [at - vec2(6.0, 0.0), at + vec2(6.0, 0.0)],
                    Stroke::new(1.5, p.accent),
                );
                painter.line_segment(
                    [at - vec2(0.0, 6.0), at + vec2(0.0, 6.0)],
                    Stroke::new(1.5, p.accent),
                );
            }
            if animated {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
        if let Some(a) = self.anchor {
            painter.circle_filled(screen(a), 4.0, p.accent);
            if let Some(b) = project_point {
                painter.line_segment([screen(a), screen(b)], Stroke::new(2.0, p.accent));
            }
        }
        if let Some((a, b)) = self.calibration {
            painter.line_segment([screen(a), screen(b)], Stroke::new(2.0, p.accent));
        }
        if let Some(cal) = &self.project.plan.calibration
            && let Some(mpp) = cal.meters_per_pixel()
        {
            let length = 1.0 / mpp * self.transform.zoom;
            if length > 20.0 && length < rect.width() * 0.7 {
                let start = pos2(rect.left() + 16.0, rect.bottom() - 22.0);
                painter.line_segment(
                    [start, start + vec2(length, 0.0)],
                    Stroke::new(3.0, p.accent),
                );
                painter.text(
                    start + vec2(0.0, -6.0),
                    Align2::LEFT_BOTTOM,
                    "1 m",
                    FontId::proportional(13.0),
                    p.text,
                );
            }
        }
    }
}
