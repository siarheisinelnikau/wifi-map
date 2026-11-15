//! CPU rasterization -> retained bitmap -> softbuffer Win32 GDI. No GPU API.
use crate::{
    app::{PendingAction, WifiApp},
    ui,
    workers::EventSink,
};
use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use std::{
    num::NonZeroU32,
    path::PathBuf,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};
use wifi_map::scheduling::FrameSchedule;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};

pub enum Event {
    Wake,
    Accessibility(egui_winit::accesskit_winit::Event),
}
impl From<egui_winit::accesskit_winit::Event> for Event {
    fn from(event: egui_winit::accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}
#[derive(Default)]
struct Statistics {
    layouts: u64,
    rasters: u64,
    presents: u64,
    layout_us: u128,
    raster_us: u128,
    present_us: u128,
    max_frame_us: u128,
}
struct RenderState {
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    _context: softbuffer::Context<Rc<Window>>,
    window: Rc<Window>,
    ctx: egui::Context,
    input: egui_winit::State,
    renderer: EguiSoftwareRender,
    pixels: Vec<u32>,
    size: [u32; 2],
    schedule: FrameSchedule,
}
pub struct Host {
    proxy: EventLoopProxy<Event>,
    state: Option<RenderState>,
    app: Option<WifiApp>,
    initial: Option<PathBuf>,
    stats: Statistics,
    diagnostics: Option<PathBuf>,
    error: Option<String>,
    update_failed: Option<PathBuf>,
}
impl Host {
    fn create(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn std::error::Error>> {
        let rgba = image::load_from_memory(include_bytes!("../assets/icon.png"))?.into_rgba8();
        let icon =
            winit::window::Icon::from_rgba(rgba.as_raw().clone(), rgba.width(), rgba.height())?;
        let window = Rc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("WifiMap")
                    .with_visible(false)
                    .with_inner_size(LogicalSize::new(1280.0, 860.0))
                    .with_min_inner_size(LogicalSize::new(960.0, 660.0))
                    .with_window_icon(Some(icon)),
            )?,
        );
        let context = softbuffer::Context::new(window.clone())?;
        let surface = softbuffer::Surface::new(&context, window.clone())?;
        let ctx = egui::Context::default();
        let mut input = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(8192),
        );
        input.init_accesskit(event_loop, &window, self.proxy.clone());
        let (tx, receiver) = mpsc::channel();
        let proxy = self.proxy.clone();
        let sink = EventSink::new(tx, move || {
            let _ = proxy.send_event(Event::Wake);
        });
        let mut app = WifiApp::new(ctx.clone(), receiver, sink, self.initial.take());
        if let Some(path) = &self.update_failed {
            app.status = Some(wifi_map::error::AppError::detail(
                "update_install",
                path.with_extension("error.txt").display(),
            ));
        }
        ui::apply_style(&ctx, app.prefs.theme, app.prefs.reduced_motion);
        self.app = Some(app);
        self.state = Some(RenderState {
            surface,
            _context: context,
            window: window.clone(),
            ctx,
            input,
            renderer: EguiSoftwareRender::new(ColorFieldOrder::Bgra),
            pixels: Vec::new(),
            size: [0, 0],
            schedule: FrameSchedule::default(),
        });
        // AccessKit must be initialized before first visibility.
        window.set_visible(true);
        window.request_redraw();
        Ok(())
    }
    fn draw(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn std::error::Error>> {
        let Some(state) = &mut self.state else {
            return Ok(());
        };
        let Some(app) = &mut self.app else {
            return Ok(());
        };
        if state.window.is_minimized() == Some(true) {
            state.schedule.visibility(false);
            return Ok(());
        }
        if !state.schedule.visible {
            return Ok(());
        }
        let size = state.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        // Defensive bound for a CPU bitmap; Windows still owns desktop composition.
        if size.width as u64 * size.height as u64 > 32_000_000 {
            return Err("Window exceeds bitmap size limit".into());
        }
        if state.size != [size.width, size.height] {
            state.size = [size.width, size.height];
            state
                .pixels
                .resize(size.width as usize * size.height as usize, 0);
            state.surface.resize(
                NonZeroU32::new(size.width).unwrap(),
                NonZeroU32::new(size.height).unwrap(),
            )?;
            state.schedule.invalidate();
        }
        let start = Instant::now();
        if state.schedule.dirty {
            let mut raw = state.input.take_egui_input(&state.window);
            egui_winit::update_viewport_info(
                raw.viewports.entry(egui::ViewportId::ROOT).or_default(),
                &state.ctx,
                &state.window,
                self.stats.layouts == 0,
            );
            let layout = Instant::now();
            let output = state.ctx.run(raw, |ctx| app.ui(ctx));
            self.stats.layouts += 1;
            self.stats.layout_us += layout.elapsed().as_micros();
            state
                .input
                .handle_platform_output(&state.window, output.platform_output);
            let delay = output
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .map(|v| v.repaint_delay)
                .unwrap_or(Duration::MAX);
            let jobs = state.ctx.tessellate(output.shapes, output.pixels_per_point);
            let raster = Instant::now();
            state.renderer.render(
                &mut BufferMutRef::new(
                    bytemuck::cast_slice_mut(&mut state.pixels),
                    size.width as usize,
                    size.height as usize,
                ),
                &jobs,
                &output.textures_delta,
                output.pixels_per_point,
            );
            self.stats.rasters += 1;
            self.stats.raster_us += raster.elapsed().as_micros();
            state.schedule.completed(Instant::now(), delay);
            let name = if app.project.name.is_empty() {
                app.text().get("untitled")
            } else {
                &app.project.name
            };
            state.window.set_title(&format!(
                "{}{} — WifiMap",
                if app.dirty { "* " } else { "" },
                name
            ));
        }
        let present = Instant::now();
        let mut buffer = state.surface.buffer_mut()?;
        buffer.copy_from_slice(&state.pixels);
        buffer.present()?;
        self.stats.presents += 1;
        self.stats.present_us += present.elapsed().as_micros();
        self.stats.max_frame_us = self.stats.max_frame_us.max(start.elapsed().as_micros());
        if app.exit {
            app.cancel.cancel();
            app.update_cancel.cancel();
            event_loop.exit();
        }
        Ok(())
    }
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl ToString) {
        self.error = Some(error.to_string());
        event_loop.exit();
    }
}
impl ApplicationHandler<Event> for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
        if self.state.is_none()
            && let Err(e) = self.create(event_loop)
        {
            self.fail(event_loop, e);
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else {
            return;
        };
        if state.window.id() != id {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                if let Some(app) = &mut self.app {
                    app.request_action(PendingAction::Close);
                    if app.exit {
                        event_loop.exit();
                        return;
                    }
                    state.window.set_minimized(false);
                    state.schedule.visibility(true);
                    state.schedule.invalidate();
                }
            }
            WindowEvent::Occluded(hidden) => {
                state
                    .schedule
                    .visibility(!hidden && state.window.is_minimized() != Some(true));
            }
            WindowEvent::Resized(size) => {
                state.schedule.visibility(
                    size.width > 0 && size.height > 0 && state.window.is_minimized() != Some(true),
                );
                state.schedule.invalidate();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.draw(event_loop) {
                    self.fail(event_loop, e);
                }
                return;
            }
            _ => {}
        }
        if let Some(state) = &mut self.state {
            let response = state.input.on_window_event(&state.window, &event);
            if response.repaint {
                state.schedule.invalidate();
            }
        }
    }
    fn user_event(&mut self, _: &ActiveEventLoop, event: Event) {
        let Some(state) = &mut self.state else {
            return;
        };
        match event {
            Event::Wake => state.schedule.invalidate(),
            Event::Accessibility(event) => {
                if event.window_id == state.window.id() {
                    match event.window_event {
                        egui_winit::accesskit_winit::WindowEvent::InitialTreeRequested => {
                            state.ctx.enable_accesskit()
                        }
                        egui_winit::accesskit_winit::WindowEvent::ActionRequested(request) => {
                            state.input.on_accesskit_action_request(request)
                        }
                        egui_winit::accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                            state.ctx.disable_accesskit()
                        }
                    }
                    state.schedule.invalidate();
                }
            }
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = &mut self.state {
            if state.schedule.take_due(Instant::now()) {
                state.window.request_redraw();
            }
            event_loop.set_control_flow(
                state
                    .schedule
                    .deadline()
                    .map(ControlFlow::WaitUntil)
                    .unwrap_or(ControlFlow::Wait),
            );
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
    fn exiting(&mut self, _: &ActiveEventLoop) {
        if let Some(app) = &mut self.app {
            app.cancel.cancel();
            app.update_cancel.cancel();
            app.worker.cancel();
        }
        if let Some(path) = &self.diagnostics {
            let data = format!(
                "backend=egui_software_backend 0.0.2 -> softbuffer 0.4.8 Win32 GDI\nlayouts={}\nrasters={}\npresents={}\nlayout_us={}\nraster_us={}\npresent_us={}\nmax_frame_us={}\n",
                self.stats.layouts,
                self.stats.rasters,
                self.stats.presents,
                self.stats.layout_us,
                self.stats.raster_us,
                self.stats.present_us,
                self.stats.max_frame_us
            );
            let _ = wifi_map::persistence::atomic_write(path, data.as_bytes());
        }
    }
}
pub fn run(
    initial: Option<PathBuf>,
    diagnostics: Option<PathBuf>,
    update_failed: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::<Event>::with_user_event().build()?;
    let mut host = Host {
        proxy: event_loop.create_proxy(),
        state: None,
        app: None,
        initial,
        stats: Statistics::default(),
        diagnostics,
        error: None,
        update_failed,
    };
    event_loop.run_app(&mut host)?;
    if let Some(error) = host.error {
        return Err(error.into());
    }
    Ok(())
}
