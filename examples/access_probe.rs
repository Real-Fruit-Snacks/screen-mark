//! Opens the Settings window on its own, for checking what a screen reader
//! is told about it (Narrator, NVDA, Accessibility Insights, Inspect.exe).
//!
//! There is no tray icon and no hotkey, nothing is saved (the config goes
//! to a temp folder), the window never takes focus, and the two controls
//! that reach outside the window (auto-start, Browse) are not pressed even
//! if asked. It closes by itself after the given number of seconds.
//!
//! Run: cargo run --example access_probe -- 30

use std::sync::Arc;
use std::time::{Duration, Instant};

use screen_mark::access::{Bridge, Request};
use screen_mark::config::Config;
use screen_mark::settings_ui::{Action, Outcome, SettingsView, SettingsWindow, WIN_H, WIN_W};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{WindowAttributes, WindowId};

struct Probe {
    proxy: EventLoopProxy<()>,
    until: Instant,
    window: Option<SettingsWindow>,
    access: Option<Bridge>,
}

impl Probe {
    fn draw(&mut self) {
        let (Some(sw), Some(access)) = (self.window.as_mut(), self.access.as_mut()) else {
            return;
        };
        sw.render();
        access.update(sw.view.snapshot(), sw.window.scale_factor());
    }
}

impl ApplicationHandler for Probe {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // Hidden until the bridge is attached, as in the app.
        let attrs = WindowAttributes::default()
            .with_title("screen-mark Settings")
            .with_inner_size(winit::dpi::LogicalSize::new(WIN_W, WIN_H))
            .with_resizable(false)
            .with_active(false)
            .with_visible(false);
        let window = Arc::new(event_loop.create_window(attrs).expect("a window"));
        let context = softbuffer::Context::new(Arc::clone(&window)).expect("a context");
        let surface = softbuffer::Surface::new(&context, Arc::clone(&window)).expect("a surface");

        let proxy = self.proxy.clone();
        self.access = Some(Bridge::new(event_loop, &window, move || {
            let _ = proxy.send_event(());
        }));
        let view = SettingsView::new(Config::default(), Default::default());
        self.window = Some(SettingsWindow::new(Arc::clone(&window), surface, view));
        self.draw();
        window.set_visible(true);
        println!("probe: Settings is up");
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {}

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let asked = self
            .access
            .as_mut()
            .map(|a| a.requests())
            .unwrap_or_default();
        for request in asked {
            let Some(sw) = self.window.as_mut() else {
                break;
            };
            let (Request::Press(i) | Request::Focus(i)) = request;
            let target = sw
                .view
                .widgets()
                .get(i)
                .map(|w| (w.action, w.label.clone()));
            let Some((action, label)) = target else {
                continue;
            };
            let outside = matches!(action, Action::ToggleAutostart | Action::BrowseDir);
            if outside && matches!(request, Request::Press(_)) {
                println!("probe: not pressing \"{label}\" (it reaches outside this window)");
                continue;
            }
            let outcome = sw.view.request(request);
            println!("probe: {request:?} on \"{label}\" -> {outcome:?}");
            match outcome {
                Outcome::Close { .. } => {
                    event_loop.exit();
                    return;
                }
                Outcome::Redraw => sw.window.request_redraw(),
                Outcome::Nothing => {}
            }
        }
        if Instant::now() >= self.until {
            event_loop.exit();
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.until));
        }
    }
}

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    // Save & Close writes here, never to the real config.
    let home = std::env::temp_dir().join(format!("screen-mark-probe-{}", std::process::id()));
    std::env::set_var("SCREEN_MARK_HOME", &home);

    let event_loop = EventLoop::new().expect("an event loop");
    let mut probe = Probe {
        proxy: event_loop.create_proxy(),
        until: Instant::now() + Duration::from_secs(seconds),
        window: None,
        access: None,
    };
    event_loop.run_app(&mut probe).expect("the event loop runs");
    // The bridge goes before its window.
    probe.access = None;
    probe.window = None;
    let _ = std::fs::remove_dir_all(&home);
}
