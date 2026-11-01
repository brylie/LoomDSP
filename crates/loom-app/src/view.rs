//! The window: the toolbar, the canvas with its overlays, the right panel,
//! and the stats bar. Each is drawn from the app's parts; nothing here
//! changes state.

use crate::matrix::{Event as MatrixEvent, Tab as RightTab};
use crate::style::button;
use crate::subscriptions::{browser_key, help_key};
use crate::{
    App, DIM, FileAction, Message, alerts, browser, inspector, learn, matrix, panic, pointer, probe_menu, settings,
    style, undo,
};
use iced::widget::{column, container, mouse_area, row, rule, space, stack, text, toggler, tooltip};
use iced::{Color, Element, Length};
use loom_canvas::{DragLine, PatchCanvas};
use std::time::Instant;

impl App {
    pub(crate) fn view(&self) -> Element<'_, Message> {
        let mut canvas = PatchCanvas::new(&self.scene, &self.selection, Message::Canvas)
            .values(&self.values)
            .window_drag_line(self.gestures.assign_drag().is_some())
            .traces(&self.traces)
            .activity(self.carrying.iter().map(|output| (output.module.0, output.port)))
            .frame_key(self.frame_key)
            .browser_key(self.browser_key)
            .interactive(
                self.browser.is_none()
                    && self.probe_menu.is_none()
                    && self.tile_menu.is_none()
                    && self.help.is_none()
                    && self.rig.window.is_none(),
            )
            .matrix_line(self.matrix_line())
            .matrix_keyboard(self.matrix.tab == RightTab::Matrix && self.matrix.focused)
            .matrix_escape(
                self.matrix.tab == RightTab::Matrix
                    && self.matrix.focused
                    && self.matrix.edit.is_none()
                    && self.matrix.add.is_none(),
            )
            .animate(self.animating)
            .learn(self.learn.shown());
        if let Some(still) = self.still {
            canvas = canvas.still(still);
        }

        let body = row![
            container(canvas).width(Length::Fill).height(Length::Fill),
            rule::vertical(1),
            self.right_panel(),
        ];

        let main = column![
            self.toolbar(),
            rule::horizontal(1),
            body,
            rule::horizontal(1),
            self.stats_bar()
        ];
        // Always a stack, so opening the browser keeps the canvas's state
        // (its camera) in the same place in the widget tree. Tested by
        // `ui_test::tests::the_camera_survives_opening_the_browser`.
        let mut layers = stack![main];
        if let Some(drag) = self.gestures.assign_drag()
            && let Some(at) = drag.at
            && let Some(port) = self.scene.port(drag.from)
        {
            layers = layers.push(DragLine::new(drag.start, at, port.colour, drag.zoom));
        }
        if let Some(open) = &self.browser {
            let outside = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::Browser(browser::Message::Close));
            layers = layers.push(outside).push(open.view().map(Message::Browser));
        }
        if let Some(menu) = &self.probe_menu {
            let outside = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::ProbeMenu(probe_menu::Message::Close))
                .on_right_press(Message::ProbeMenu(probe_menu::Message::Close));
            layers = layers.push(outside).push(menu.view().map(Message::ProbeMenu));
        }
        if let Some((module, at)) = self.tile_menu {
            let outside = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::CloseTileMenu)
                .on_right_press(Message::CloseTileMenu);
            let menu = container(button("Help").on_press(Message::ShowModuleHelp(module)).width(140))
                .padding(4)
                .style(container::rounded_box);
            layers = layers
                .push(outside)
                .push(iced::widget::pin(iced::widget::opaque(menu)).position(at));
        }
        if let Some(help) = &self.help {
            layers = layers.push(iced::widget::opaque(help.view(self.theme())));
        }
        if let Some(window) = self.preferences_view() {
            layers = layers.push(window);
        }
        // The control channel's pointer events go in here, as a real
        // pointer's would (control-channel.md, Pointer commands).
        let browser_open = self.browser.is_some() && self.help.is_none();
        let matrix_keys = self.matrix.tab == RightTab::Matrix && self.matrix.focused;
        pointer::Injector::new(layers, self, move |key, status| {
            // As the subscriptions do: `keyboard::listen` passes on what no
            // widget took, and the open browser hears every key.
            let mut messages = Vec::new();
            if status == iced::event::Status::Ignored {
                messages.push(Message::Key(key.clone()));
            }
            if matrix_keys && let Some(message) = matrix::key_message(key.clone(), status) {
                messages.push(message);
            }
            if let Some(message) = panic::key(iced::Event::Keyboard(key.clone()), status, iced::window::Id::unique()) {
                messages.push(message);
            }
            if let Some(message) = help_key(iced::Event::Keyboard(key.clone()), status, iced::window::Id::unique()) {
                messages.push(message);
            }
            if browser_open
                && let Some(message) = browser_key(iced::Event::Keyboard(key), status, iced::window::Id::unique())
            {
                messages.push(message);
            }
            messages
        })
        .into()
    }

    /// The toolbar: the patch name, then buttons in groups (file, add,
    /// play, edit, setup), with wider space between groups than inside them
    /// (visual-system.md, Toolbar).
    pub(crate) fn toolbar(&self) -> Element<'_, Message> {
        let tip = |button: iced::widget::Button<'static, Message>, hint: &'static str| -> Element<'static, Message> {
            tooltip(
                button,
                container(text(hint).size(style::BODY))
                    .padding(6)
                    .style(container::rounded_box),
                tooltip::Position::Bottom,
            )
            .into()
        };
        let label = |s: &'static str| text(s).size(style::CONTROL);
        let file = row![
            button(label("New")).on_press(Message::File(FileAction::New)),
            button(label("Open")).on_press(Message::File(FileAction::Open)),
            button(label("Save")).on_press(Message::File(FileAction::Save)),
            button(label("Save As")).on_press(Message::File(FileAction::SaveAs)),
        ];
        let add = tip(
            button(label("Add module")).on_press(Message::OpenBrowser),
            "Or press A over the canvas, or double-click empty canvas",
        );
        let play = row![
            button(label(if self.holding { "Release note" } else { "Hold note" }))
                .on_press_maybe(self.midi.as_ref().map(|_| Message::HoldNote)),
            tip(
                button(label("Panic")).on_press(Message::Panic).style(style::danger),
                if cfg!(target_os = "macos") {
                    "Silence everything: Cmd+."
                } else {
                    "Silence everything: Ctrl+."
                },
            ),
        ];
        let edit = row![
            self.history_button("Undo", "Undo", self.edits.history.undo_step(), Message::Undo),
            self.history_button("Redo", "Redo", self.edits.history.redo_step(), Message::Redo),
        ];
        let setup = row![
            // Primary only while learning: a mode that is on.
            button(label("MIDI learn"))
                .on_press(Message::Learn(learn::Message::Toggle))
                .style(if self.learn.is_on() {
                    style::primary
                } else {
                    style::secondary
                }),
            tip(
                button(label("Preferences")).on_press(Message::Preferences(settings::Message::Open)),
                if cfg!(target_os = "macos") { "Cmd+," } else { "Ctrl+," },
            ),
        ];
        let group = |items: iced::widget::Row<'static, Message>| items.spacing(6).align_y(iced::Alignment::Center);
        row![
            text(format!(
                "{}{}",
                if self.unsaved() { "* " } else { "" },
                self.patch_name()
            ))
            .size(style::HEADING),
            group(file),
            space::horizontal(),
            add,
            group(play),
            group(edit),
            group(setup),
        ]
        .spacing(20)
        .padding([6, 10])
        .align_y(iced::Alignment::Center)
        .into()
    }

    /// An Undo or Redo button. It is disabled with nothing to do, and its
    /// hover text names the step.
    pub(crate) fn history_button<'a>(
        &self,
        label: &'a str,
        verb: &str,
        step: Option<&undo::Step>,
        message: Message,
    ) -> Element<'a, Message> {
        let hint = match step {
            Some(step) => format!("{verb}: {}", step.name()),
            None => format!("Nothing to {}", verb.to_lowercase()),
        };
        tooltip(
            button(text(label).size(style::CONTROL)).on_press_maybe(step.map(|_| message)),
            container(text(hint).size(style::BODY))
                .padding(6)
                .style(container::rounded_box),
            tooltip::Position::Bottom,
        )
        .into()
    }

    pub(crate) fn inspector(&self) -> Element<'_, Message> {
        inspector::view(
            self.inspector_model(),
            self.gestures.drop_row(),
            self.gestures.assign_drag().is_some() && !self.learn.is_on(),
        )
    }

    /// Inspector and matrix share the same width in the right-hand column.
    pub(crate) fn right_panel(&self) -> Element<'_, Message> {
        // The open tab has a fill and a border (visual-system.md, Tabs).
        let tab = |label: &'static str, which: RightTab| {
            button(text(label).size(style::CONTROL))
                .padding([4, 12])
                .style(style::tab(self.matrix.tab == which))
                .on_press(Message::Matrix(MatrixEvent::Tab(which)))
        };
        let tabs = row![tab("Inspector", RightTab::Inspector), tab("Matrix", RightTab::Matrix)]
            .spacing(4)
            .padding([6, 8]);
        let panel = match self.matrix.tab {
            RightTab::Inspector => self.inspector(),
            RightTab::Matrix => matrix::view(
                self.matrix.model(self.controller.graph(), &self.selection),
                &self.matrix,
                self.controller.graph(),
            ),
        };
        column![tabs, panel].width(280).height(Length::Fill).into()
    }

    pub(crate) fn stats_bar(&self) -> Element<'_, Message> {
        let graph = self.controller.graph();
        let frames = if self.measuring {
            self.meter.stats().summary()
        } else {
            "redraws on change".to_string()
        };
        // Load and dropouts are always shown, and turn amber or red before
        // a problem is audible (FR-PERF-04).
        // The snapshot reads the same model, so the two cannot disagree.
        let stats = self.stats_model();
        let audio: Element<'_, Message> = if stats.playing {
            let load = bar_text(stats.load_text()).color(severity_colour(alerts::load_severity(self.stream.load().0)));
            let dropouts = bar_text(format!("dropouts {}", stats.dropouts))
                .color(severity_colour(self.alerts.dropout_severity(Instant::now())));
            let mut items = row![bar_text(stats.label).color(DIM), load, dropouts].spacing(10);
            if let Some((underruns, overruns)) = stats.input_alert {
                items = items.push(
                    bar_text(format!("input underruns {underruns}, overruns {overruns}"))
                        .color(severity_colour(alerts::Severity::Warning)),
                );
            }
            if stats.telemetry_dropped > 0 {
                items = items.push(bar_text(format!("telemetry dropped {}", stats.telemetry_dropped)).color(DIM));
            }
            items.into()
        } else {
            bar_text(stats.label).color(DIM).into()
        };
        // A count, with the names in hover text, so a long list never
        // pushes the other items off the window (visual-system.md, rule 7).
        let midi: Element<'_, Message> = match self.rig.ports.as_slice() {
            [] => bar_text("MIDI: no inputs").color(DIM).into(),
            ports => tooltip(
                bar_text(match ports.len() {
                    1 => "MIDI: 1 input".to_string(),
                    n => format!("MIDI: {n} inputs"),
                })
                .color(DIM),
                container(text(ports.iter().map(|port| port.name()).collect::<Vec<_>>().join("\n")).size(style::BODY))
                    .padding(6)
                    .style(container::rounded_box),
                tooltip::Position::Top,
            )
            .into(),
        };
        let mut bar = row![].spacing(16).align_y(iced::Alignment::Center);
        if self.dev_tools {
            bar = bar
                .push(
                    toggler(self.measuring)
                        .label("Measure")
                        .on_toggle(Message::Measure)
                        .size(style::SWITCH)
                        .text_size(style::BODY),
                )
                .push(
                    toggler(self.animating)
                        .label("Move camera")
                        .on_toggle(Message::Animate)
                        .size(style::SWITCH)
                        .text_size(style::BODY),
                )
                .push(bar_text(frames));
        }
        bar.push(bar_text(format!(
            "{} modules, {} cables",
            graph.len(),
            graph.cables().count()
        )))
        .push(audio)
        .push(midi)
        // The status takes the remaining width and is cut off rather
        // than wrapped, so the bar stays one line high. Hover shows all
        // of it.
        .push(tooltip(
            container(bar_text(self.alerts.status()).color(DIM))
                .width(Length::Fill)
                .align_x(iced::Alignment::End)
                .clip(true),
            container(text(self.alerts.status()).size(style::BODY))
                .padding(6)
                .max_width(480)
                .style(container::rounded_box),
            tooltip::Position::Top,
        ))
        .padding([4, 10])
        .into()
    }
}

/// The inspector's remove button for an item in a list, such as a MIDI
/// mapping or a modulation assignment: a small × at the end of its row,
/// with hover text that names what it removes (panels.md, The inspector).
pub(crate) fn remove_button<'a>(what: &'a str, on_press: Message) -> Element<'a, Message> {
    tooltip(
        button(text("×").size(style::CONTROL))
            .padding([0, 6])
            .style(style::plain)
            .on_press(on_press),
        container(text(what).size(style::BODY))
            .padding(6)
            .style(container::rounded_box),
        tooltip::Position::Top,
    )
    .into()
}

/// Stats bar text: one line, never wrapped.
pub(crate) fn bar_text<'a>(content: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(content).size(style::BODY).wrapping(text::Wrapping::None)
}

/// Stats bar colour for a warning level.
pub(crate) fn severity_colour(severity: alerts::Severity) -> Color {
    match severity {
        alerts::Severity::Normal => DIM,
        alerts::Severity::Warning => Color::from_rgb8(0xF5, 0xA6, 0x23),
        alerts::Severity::Alarm => Color::from_rgb8(0xEB, 0x6B, 0x6B),
    }
}
