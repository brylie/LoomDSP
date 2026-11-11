//! The winit window that hands the sliders' tree to AccessKit through
//! `accesskit_winit`. The window draws nothing: the sliders exist only in
//! the accessibility tree, which is all this reproduction needs.

use crate::model::{self, Slider};
use accesskit_winit::{Adapter, Event as AccessEvent, WindowEvent as AccessWindowEvent};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::error::EventLoopError;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// The running program: its sliders, which one has focus, and the window
/// with its AccessKit adapter once the event loop has started.
struct App {
    title: &'static str,
    sliders: Vec<Slider>,
    focus: usize,
    proxy: EventLoopProxy<AccessEvent>,
    open: Option<(Window, Adapter)>,
}

impl App {
    /// Sends the current tree to AccessKit, if assistive technology is
    /// listening, and prints the sliders so a run can be followed in the
    /// terminal.
    fn publish(&mut self) {
        let Some((window, adapter)) = &mut self.open else {
            return;
        };
        let scale = window.scale_factor();
        adapter.update_if_active(|| model::tree(self.title, &self.sliders, self.focus, scale));
        for (index, slider) in self.sliders.iter().enumerate() {
            let mark = if index == self.focus { ">" } else { " " };
            let text = slider.text().unwrap_or_else(|| "(no string value)".into());
            println!("{mark} {} = {}  string value: {text}", slider.label, slider.value);
        }
    }
}

impl ApplicationHandler<AccessEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.open.is_some() {
            return;
        }
        // AccessKit's adapter must exist before the window is first shown.
        let attributes = Window::default_attributes()
            .with_title(self.title)
            .with_inner_size(LogicalSize::new(400.0, 60.0 * self.sliders.len() as f64 + 20.0))
            .with_visible(false);
        let window = event_loop.create_window(attributes).expect("the window opens");
        let adapter = Adapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone());
        window.set_visible(true);
        self.open = Some((window, adapter));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some((window, adapter)) = &mut self.open else {
            return;
        };
        adapter.process_event(window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            // Bounds are in physical pixels, so a new scale factor needs a
            // new tree.
            WindowEvent::ScaleFactorChanged { .. } => self.publish(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.logical_key {
                    Key::Named(NamedKey::ArrowRight | NamedKey::ArrowUp) => {
                        model::nudge(&mut self.sliders, self.focus, 1.0);
                    }
                    Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowDown) => {
                        model::nudge(&mut self.sliders, self.focus, -1.0);
                    }
                    Key::Named(NamedKey::Tab) => self.focus = (self.focus + 1) % self.sliders.len(),
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    _ => return,
                }
                self.publish();
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AccessEvent) {
        match event.window_event {
            AccessWindowEvent::InitialTreeRequested => self.publish(),
            AccessWindowEvent::ActionRequested(request) => {
                println!(
                    "action from assistive technology: {:?} {:?}",
                    request.action, request.data
                );
                if model::apply(&mut self.sliders, &mut self.focus, &request) {
                    self.publish();
                }
            }
            AccessWindowEvent::AccessibilityDeactivated => {}
        }
    }
}

/// Opens a window titled `title` whose accessibility tree holds `sliders`,
/// and runs until the window closes or Escape is pressed.
///
/// Tab moves the focus between sliders, and the arrow keys move the
/// focused slider, so the program can be driven with no screen reader too.
pub fn run(title: &'static str, sliders: Vec<Slider>) -> Result<(), EventLoopError> {
    assert!(!sliders.is_empty(), "the window needs at least one slider");
    let event_loop = EventLoop::<AccessEvent>::with_user_event().build()?;
    let mut app = App {
        title,
        sliders,
        focus: 0,
        proxy: event_loop.create_proxy(),
        open: None,
    };
    event_loop.run_app(&mut app)
}
