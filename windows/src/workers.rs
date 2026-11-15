use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, mpsc},
};
use wifi_map::{
    error::Result,
    heatmap::{self, Cancellation, Heatmap},
    model::*,
    peer::ProbeResult,
    updates,
    wlan::{Adapter, ScanSnapshot},
};

pub enum UiEvent {
    Adapters(Result<Vec<Adapter>>),
    Scan(Result<ScanSnapshot>, Option<MeasurementTarget>),
    Opened(Result<Option<(PathBuf, Project, Option<egui::ColorImage>)>>),
    Imported(Result<Option<(FloorPlan, egui::ColorImage)>>),
    Saved(Result<Option<(PathBuf, u64)>>),
    Exported(Result<Option<()>>, &'static str),
    Probed(Result<ProbeResult>, u64, u64),
    Heat(u64, Heatmap),
    Checked(Result<updates::CheckResult>, bool),
    Progress(u8),
    Downloaded(Result<updates::VerifiedDownload>),
    InstallPrepared(Result<updates::PreparedInstall>),
}
#[derive(Clone)]
pub struct MeasurementTarget {
    pub survey_id: u64,
    pub position: Point,
}
#[derive(Clone)]
pub struct EventSink {
    tx: mpsc::Sender<UiEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl EventSink {
    pub fn new(tx: mpsc::Sender<UiEvent>, wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            tx,
            wake: Arc::new(wake),
        }
    }
    pub fn send(&self, event: UiEvent) {
        if self.tx.send(event).is_ok() {
            (self.wake)();
        }
    }
}
pub struct HeatRequest {
    pub generation: u64,
    pub project: Project,
    pub survey: usize,
    pub baseline: Option<usize>,
    pub ap: Bssid,
    pub metric: Metric,
    pub radius: f32,
    pub cancel: Cancellation,
}
struct Queue {
    pending: Option<HeatRequest>,
    stop: bool,
}
pub struct HeatWorker {
    shared: Arc<(Mutex<Queue>, Condvar)>,
    current: Cancellation,
}
impl HeatWorker {
    pub fn new(sink: EventSink) -> Self {
        let shared = Arc::new((
            Mutex::new(Queue {
                pending: None,
                stop: false,
            }),
            Condvar::new(),
        ));
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("heatmap".into())
            .spawn(move || {
                loop {
                    let request = {
                        let (lock, ready) = &*worker;
                        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
                        while queue.pending.is_none() && !queue.stop {
                            queue = ready.wait(queue).unwrap_or_else(|e| e.into_inner());
                        }
                        if queue.stop {
                            break;
                        }
                        queue.pending.take().unwrap()
                    };
                    if let Some(map) = heatmap::compute(
                        &request.project,
                        request.survey,
                        request.baseline,
                        request.ap,
                        request.metric,
                        request.radius,
                        &request.cancel,
                    ) && !request.cancel.is_cancelled()
                    {
                        sink.send(UiEvent::Heat(request.generation, map));
                    }
                }
            })
            .expect("create bounded heatmap worker");
        Self {
            shared,
            current: Cancellation::default(),
        }
    }
    pub fn cancel(&mut self) {
        self.current.cancel();
        let mut queue = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(request) = queue.pending.take() {
            request.cancel.cancel();
        }
    }
    pub fn submit(&mut self, request: HeatRequest) {
        self.cancel();
        self.current = request.cancel.clone();
        self.shared
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending = Some(request);
        self.shared.1.notify_one();
    }
}
impl Drop for HeatWorker {
    fn drop(&mut self) {
        self.cancel();
        self.shared.0.lock().unwrap_or_else(|e| e.into_inner()).stop = true;
        self.shared.1.notify_one();
    }
}

/// Strip image bytes and drawing-only walls from the CPU computation snapshot.
pub fn heat_snapshot(p: &Project) -> Project {
    Project {
        plan: FloorPlan {
            size: p.plan.size,
            image: None,
            walls: Vec::new(),
            calibration: p.plan.calibration.clone(),
        },
        surveys: p.surveys.clone(),
        name: String::new(),
        schema_version: p.schema_version,
        next_id: p.next_id,
        demo: p.demo,
    }
}
pub fn decoded_plan(plan: &FloorPlan) -> Result<Option<egui::ColorImage>> {
    plan.image
        .as_ref()
        .map(|img| {
            wifi_map::persistence::decode_image(&img.png).map(|rgba| {
                egui::ColorImage::from_rgba_unmultiplied(
                    [rgba.width() as usize, rgba.height() as usize],
                    rgba.as_raw(),
                )
            })
        })
        .transpose()
}
