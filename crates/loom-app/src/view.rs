//! The window: the toolbar, the canvas with its overlays, the right panel,
//! and the stats bar. Each is drawn from the app's parts; nothing here
//! changes state.

use crate::Element;
use crate::edits::ArrowGroup;
use crate::matrix::{Event as MatrixEvent, Tab as RightTab};
use crate::style::button;
use crate::{App, Message, inspector, matrix, pointer, style};
use iced::Length;
use iced::widget::{column, container, mouse_area, row, rule, space, stack, text, tooltip};
use loom_canvas::iced_convert::ToIced as _;
use loom_canvas::{DragLine, EditGroupView, PatchCanvas};

impl App {
    pub(crate) fn view(&self) -> Element<'_, Message> {
        let mut canvas = PatchCanvas::new(&self.session.scene, &self.session.selection, Message::Canvas)
            .drawn(&self.canvas_drawn)
            .clock(&self.clock)
            .display_scale(self.window_scale)
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
        // The module browser is the runtime's, in its Overlays layer, with
        // its press outside (row 5, `browser/panel.rs`).
        // The probe menu and Recent are the runtime's, in its Overlays
        // layer over everything Iced draws (row 3b, `menus/panel.rs`).
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
        if self.overlays.cable_menu().is_some()
            && let Some(menu) = self.menu_model()
        {
            let outside = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::CloseTileMenu)
                .on_right_press(Message::CloseTileMenu);
            let mut items = column![].spacing(2);
            if menu.items.is_empty() {
                items = items.push(iced::widget::text("No ports to connect").size(style::CONTROL));
            }
            for (index, item) in menu.items.into_iter().enumerate() {
                items = items.push(crate::inspector::ids::named(
                    &item.id,
                    button(iced::widget::text(item.label).size(style::CONTROL))
                        .on_press(item.message)
                        .width(Length::Fill)
                        .style(style::menu_item(menu.highlight == Some(index))),
                ));
            }
            let panel = column![
                iced::widget::text(menu.label).size(style::HEADING),
                iced::widget::scrollable(items).id(crate::places::widget_id(&crate::cable_picker::list_id()))
            ]
            .spacing(6);
            let panel = crate::inspector::ids::named(
                &menu.id,
                container(panel)
                    .padding(8)
                    .width(270)
                    .max_height(350)
                    .style(style::canvas_menu),
            );
            layers = layers
                .push(outside)
                .push(iced::widget::center(iced::widget::opaque(panel)));
        }
        if let Some(window) = self.preferences_view() {
            layers = layers.push(window);
        }
        // Your patches is the runtime's (row 5b, `patches/panel.rs`).
        // Over the Preferences window, which also opens it.
        if let Some(chooser) = &self.overlays.chooser {
            layers = layers.push(crate::licence_chooser::view(chooser.model()));
        }
        // Help is the runtime's, in its Overlays layer, over the dialogs
        // Iced draws, because F1 opens it from them (help-pages.md; row 6).
        // The question is the runtime's, in its Question layer, over
        // everything Iced draws (row 3b).
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
                    &self.session.level,
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
