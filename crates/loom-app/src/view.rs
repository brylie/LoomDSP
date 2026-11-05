//! The window: the toolbar, the canvas with its overlays, the right panel,
//! and the stats bar. Each is drawn from the app's parts; nothing here
//! changes state.

use crate::edits::ArrowGroup;
use crate::matrix::{Event as MatrixEvent, Tab as RightTab};
use crate::style::button;
use crate::{
    App, DIM, FileAction, Message, alerts, browser, inspector, learn, matrix, pointer, probe_menu, recent, settings,
    style,
};
use iced::widget::{column, container, mouse_area, row, rule, space, stack, text, toggler, tooltip};
use iced::{Color, Element, Length};
use loom_canvas::{ArrowGroupView, DragLine, PatchCanvas};
use std::sync::atomic::Ordering;
use std::time::Instant;

impl App {
    pub(crate) fn view(&self) -> Element<'_, Message> {
        let mut canvas = PatchCanvas::new(&self.session.scene, &self.session.selection, Message::Canvas)
            .values(&self.session.values)
            .window_drag_line(self.session.gestures.assign_drag().is_some())
            .traces(&self.session.traces)
            .activity(
                self.session
                    .carrying
                    .iter()
                    .map(|output| (output.module.0, output.port)),
            )
            .frame_key(self.frame_key)
            .browser_key(self.browser_key)
            .focus_policy(self.canvas_focus_policy())
            .arrow_group(match self.session.edits.arrow_group() {
                ArrowGroup::Closed => ArrowGroupView::Closed,
                ArrowGroup::Held => ArrowGroupView::Held,
                ArrowGroup::Released { due } => ArrowGroupView::Released { due },
            })
            .matrix_line(self.matrix_line())
            .typed_check(|module, widget, text| self.typed_check(module, widget, text))
            .animate(self.animating)
            .learn(self.session.learn.shown());
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
        if let Some(drag) = self.session.gestures.assign_drag()
            && let Some(at) = drag.at
            && let Some(port) = self.session.scene.port(drag.from)
        {
            layers = layers.push(DragLine::new(drag.start, at, port.colour, drag.zoom));
        }
        if let Some(open) = self.overlays.browser() {
            let outside = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::Browser(browser::Message::Close));
            layers = layers.push(outside).push(open.view().map(Message::Browser));
        }
        if let Some(menu) = self.overlays.probe_menu() {
            let outside = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::ProbeMenu(probe_menu::Message::Close))
                .on_right_press(Message::ProbeMenu(probe_menu::Message::Close));
            layers = layers.push(outside).push(menu.view().map(Message::ProbeMenu));
        }
        if let Some((module, at)) = self.overlays.tile_menu() {
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
        if let Some(rows) = self.overlays.recent_rows() {
            let outside = mouse_area(space::horizontal().width(Length::Fill).height(Length::Fill))
                .on_press(Message::Recent(recent::Message::Close))
                .on_right_press(Message::Recent(recent::Message::Close));
            layers = layers.push(outside).push(
                iced::widget::pin(iced::widget::opaque(recent::view(rows).map(Message::Recent)))
                    .position(recent::LIST_AT),
            );
        }
        if let Some(window) = self.preferences_view() {
            layers = layers.push(window);
        }
        if let Some(open) = self.overlays.patches() {
            layers = layers.push(open.view().map(Message::Patches));
        }
        // Over the Preferences window, which also opens it.
        if let Some(chooser) = &self.overlays.chooser {
            layers = layers.push(chooser.view());
        }
        // Over the dialogs, because F1 opens it from them (help-pages.md).
        if let Some(help) = &self.overlays.help {
            layers = layers.push(iced::widget::opaque(help.view(self.theme())));
        }
        // Over everything: a question must be answered first.
        if let Some(question) = &self.overlays.question {
            layers = layers.push(question.view().map(Message::Question));
        }
        // The control channel's pointer events go in here, as a real
        // pointer's would (control-channel.md, Pointer commands).
        let listening = self.listening();
        let on_input = move |event: iced::Event, status| crate::input_owner::heard_messages(&event, status, listening);
        pointer::Injector::new(layers, self, on_input).into()
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
            tip(
                button(label("Browse")).on_press(Message::File(FileAction::Browse)),
                if cfg!(target_os = "macos") {
                    "Your patches: Cmd+Shift+O"
                } else {
                    "Your patches: Ctrl+Shift+O"
                },
            ),
            tip(
                button(label("Recent"))
                    .on_press_maybe((!self.rig.recent.files().is_empty()).then_some(Message::OpenRecent)),
                self.recent_hover(),
            ),
        ];
        let add = tip(
            button(label("Add module")).on_press(Message::OpenBrowser),
            "Or press A over the canvas, or double-click empty canvas",
        );
        let hold = button(label(if self.session.engine.holding {
            "Release note"
        } else {
            "Hold note"
        }))
        .on_press_maybe(self.session.engine.midi.as_ref().map(|_| Message::HoldNote));
        // Disabled controls say why (visual-system.md, Status messages).
        let hold: Element<'static, Message> = match self.hold_hover() {
            Some(reason) => tip(hold, reason),
            None => hold.into(),
        };
        let play = row![
            hold,
            tip(
                button(label("Panic")).on_press(Message::Panic).style(style::danger),
                if cfg!(target_os = "macos") {
                    "Silence everything: Cmd+."
                } else {
                    "Silence everything: Ctrl+."
                },
            ),
        ];
        let (undo_hint, redo_hint) = (self.undo_hint(), self.redo_hint());
        let edit = row![
            history_button("Undo", undo_hint, "Nothing to undo", Message::Undo),
            history_button("Redo", redo_hint, "Nothing to redo", Message::Redo),
        ];
        let setup = row![
            // Primary only while learning: a mode that is on.
            button(label("MIDI learn"))
                .on_press(Message::Learn(learn::Message::Toggle))
                .style(if self.session.learn.is_on() {
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

    /// Hold note's hover text: why it is disabled, if it is.
    pub(crate) fn hold_hover(&self) -> Option<&'static str> {
        self.session.engine.midi.is_none().then_some(HOLD_DISABLED)
    }

    /// Recent's hover text, which says why while it is disabled.
    pub(crate) fn recent_hover(&self) -> &'static str {
        if self.rig.recent.files().is_empty() {
            RECENT_DISABLED
        } else {
            "The last patches you opened or saved"
        }
    }

    /// An Undo or Redo button. It is disabled with nothing to do, and its
    /// hover text names the step.
    pub(crate) fn inspector(&self) -> Element<'_, Message> {
        inspector::view(
            self.inspector_model(),
            &self.session.patch_info.authors,
            self.session.gestures.drop_row(),
            self.session.gestures.assign_drag().is_some() && !self.session.learn.is_on(),
        )
    }

    /// Inspector and matrix share the same width in the right-hand column.
    pub(crate) fn right_panel(&self) -> Element<'_, Message> {
        // The open tab has a fill and a border (visual-system.md, Tabs).
        let tab = |label: &'static str, which: RightTab| {
            button(text(label).size(style::CONTROL))
                .padding([4, 12])
                .style(style::tab(self.right_tab == which))
                .on_press(Message::Matrix(MatrixEvent::Tab(which)))
        };
        let tabs = row![tab("Inspector", RightTab::Inspector), tab("Matrix", RightTab::Matrix)]
            .spacing(4)
            .padding([6, 8]);
        let panel = match self.right_tab {
            RightTab::Inspector => self.inspector(),
            RightTab::Matrix => matrix::view(
                self.session
                    .matrix
                    .model(self.session.controller.graph(), &self.session.selection, self.right_tab),
                &self.session.matrix,
                self.session.controller.graph(),
            ),
        };
        column![tabs, panel].width(280).height(Length::Fill).into()
    }

    pub(crate) fn stats_bar(&self) -> Element<'_, Message> {
        let graph = self.session.controller.graph();
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
            let load = bar_text(stats.load_text()).color(severity_colour(alerts::load_severity(
                self.session.engine.stream.load().0,
            )));
            let dropouts = bar_text(format!("dropouts {}", stats.dropouts))
                .color(severity_colour(self.alerts.dropout_severity(Instant::now())));
            let mut items = row![bar_text(stats.label).color(DIM), load, dropouts].spacing(10);
            if let Some((underruns, overruns)) = stats.input_alert {
                items = items.push(
                    bar_text(format!("input underruns {underruns}, overruns {overruns}"))
                        .color(severity_colour(alerts::Severity::Warning)),
                );
            }
            let engine = self.session.controller.stats();
            if stats.output_silenced {
                items = items.push(output_warning(
                    "Output: bad samples silenced",
                    alerts::Severity::Alarm,
                    format!(
                        "{} samples since the engine started",
                        engine.output_silenced.load(Ordering::Relaxed)
                    ),
                ));
            }
            if stats.output_clipped {
                items = items.push(output_warning(
                    "Output clipped",
                    alerts::Severity::Warning,
                    format!(
                        "{} samples since the engine started",
                        engine.output_clipped.load(Ordering::Relaxed)
                    ),
                ));
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
        // A word, so it reads without colour, and it stays whatever the
        // status says: in learn mode screen controls do not change values.
        let learning = self.session.learn.is_on().then(|| bar_text("Learning"));
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
        .extend(learning.map(Element::from))
        // The fixed items come first. The kept error and the status share
        // the remaining width, and are cut off rather than wrapped, so the
        // bar stays one line high. Hover shows all of it.
        .extend(self.alerts.kept_field().map(|kept| {
            status_field(
                alerts::labelled(kept.severity, &kept.text),
                self.alerts.kept_hover(),
                kept.severity,
                kept.dismissible.then_some(kept.cause),
            )
        }))
        .push(status_field(
            alerts::labelled(self.alerts.severity(), self.alerts.status()),
            if self.alerts.status_is_kept() {
                self.alerts.kept_hover()
            } else {
                alerts::labelled(self.alerts.severity(), self.alerts.status())
            },
            self.alerts.severity(),
            self.alerts
                .top_kept()
                .filter(|kept| kept.dismissible && self.alerts.kept_field().is_none())
                .map(|kept| kept.cause),
        ))
        .padding([4, 10])
        .into()
    }
}

/// Hold note's hover text while it is disabled. The control channel refuses
/// the press with the same words.
pub(crate) const HOLD_DISABLED: &str = "Hold note is disabled: no audio is playing the engine";

/// Recent's hover text while it is disabled.
const RECENT_DISABLED: &str = "Recent is disabled: no patches opened or saved yet";

/// A stats bar warning about the output, with its counts in hover text.
fn output_warning<'a>(label: &'a str, severity: alerts::Severity, hover: String) -> Element<'a, Message> {
    tooltip(
        bar_text(label).color(severity_colour(severity)),
        container(text(hover).size(style::BODY))
            .padding(6)
            .style(container::rounded_box),
        tooltip::Position::Top,
    )
    .into()
}

/// One message in the status bar: cut to its share of the width, all of it in
/// hover text, with a × if the player may dismiss it.
fn status_field<'a>(
    shown: String,
    hover: String,
    severity: alerts::Severity,
    dismiss: Option<alerts::Cause>,
) -> Element<'a, Message> {
    row![tooltip(
        container(bar_text(shown).color(severity_colour(severity)))
            .width(Length::Fill)
            .align_x(iced::Alignment::End)
            .clip(true),
        container(text(hover).size(style::BODY))
            .padding(6)
            .max_width(480)
            .style(container::rounded_box),
        tooltip::Position::Top,
    )]
    .extend(dismiss.map(|cause| {
        let hover = if severity == alerts::Severity::Warning {
            "Dismiss this warning"
        } else {
            "Dismiss this error"
        };
        remove_button(hover, Message::DismissError(cause))
    }))
    .width(Length::Fill)
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .into()
}

/// The inspector's remove button for an item in a list, such as a MIDI
/// mapping or a modulation assignment: a small × at the end of its row,
/// with hover text that names what it removes (panels.md, The inspector).
pub(crate) fn remove_button<'a>(what: impl Into<String>, on_press: Message) -> Element<'a, Message> {
    tooltip(
        button(text("×").size(style::CONTROL))
            .padding([0, 6])
            .style(style::plain)
            .on_press(on_press),
        container(text(what.into()).size(style::BODY))
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
        alerts::Severity::Warning => style::WARNING_TEXT,
        alerts::Severity::Alarm => style::ERROR_TEXT,
    }
}

/// A toolbar Undo or Redo button. `hint` names what it would do, and the
/// button works only when there is one. `nothing` is the hint otherwise.
fn history_button<'a>(label: &'a str, hint: Option<String>, nothing: &str, message: Message) -> Element<'a, Message> {
    let enabled = hint.is_some();
    let hint = hint.unwrap_or_else(|| nothing.to_string());
    tooltip(
        button(text(label).size(style::CONTROL)).on_press_maybe(enabled.then_some(message)),
        container(text(hint).size(style::BODY))
            .padding(6)
            .style(container::rounded_box),
        tooltip::Position::Bottom,
    )
    .into()
}
