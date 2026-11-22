//! The window: the toolbar, the canvas with its overlays, the right panel,
//! and the stats bar. Each is drawn from the app's parts; nothing here
//! changes state.

use crate::Element;
use crate::edits::ArrowGroup;
use crate::matrix::{Event as MatrixEvent, Tab as RightTab};
use crate::style::button;
use crate::{App, Message, browser, inspector, matrix, pointer, probe_menu, recent, style};
use iced::Length;
use iced::widget::{column, container, mouse_area, row, rule, space, stack, text, tooltip};
use loom_canvas::iced_convert::ToIced as _;
use loom_canvas::{DragLine, EditGroupView, PatchCanvas};

impl App {
    pub(crate) fn view(&self) -> Element<'_, Message> {
        let mut canvas = PatchCanvas::new(&self.session.scene, &self.session.selection, Message::Canvas)
            .drawn(&self.canvas_drawn)
            .clock(&self.clock)
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
            .report_typed(self.a11y.is_active())
            .typed_request(self.a11y.typed_request().0, self.a11y.typed_request().1)
            .overview(self.map_view())
            .centre_request(self.session.outline.centre.0, self.session.outline.centre.1)
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
        // The toolbar and the breadcrumb are `loom-ui`'s (`chrome.rs`): Iced
        // keeps their place, empty, at the same height.
        let main = column![
            space().height(style::top_height(!self.session.level.is_root())),
            body,
            rule::horizontal(1),
            // The stats bar is `loom-ui`'s (`stats_bar.rs`): Iced keeps its
            // place, empty, at the same height.
            space().height(style::STATS_BAR_HEIGHT)
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
            layers = layers
                .push(outside)
                .push(menu.view(self.overlays.menu_highlight()).map(Message::ProbeMenu));
        }
        if let Some((module, at)) = self.overlays.tile_menu() {
            let outside = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::CloseTileMenu)
                .on_right_press(Message::CloseTileMenu);
            // Drawn from the menu's model, which its keys and the tree read
            // too (`menus.rs`); each item is named by its UI ID.
            let mut items = column![].spacing(2);
            let model = self.menu_model();
            let highlight = model.as_ref().and_then(|menu| menu.highlight);
            for (at, item) in model.iter().flat_map(|menu| menu.items.iter()).enumerate() {
                items = items.push(crate::inspector::ids::named(
                    &item.id,
                    button(iced::widget::text(item.label.clone()).size(style::CONTROL))
                        .on_press(item.message.clone())
                        .width(200)
                        .style(style::menu_item(highlight == Some(at))),
                ));
            }
            let menu = crate::inspector::ids::named(
                &crate::menus::tile_id(module),
                container(items).padding(4).style(style::canvas_menu),
            );
            layers = layers
                .push(outside)
                .push(iced::widget::pin(iced::widget::opaque(menu)).position(at.to_iced()));
        }
        if let Some(rows) = self.overlays.recent_rows() {
            let outside = mouse_area(space::horizontal().width(Length::Fill).height(Length::Fill))
                .on_press(Message::Recent(recent::Message::Close))
                .on_right_press(Message::Recent(recent::Message::Close));
            layers = layers.push(outside).push(
                iced::widget::pin(iced::widget::opaque(
                    recent::view(rows, self.overlays.menu_highlight()).map(Message::Recent),
                ))
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
            layers = layers.push(crate::licence_chooser::view(chooser.model()));
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
        let listeners = self.listeners();
        let on_input = move |event: iced::Event, status| listeners.messages(&event, status);
        pointer::Injector::new(layers, self, on_input).into()
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
            &self.session.patch_info,
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
                container(tab("Outline", RightTab::Outline)).id("panel/outline"),
            ]
            .spacing(4)
            .padding([6, 8]),
        )
        .id("panel");
        let panel = self.jump_view().unwrap_or_else(|| match self.right_tab {
            RightTab::Outline => self.outline_view(),
            RightTab::Inspector => self.inspector(),
            RightTab::Matrix => matrix::view(
                self.session.matrix.model(
                    self.session.controller.graph(),
                    &self.session.hierarchy,
                    &self.session.selection,
                    self.right_tab,
                ),
                self.a11y.is_active(),
            ),
        });
        column![tabs, panel].width(280).height(Length::Fill).into()
    }
}

/// Hold note's hover text while it is disabled. The control channel refuses
/// the press with the same words.
pub(crate) const HOLD_DISABLED: &str = "Hold note is disabled: no audio is playing the engine";

/// Recent's hover text while it is disabled.
const RECENT_DISABLED: &str = "Recent is disabled: no patches opened or saved yet";

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
