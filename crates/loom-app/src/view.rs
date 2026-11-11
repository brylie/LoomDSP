//! The window: the toolbar, the canvas with its overlays, the right panel,
//! and the stats bar. Each is drawn from the app's parts; nothing here
//! changes state.

use crate::edits::ArrowGroup;
use crate::matrix::{Event as MatrixEvent, Tab as RightTab};
use crate::style::button;
use crate::{App, DIM, Message, alerts, browser, inspector, matrix, pointer, probe_menu, recent, style};
use iced::widget::{column, container, mouse_area, row, rule, space, stack, text, toggler, tooltip};
use iced::{Color, Element, Length};
use loom_canvas::iced_convert::ToIced as _;
use loom_canvas::{DragLine, EditGroupView, PatchCanvas};

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
            .level(
                self.session.level_key,
                self.session.cameras.get(&self.session.level).copied(),
            )
            .browser_key(self.browser_key)
            .focus_policy(self.canvas_focus_policy())
            .focus_request(self.a11y.focus_request().0, self.a11y.focus_request().1)
            .edit_group(match self.session.edits.arrow_group() {
                // A screen reader's group waits as a released arrow group
                // does, so the canvas is shown both as an open group: it
                // wakes once at the deadline, and reports the key or
                // pointer press that ends the group sooner. Nothing wakes
                // while no group is open.
                ArrowGroup::Closed => match self.session.edits.reader_group().due() {
                    Some(due) => EditGroupView::Open { due },
                    None => EditGroupView::Closed,
                },
                ArrowGroup::Held => EditGroupView::Held,
                ArrowGroup::Released { due } => EditGroupView::Open { due },
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

        // The breadcrumb slot is always there, empty at the top level, so the
        // canvas keeps its place in the widget tree and with it its state
        // (the camera) when a level opens or closes.
        let main = column![
            self.toolbar(),
            rule::horizontal(1),
            self.breadcrumb(),
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
            let mut items = column![].spacing(2);
            for (label, message) in self.tile_menu_items(module) {
                items = items.push(button(iced::widget::text(label)).on_press(message).width(200));
            }
            let menu = container(items).padding(4).style(container::rounded_box);
            layers = layers
                .push(outside)
                .push(iced::widget::pin(iced::widget::opaque(menu)).position(at.to_iced()));
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

    /// The breadcrumb bar, shown only while a sub-patch is open: `Patch ›
    /// Voice › Filter`, each name a button that shows that level
    /// (sub-patches.md, Make, open, and dissolve). At the top level it is
    /// an empty column of no height.
    pub(crate) fn breadcrumb(&self) -> Element<'_, Message> {
        let level = &self.session.level;
        if level.is_root() {
            return column![].into();
        }
        let names = self.session.hierarchy.path_names(level);
        let mut bar = row![
            button(text("Patch").size(style::CONTROL))
                .style(style::plain)
                .on_press(Message::OpenLevel(loom_patch::ModulePath::root()))
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center)
        .padding([2, 8]);
        for (depth, name) in names.into_iter().enumerate() {
            let target = level.prefix(depth + 1);
            bar = bar.push(text("›").size(style::CONTROL).color(style::QUIET));
            bar = bar.push(
                button(text(name).size(style::CONTROL))
                    .style(style::plain)
                    .on_press(Message::OpenLevel(target)),
            );
        }
        column![bar, rule::horizontal(1)].into()
    }

    /// The toolbar: the patch name, then buttons in groups (file, add,
    /// play, edit, setup), with wider space between groups than inside them
    /// (visual-system.md, Toolbar).
    pub(crate) fn toolbar(&self) -> Element<'_, Message> {
        use crate::toolbar::Group;
        let tip = |button: Element<'static, Message>, hint: String| -> Element<'static, Message> {
            tooltip(
                button,
                container(text(hint).size(style::BODY))
                    .padding(6)
                    .style(container::rounded_box),
                tooltip::Position::Bottom,
            )
            .into()
        };
        // Drawn from the model the accessibility tree reads too. Each
        // button sits in a container named by its UI ID, so the window
        // can say where it is (`places.rs`).
        let model = self.toolbar_model();
        let group = |which: Group| {
            let items = model.buttons.iter().filter(|b| b.group == which).map(|b| {
                let press = b.enabled.then(|| crate::toolbar::message(b.name)).flatten();
                let face = button(text(b.label).size(style::CONTROL)).on_press_maybe(press);
                let face = match (b.name, b.on) {
                    ("panic", _) => face.style(style::danger),
                    // Primary only while learning: a mode that is on.
                    (_, Some(true)) => face.style(style::primary),
                    (_, Some(false)) => face.style(style::secondary),
                    _ => face,
                };
                let named: Element<'static, Message> = container(face).id(b.id).into();
                match &b.hover {
                    Some(hint) => tip(named, hint.clone()),
                    None => named,
                }
            });
            iced::widget::Row::with_children(items)
                .spacing(6)
                .align_y(iced::Alignment::Center)
        };
        container(
            row![
                text(format!(
                    "{}{}",
                    if self.unsaved() { "* " } else { "" },
                    self.patch_name()
                ))
                .size(style::HEADING),
                group(Group::File),
                space::horizontal(),
                group(Group::Add),
                group(Group::Play),
                group(Group::Edit),
                group(Group::Setup),
            ]
            .spacing(20)
            .padding([6, 10])
            .align_y(iced::Alignment::Center),
        )
        .id("toolbar")
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

    pub(crate) fn inspector(&self) -> Element<'_, Message> {
        inspector::view(
            self.inspector_model(),
            &self.session.patch_info.authors,
            self.session.gestures.drop_row(),
            self.session.gestures.assign_drag().is_some() && !self.session.learn.is_on(),
            self.a11y.is_active(),
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
        // Each tab, and the row of them, is named for the window
        // (`places.rs`).
        let tabs = container(
            row![
                container(tab("Inspector", RightTab::Inspector)).id("panel/inspector"),
                container(tab("Matrix", RightTab::Matrix)).id("panel/matrix"),
            ]
            .spacing(4)
            .padding([6, 8]),
        )
        .id("panel");
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
        // Drawn from the model the accessibility tree reads too. Each item
        // sits in a container named by its UI ID (`places.rs`).
        let model = self.bars_model();
        let mut bar = row![].spacing(16).align_y(iced::Alignment::Center);
        if let Some(dev) = &model.dev {
            bar = bar
                .push(
                    container(
                        toggler(dev.measuring)
                            .label("Measure")
                            .on_toggle(Message::Measure)
                            .size(style::SWITCH)
                            .text_size(style::BODY),
                    )
                    .id(crate::bars::MEASURE_ID),
                )
                .push(
                    container(
                        toggler(dev.animating)
                            .label("Move camera")
                            .on_toggle(Message::Animate)
                            .size(style::SWITCH)
                            .text_size(style::BODY),
                    )
                    .id(crate::bars::MOVE_CAMERA_ID),
                )
                .push(bar_text(dev.frames.clone()));
        }
        // The audio items sit closer together than the rest of the bar.
        let shown = |item: &crate::bars::Item| -> Element<'static, Message> {
            let words = bar_text(item.text.clone());
            let words = match item.tone {
                Some(severity) => words.color(severity_colour(severity)),
                None => words,
            };
            let named = container(words).id(item.id);
            match &item.hover {
                Some(hover) => tooltip(
                    named,
                    container(text(hover.clone()).size(style::BODY))
                        .padding(6)
                        .style(container::rounded_box),
                    tooltip::Position::Top,
                )
                .into(),
                None => named.into(),
            }
        };
        let audio_names = ["audio", "load", "dropouts", "input", "silenced", "clipped", "telemetry"];
        let audio = iced::widget::Row::with_children(
            model
                .items
                .iter()
                .filter(|item| audio_names.contains(&item.name))
                .map(shown),
        )
        .spacing(10);
        for item in model.items.iter().filter(|item| item.name == "patch") {
            bar = bar.push(shown(item));
        }
        bar = bar.push(audio);
        for item in model
            .items
            .iter()
            .filter(|item| item.name == "midi" || item.name == "learning")
        {
            bar = bar.push(shown(item));
        }
        // The fixed items come first. The kept error and the status share
        // the remaining width, and are cut off rather than wrapped, so the
        // bar stays one line high. Hover shows all of it.
        container(
            bar.extend(
                model
                    .kept
                    .as_ref()
                    .map(|kept| status_field(kept, crate::bars::KEPT_ID, crate::bars::KEPT_DISMISS_ID)),
            )
            .push(status_field(
                &model.status,
                crate::bars::STATUS_ID,
                crate::bars::STATUS_DISMISS_ID,
            ))
            .padding([4, 10]),
        )
        .id("stats")
        .into()
    }
}

/// Hold note's hover text while it is disabled. The control channel refuses
/// the press with the same words.
pub(crate) const HOLD_DISABLED: &str = "Hold note is disabled: no audio is playing the engine";

/// Recent's hover text while it is disabled.
const RECENT_DISABLED: &str = "Recent is disabled: no patches opened or saved yet";

/// One message in the status bar: cut to its share of the width, all of it in
/// hover text, with a × if the player may dismiss it. `id` and `dismiss_id`
/// name the text and the × for the window (`places.rs`).
fn status_field<'a>(field: &crate::bars::Field, id: &'static str, dismiss_id: &'static str) -> Element<'a, Message> {
    row![tooltip(
        container(bar_text(field.shown.clone()).color(severity_colour(field.severity)))
            .id(id)
            .width(Length::Fill)
            .align_x(iced::Alignment::End)
            .clip(true),
        container(text(field.hover.clone()).size(style::BODY))
            .padding(6)
            .max_width(480)
            .style(container::rounded_box),
        tooltip::Position::Top,
    )]
    .extend(field.dismiss.map(|cause| {
        container(remove_button(field.dismiss_hover(), Message::DismissError(cause)))
            .id(dismiss_id)
            .into()
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
