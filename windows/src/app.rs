use crate::workers::*;
use std::{path::PathBuf, sync::mpsc, time::Instant};
use wifi_map::{
    error::AppError,
    heatmap::{Cancellation, Heatmap},
    i18n::Text,
    model::*,
    persistence,
    preferences::Preferences,
    updates::{self, AvailableUpdate, CheckResult},
    wlan::{Adapter, WindowsWlan, WirelessAdapter},
};

#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Point,
    Wall,
    Scale,
    Router,
}
#[derive(Clone, Copy)]
pub enum PendingAction {
    New,
    Demo,
    Open,
    Import,
    Close,
}
pub enum Undo {
    Move(u64, u64, Point),
    Remove(u64, usize, Sample),
    Wall,
    Router(u64, Option<Point>),
}
pub enum UpdateState {
    Idle,
    Checking,
    Current,
    Available(AvailableUpdate),
    Downloading(u8),
    Ready(updates::VerifiedDownload),
    Error(AppError),
}
pub struct WifiApp {
    pub context: egui::Context,
    pub project: Project,
    pub path: Option<PathBuf>,
    pub revision: u64,
    pub dirty: bool,
    pub survey: usize,
    pub baseline: usize,
    pub compare: bool,
    pub ap: Option<Bssid>,
    pub metric: Metric,
    pub radius: f32,
    pub point: Option<Point>,
    pub selected: Option<u64>,
    pub tool: Tool,
    pub anchor: Option<Point>,
    pub calibration: Option<(Point, Point)>,
    pub scale_m: f32,
    pub transform: ViewTransform,
    pub fit_pending: bool,
    pub canvas_size: egui::Vec2,
    pub drag: Option<(u64, Point)>,
    pub undo: Option<Undo>,
    pub plan_texture: Option<egui::TextureHandle>,
    pub heat_texture: Option<egui::TextureHandle>,
    pub heat: Option<Heatmap>,
    pub heat_generation: u64,
    pub heat_dirty: bool,
    pub calculating: bool,
    pub adapters: Vec<Adapter>,
    pub adapter: usize,
    pub access_points: Vec<Observation>,
    pub busy: Option<&'static str>,
    pub cancel: Cancellation,
    pub status: Option<AppError>,
    pub notice: Option<&'static str>,
    pub prefs: Preferences,
    pub prefs_path: PathBuf,
    pub settings: bool,
    pub pending: Option<PendingAction>,
    pub confirming: bool,
    pub exit: bool,
    pub update: UpdateState,
    pub update_cancel: Cancellation,
    pub startup_check: bool,
    pub peer_address: String,
    pub peer_token: String,
    pub selected_at: Instant,
    pub receiver: mpsc::Receiver<UiEvent>,
    pub sink: EventSink,
    pub worker: HeatWorker,
}
impl WifiApp {
    pub fn new(
        context: egui::Context,
        receiver: mpsc::Receiver<UiEvent>,
        sink: EventSink,
        initial: Option<PathBuf>,
    ) -> Self {
        let prefs_path = persistence::preferences_path();
        let loaded = Preferences::load(&prefs_path);
        let (prefs, status) = match loaded {
            Ok(p) => (p, None),
            Err(e) => (
                Preferences {
                    check_updates_on_startup: false,
                    ..Preferences::default()
                },
                Some(e),
            ),
        };
        Self::with_preferences(context, receiver, sink, initial, prefs, prefs_path, status)
    }
    fn with_preferences(
        context: egui::Context,
        receiver: mpsc::Receiver<UiEvent>,
        sink: EventSink,
        initial: Option<PathBuf>,
        prefs: Preferences,
        prefs_path: PathBuf,
        status: Option<AppError>,
    ) -> Self {
        let worker = HeatWorker::new(sink.clone());
        let mut app = Self {
            context,
            project: Project::default(),
            path: None,
            revision: 0,
            dirty: false,
            survey: 0,
            baseline: 0,
            compare: false,
            ap: None,
            metric: Metric::RssiDbm,
            radius: 3.0,
            point: None,
            selected: None,
            tool: Tool::Point,
            anchor: None,
            calibration: None,
            scale_m: 3.0,
            transform: ViewTransform {
                origin: Point::default(),
                zoom: 1.0,
            },
            fit_pending: true,
            canvas_size: egui::Vec2::ZERO,
            drag: None,
            undo: None,
            plan_texture: None,
            heat_texture: None,
            heat: None,
            heat_generation: 0,
            heat_dirty: false,
            calculating: false,
            adapters: Vec::new(),
            adapter: 0,
            access_points: Vec::new(),
            busy: None,
            cancel: Cancellation::default(),
            status,
            notice: None,
            prefs,
            prefs_path,
            settings: false,
            pending: None,
            confirming: false,
            exit: false,
            update: UpdateState::Idle,
            update_cancel: Cancellation::default(),
            startup_check: false,
            peer_address: String::new(),
            peer_token: String::new(),
            selected_at: Instant::now(),
            receiver,
            sink,
            worker,
        };
        if let Some(path) = initial {
            app.open_path(Some(path));
        }
        app
    }
    pub fn text(&self) -> Text {
        Text::new(&self.prefs.language)
    }
    pub fn mark_changed(&mut self) {
        self.revision += 1;
        self.dirty = true;
        self.notice = None;
        if !self.confirming {
            self.pending = None;
        }
    }
    pub fn invalidate_heat(&mut self) {
        self.worker.cancel();
        self.heat_generation += 1;
        self.heat = None;
        self.heat_texture = None;
        self.calculating = false;
        self.heat_dirty = true;
    }
    pub fn current_baseline(&self) -> Option<usize> {
        (self.compare && self.baseline != self.survey).then_some(self.baseline)
    }
    pub fn selected_sample(&self) -> Option<&Sample> {
        self.selected.and_then(|id| {
            self.project.surveys[self.survey]
                .samples
                .iter()
                .find(|s| s.id == id)
        })
    }
    pub fn select(&mut self, id: Option<u64>, p: Point) {
        self.selected = id;
        self.point = Some(p);
        self.selected_at = Instant::now();
    }
    fn spawn(
        &mut self,
        label: &'static str,
        job: impl FnOnce(EventSink, Cancellation) + Send + 'static,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(label);
        self.status = None;
        self.notice = None;
        self.cancel = Cancellation::default();
        let sink = self.sink.clone();
        let cancel = self.cancel.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("survey-io".into())
            .spawn(move || job(sink, cancel))
        {
            self.busy = None;
            self.status = Some(AppError::detail("file_error", e));
        }
    }
    pub fn request_action(&mut self, action: PendingAction) {
        if self.busy.is_some() {
            if matches!(action, PendingAction::Close) {
                self.pending = Some(action);
                self.cancel.cancel();
                // A synchronous driver call can outlive the polling budget.
                // Closing must remain possible without terminating that thread.
                if matches!(self.busy, Some("scanning" | "local_test")) {
                    if self.dirty {
                        self.confirming = true;
                    } else {
                        self.exit = true;
                    }
                }
            }
            return;
        }
        if self.dirty {
            self.pending = Some(action);
            self.confirming = true;
        } else {
            self.perform_action(action);
        }
    }
    pub fn perform_action(&mut self, action: PendingAction) {
        self.pending = None;
        self.confirming = false;
        match action {
            PendingAction::Close => self.exit = true,
            PendingAction::New => self.set_project(Project::default(), None, None),
            PendingAction::Demo => {
                let mut p = demo_project();
                let t = self.text();
                p.name = t.get("demo").into();
                p.surveys[0].name = t.get("before").into();
                p.surveys[1].name = t.get("after").into();
                self.set_project(p, None, None);
            }
            PendingAction::Open => self.open_path(None),
            PendingAction::Import => self.import_plan(),
        }
    }
    pub fn set_project(
        &mut self,
        p: Project,
        path: Option<PathBuf>,
        image: Option<egui::ColorImage>,
    ) {
        self.worker.cancel();
        self.project = p;
        self.path = path;
        self.revision += 1;
        self.dirty = false;
        self.survey = 0;
        self.baseline = 1.min(self.project.surveys.len() - 1);
        self.compare = false;
        self.ap = self.project.access_points(0).first().map(|(b, _)| *b);
        self.selected = None;
        self.point = None;
        self.anchor = None;
        self.calibration = None;
        self.undo = None;
        self.fit_pending = true;
        self.plan_texture = image.map(|img| {
            self.context
                .load_texture("floor-plan", img, egui::TextureOptions::LINEAR)
        });
        self.invalidate_heat();
        self.status = None;
        self.notice = None;
    }
    pub fn open_path(&mut self, path: Option<PathBuf>) {
        let t = self.text();
        self.spawn("working", move |sink, _| {
            let result = (|| {
                let Some(path) = path.or_else(|| {
                    rfd::FileDialog::new()
                        .set_title(t.get("open"))
                        .add_filter(t.get("project_filter"), &["wifimap"])
                        .pick_file()
                }) else {
                    return Ok(None);
                };
                let p = persistence::load(&path)?;
                let image = decoded_plan(&p.plan)?;
                Ok(Some((path, p, image)))
            })();
            sink.send(UiEvent::Opened(result));
        });
    }
    pub fn import_plan(&mut self) {
        let t = self.text();
        self.spawn("working", move |sink, _| {
            let result = (|| {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(t.get("import"))
                    .add_filter(t.get("image_filter"), &["png", "jpg", "jpeg"])
                    .pick_file()
                else {
                    return Ok(None);
                };
                let plan = persistence::import_plan(&path)?;
                let image = decoded_plan(&plan)?.ok_or_else(|| AppError::new("image_error"))?;
                Ok(Some((plan, image)))
            })();
            sink.send(UiEvent::Imported(result));
        });
    }
    pub fn save(&mut self, save_as: bool) {
        let p = self.project.clone();
        let path = if save_as { None } else { self.path.clone() };
        let revision = self.revision;
        let t = self.text();
        self.spawn("working", move |sink, _| {
            let result = (|| {
                let Some(path) = path.or_else(|| {
                    rfd::FileDialog::new()
                        .set_title(t.get("save_as"))
                        .set_file_name("Apartment.wifimap")
                        .add_filter(t.get("project_filter"), &["wifimap"])
                        .save_file()
                }) else {
                    return Ok(None);
                };
                persistence::save(&path, &p)?;
                Ok(Some((path, revision)))
            })();
            sink.send(UiEvent::Saved(result));
        });
    }
    pub fn export(&mut self) {
        let Some(ap) = self.ap else { return };
        let p = self.project.clone();
        let s = self.survey;
        let b = self.current_baseline();
        let metric = self.metric;
        let map = self.heat.clone();
        let t = self.text();
        self.spawn("working", move |sink, _| {
            let result = (|| {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(t.get("export"))
                    .set_file_name("WifiMap-report.svg")
                    .add_filter(t.get("report_filter"), &["svg"])
                    .save_file()
                else {
                    return Ok(None);
                };
                let svg = wifi_map::export::svg(&p, s, b, ap, metric, map.as_ref(), t)?;
                persistence::atomic_write(&path, svg.as_bytes())?;
                Ok(Some(()))
            })();
            sink.send(UiEvent::Exported(result, "exported"));
        });
    }
    pub fn save_notices(&mut self) {
        let t = self.text();
        self.spawn("working", move |sink, _| {
            let result = (|| {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(t.get("licenses"))
                    .set_file_name("WifiMap-license-notices.txt")
                    .add_filter(t.get("licenses"), &["txt"])
                    .save_file()
                else {
                    return Ok(None);
                };
                let content = concat!(
                    include_str!("../LICENSE"),
                    "\n\n",
                    include_str!("../THIRD_PARTY_NOTICES.txt")
                );
                persistence::atomic_write(&path, content.as_bytes())?;
                Ok(Some(()))
            })();
            sink.send(UiEvent::Exported(result, "licenses_saved"));
        });
    }
    pub fn find_adapters(&mut self) {
        if self.project.demo {
            self.status = Some(AppError::new("demo_isolated"));
            return;
        }
        self.spawn("working", |sink, _| {
            sink.send(UiEvent::Adapters(WindowsWlan.adapters()))
        });
    }
    pub fn scan(&mut self, measure: bool) {
        if self.project.demo {
            self.status = Some(AppError::new("demo_isolated"));
            return;
        }
        let Some(adapter) = self.adapters.get(self.adapter).map(|a| a.id.clone()) else {
            return;
        };
        let target = if measure {
            let Some(position) = self.point else {
                return;
            };
            Some(MeasurementTarget {
                survey_id: self.project.surveys[self.survey].id,
                position,
            })
        } else {
            None
        };
        let ap = if measure { self.ap } else { None };
        if measure && ap.is_none() {
            return;
        }
        self.spawn("scanning", move |sink, cancel| {
            sink.send(UiEvent::Scan(
                WindowsWlan.scan(&adapter, ap, &cancel),
                target,
            ))
        });
    }
    pub fn run_probe(&mut self) {
        if self.project.demo {
            self.status = Some(AppError::new("demo_isolated"));
            return;
        }
        let Some(sample) = self.selected_sample() else {
            return;
        };
        let sample_id = sample.id;
        let survey_id = self.project.surveys[self.survey].id;
        let config = wifi_map::peer::local_address(&self.peer_address).and_then(|address| {
            wifi_map::peer::parse_token(&self.peer_token).map(|token| (address, token))
        });
        match config {
            Err(e) => self.status = Some(e),
            Ok((address, token)) => self.spawn("local_test", move |sink, cancel| {
                sink.send(UiEvent::Probed(
                    wifi_map::peer::probe(address, token, &cancel),
                    survey_id,
                    sample_id,
                ))
            }),
        }
    }
    pub fn check_updates(&mut self, manual: bool) {
        updates::dispatch_check(manual, self.prefs.check_updates_on_startup, || {
            self.start_update_check(manual)
        });
    }
    fn start_update_check(&mut self, manual: bool) {
        if matches!(
            self.update,
            UpdateState::Checking | UpdateState::Downloading(_)
        ) {
            return;
        }
        self.update = UpdateState::Checking;
        let sink = self.sink.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || sink.send(UiEvent::Checked(updates::check(), manual)))
        {
            self.update = UpdateState::Error(AppError::detail("update_error", e));
        }
    }
    pub fn download_update(&mut self) {
        let UpdateState::Available(update) = &self.update else {
            return;
        };
        let update = update.clone();
        self.update = UpdateState::Downloading(0);
        self.update_cancel = Cancellation::default();
        let sink = self.sink.clone();
        let cancel = self.update_cancel.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("update-download".into())
            .spawn(move || {
                let result =
                    updates::download(&update, &cancel, |p| sink.send(UiEvent::Progress(p)));
                sink.send(UiEvent::Downloaded(result));
            })
        {
            self.update = UpdateState::Error(AppError::detail("update_error", e));
        }
    }
    pub fn install_update(&mut self) {
        if self.dirty {
            self.status = Some(AppError::new("save_first"));
            return;
        }
        let UpdateState::Ready(staged) = &self.update else {
            return;
        };
        let staged = staged.clone();
        let reopen = self.path.clone();
        self.spawn("working", move |sink, _| {
            sink.send(UiEvent::InstallPrepared(updates::prepare_install(
                &staged, reopen,
            )))
        });
    }
    pub fn undo_edit(&mut self) {
        let Some(edit) = self.undo.take() else {
            return;
        };
        match edit {
            Undo::Move(survey, id, p) => {
                if let Some(s) = self
                    .project
                    .surveys
                    .iter_mut()
                    .find(|s| s.id == survey)
                    .and_then(|s| s.samples.iter_mut().find(|s| s.id == id))
                {
                    s.position = p;
                    self.point = Some(p);
                }
            }
            Undo::Remove(survey, index, sample) => {
                if let Some(s) = self.project.surveys.iter_mut().find(|s| s.id == survey) {
                    s.samples.insert(index.min(s.samples.len()), sample);
                }
            }
            Undo::Wall => {
                self.project.plan.walls.pop();
            }
            Undo::Router(id, p) => {
                if let Some(s) = self.project.surveys.iter_mut().find(|s| s.id == id) {
                    s.router = p;
                }
            }
        }
        self.mark_changed();
        self.invalidate_heat();
    }
    pub fn remove_point(&mut self) {
        if let Some(id) = self.selected {
            let survey = &mut self.project.surveys[self.survey];
            if let Some(index) = survey.samples.iter().position(|s| s.id == id) {
                let sample = survey.samples.remove(index);
                self.undo = Some(Undo::Remove(survey.id, index, sample));
                self.selected = None;
                self.mark_changed();
                self.invalidate_heat();
            }
        }
    }
    pub fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                UiEvent::Heat(generation, map) => {
                    if generation == self.heat_generation {
                        self.heat_texture = Some(ctx.load_texture(
                            "coverage",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [map.width, map.height],
                                &map.rgba(),
                            ),
                            egui::TextureOptions::LINEAR,
                        ));
                        self.heat = Some(map);
                        self.calculating = false;
                    }
                }
                UiEvent::Checked(result, manual) => {
                    self.update = match result {
                        Ok(CheckResult::Current) => {
                            if manual {
                                UpdateState::Current
                            } else {
                                UpdateState::Idle
                            }
                        }
                        Ok(CheckResult::Available(u)) => UpdateState::Available(u),
                        Err(e) => {
                            if manual {
                                UpdateState::Error(e)
                            } else {
                                UpdateState::Idle
                            }
                        }
                    };
                }
                UiEvent::Progress(p) => self.update = UpdateState::Downloading(p),
                UiEvent::Downloaded(result) => {
                    self.update = match result {
                        Ok(path) => UpdateState::Ready(path),
                        Err(e) => UpdateState::Error(e),
                    }
                }
                other => {
                    self.busy = None;
                    let cancelled = self.cancel.is_cancelled();
                    match other {
                        UiEvent::Adapters(result) => match result {
                            Ok(list) => {
                                self.adapters = list;
                                self.adapter = 0;
                                self.access_points.clear();
                                if self.adapters.is_empty() {
                                    self.status = Some(AppError::new("no_adapters"));
                                } else if !cancelled {
                                    self.scan(false);
                                }
                            }
                            Err(e) => self.status = Some(e),
                        },
                        UiEvent::Scan(result, target) => {
                            if !cancelled {
                                match result {
                                    Ok(snapshot) => {
                                        if let Some(target) = target {
                                            if let (Some(index), Some(observation)) = (
                                                self.project
                                                    .surveys
                                                    .iter()
                                                    .position(|s| s.id == target.survey_id),
                                                snapshot.observations.first(),
                                            ) {
                                                match self.project.add_sample(
                                                    index,
                                                    target.position,
                                                    observation.clone(),
                                                ) {
                                                    Ok(id) => {
                                                        if index == self.survey {
                                                            self.ap = Some(observation.bssid);
                                                            self.select(Some(id), target.position);
                                                        }
                                                        self.mark_changed();
                                                        self.invalidate_heat();
                                                    }
                                                    Err(key) => {
                                                        self.status = Some(AppError::new(key))
                                                    }
                                                }
                                            }
                                        } else {
                                            if self.ap.is_none_or(|ap| {
                                                !snapshot.observations.iter().any(|o| o.bssid == ap)
                                            }) {
                                                self.ap = snapshot
                                                    .observations
                                                    .iter()
                                                    .find(|o| o.connected_bssid == Some(o.bssid))
                                                    .or(snapshot.observations.first())
                                                    .map(|o| o.bssid);
                                                self.invalidate_heat();
                                            }
                                            self.access_points = snapshot.observations;
                                        }
                                    }
                                    Err(e) => self.status = Some(e),
                                }
                            }
                        }
                        UiEvent::Opened(result) => match result {
                            Ok(Some((path, p, image))) => self.set_project(p, Some(path), image),
                            Ok(None) => {}
                            Err(e) => self.status = Some(e),
                        },
                        UiEvent::Imported(result) => match result {
                            Ok(Some((plan, image))) => {
                                let p = Project {
                                    plan,
                                    ..Project::default()
                                };
                                self.set_project(p, None, Some(image));
                                self.mark_changed();
                            }
                            Ok(None) => {}
                            Err(e) => self.status = Some(e),
                        },
                        UiEvent::Saved(result) => match result {
                            Ok(Some((path, revision))) => {
                                self.path = Some(path);
                                if revision == self.revision {
                                    self.dirty = false;
                                    self.notice = Some("saved");
                                    if let Some(action) = self.pending.take() {
                                        self.perform_action(action);
                                    }
                                }
                            }
                            Ok(None) => {
                                self.pending = None;
                            }
                            Err(e) => {
                                self.pending = None;
                                self.status = Some(e);
                            }
                        },
                        UiEvent::Exported(result, label) => match result {
                            Ok(Some(())) => self.notice = Some(label),
                            Ok(None) => {}
                            Err(e) => self.status = Some(e),
                        },
                        UiEvent::Probed(result, survey_id, sample_id) => {
                            if !cancelled {
                                match result {
                                    Ok(probe) => {
                                        if let Some(sample) = self
                                            .project
                                            .surveys
                                            .iter_mut()
                                            .find(|s| s.id == survey_id)
                                            .and_then(|s| {
                                                s.samples.iter_mut().find(|s| s.id == sample_id)
                                            })
                                        {
                                            sample.local_probe = Some(probe);
                                            self.mark_changed();
                                        }
                                    }
                                    Err(e) => self.status = Some(e),
                                }
                            }
                        }
                        UiEvent::InstallPrepared(result) => match result {
                            Ok(prepared) => {
                                if self.dirty {
                                    self.status = Some(AppError::new("save_first"));
                                } else {
                                    match updates::start_install(&prepared) {
                                        Ok(()) => self.exit = true,
                                        Err(e) => self.status = Some(e),
                                    }
                                }
                            }
                            Err(e) => self.status = Some(e),
                        },
                        _ => {}
                    }
                }
            }
        }
        if self.busy.is_none()
            && !self.confirming
            && let Some(action) = self.pending.take()
        {
            self.request_action(action);
        }
    }
    pub fn queue_heat(&mut self) {
        if !self.heat_dirty {
            return;
        }
        self.heat_dirty = false;
        if let Some(ap) = self.ap {
            self.calculating = true;
            self.worker.submit(HeatRequest {
                generation: self.heat_generation,
                project: heat_snapshot(&self.project),
                survey: self.survey,
                baseline: self.current_baseline(),
                ap,
                metric: self.metric,
                radius: self.radius,
                cancel: Cancellation::default(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> WifiApp {
        let (tx, rx) = mpsc::channel();
        let sink = EventSink::new(tx, || {});
        WifiApp::with_preferences(
            egui::Context::default(),
            rx,
            sink,
            None,
            Preferences {
                check_updates_on_startup: false,
                ..Preferences::default()
            },
            PathBuf::from("unused-fixture-preferences.json"),
            None,
        )
    }
    fn observation() -> Observation {
        let mut o = demo_project().surveys[0].samples[0].observation.clone();
        o.source = ObservationSource::WindowsBss;
        o
    }
    #[test]
    fn measurement_result_keeps_requested_location_identity_and_timestamp() {
        let mut app = app();
        let position = Point::new(125.0, 203.0);
        let o = observation();
        let timestamp = o.observed_unix_ms;
        app.busy = Some("scanning");
        app.sink.send(UiEvent::Scan(
            Ok(wifi_map::wlan::ScanSnapshot {
                observations: vec![o],
                discarded_stale: 0,
            }),
            Some(MeasurementTarget {
                survey_id: 1,
                position,
            }),
        ));
        app.point = Some(Point::new(300.0, 300.0));
        app.drain_events(&app.context.clone());
        let sample = &app.project.surveys[0].samples[0];
        assert_eq!(sample.position, position);
        assert_eq!(sample.observation.observed_unix_ms, timestamp);
        assert_eq!(sample.id, 2);
        assert!(app.dirty);
    }
    #[test]
    fn cancelled_measurement_and_failed_adapter_do_not_add_samples() {
        let mut app = app();
        app.cancel.cancel();
        app.busy = Some("scanning");
        app.sink.send(UiEvent::Scan(
            Ok(wifi_map::wlan::ScanSnapshot {
                observations: vec![observation()],
                discarded_stale: 0,
            }),
            Some(MeasurementTarget {
                survey_id: 1,
                position: Point::new(50.0, 50.0),
            }),
        ));
        app.drain_events(&app.context.clone());
        assert!(app.project.surveys[0].samples.is_empty());
        assert!(!app.dirty);
        app.cancel = Cancellation::default();
        app.sink
            .send(UiEvent::Scan(Err(AppError::new("wlan_permission")), None));
        app.drain_events(&app.context.clone());
        assert_eq!(app.status.as_ref().unwrap().key, "wlan_permission");
        assert!(app.project.surveys[0].samples.is_empty());
    }
    #[test]
    fn stale_heat_result_and_save_result_cannot_overwrite_new_edits() {
        let mut app = app();
        let p = demo_project();
        let ap = p.surveys[0].samples[0].observation.bssid;
        let map = wifi_map::heatmap::compute(
            &p,
            0,
            None,
            ap,
            Metric::RssiDbm,
            3.0,
            &Cancellation::default(),
        )
        .unwrap();
        app.heat_generation = 2;
        app.sink.send(UiEvent::Heat(1, map));
        app.drain_events(&app.context.clone());
        assert!(app.heat.is_none());
        app.pending = Some(PendingAction::Close);
        app.mark_changed();
        app.sink.send(UiEvent::Saved(Ok(Some((
            PathBuf::from("fixture.wifimap"),
            0,
        )))));
        app.drain_events(&app.context.clone());
        assert!(app.dirty);
        assert!(!app.exit);
    }
    #[test]
    fn remove_and_undo_preserves_the_original_measurement() {
        let mut app = app();
        app.set_project(demo_project(), None, None);
        let sample = app.project.surveys[0].samples[0].clone();
        app.select(Some(sample.id), sample.position);
        app.remove_point();
        assert_eq!(app.project.surveys[0].samples.len(), 7);
        app.undo_edit();
        let restored = &app.project.surveys[0].samples[0];
        assert_eq!(restored.id, sample.id);
        assert_eq!(
            restored.observation.observed_unix_ms,
            sample.observation.observed_unix_ms
        );
        assert_eq!(restored.position, sample.position);
    }
    #[test]
    fn driver_job_cannot_trap_close_or_discard_new_edits_during_update_preparation() {
        let mut clean = app();
        clean.busy = Some("scanning");
        clean.request_action(PendingAction::Close);
        assert!(clean.exit);
        assert!(clean.cancel.is_cancelled());
        let mut dirty = app();
        dirty.dirty = true;
        dirty.busy = Some("scanning");
        dirty.request_action(PendingAction::Close);
        assert!(!dirty.exit);
        assert!(dirty.confirming);
        dirty.pending = None;
        dirty.confirming = false;
        dirty
            .sink
            .send(UiEvent::InstallPrepared(Ok(updates::PreparedInstall {
                helper: PathBuf::from("never-launch-fixture.exe"),
                job_path: PathBuf::from("never-read-fixture.json"),
            })));
        dirty.drain_events(&dirty.context.clone());
        assert!(!dirty.exit);
        assert_eq!(dirty.status.as_ref().unwrap().key, "save_first");
    }
}
