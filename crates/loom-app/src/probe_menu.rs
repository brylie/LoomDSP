//! The probe menu: right-click an output or a cable, then Add scope or Add
//! meter places that module next to it, already connected. Spec:
//! `docs/design/panels.md`, Add scope and Add meter.

use crate::style::{self, button};
use iced::widget::{column, container, text};
use iced::{Element, Length, Point, Rectangle, Size, Theme};
use loom_canvas::{GRID, PortRef, tile_size};
use loom_core::ModuleDescriptor;

/// Width of the menu.
const WIDTH: f32 = 140.0;
/// Height of one item.
const ITEM_HEIGHT: f32 = 26.0;
const PADDING: f32 = 4.0;
/// Height of the whole menu, for keeping it inside the canvas.
const HEIGHT: f32 = 2.0 * PADDING + 2.0 * ITEM_HEIGHT;

/// How far right of the output the new module's corner goes.
const GAP: f32 = 3.0 * GRID;
/// How far down the new module moves while it overlaps a tile.
const STEP: f32 = 2.0 * GRID;

/// What the menu adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// A scope (`loom.display.scope`).
    Scope,
    /// A level meter (`loom.display.meter`).
    Meter,
}

impl Probe {
    /// The module kind it adds.
    pub fn id(self) -> &'static str {
        match self {
            Self::Scope => "loom.display.scope",
            Self::Meter => "loom.display.meter",
        }
    }

    /// The menu item and undo step name, such as "add scope".
    pub fn verb(self) -> &'static str {
        match self {
            Self::Scope => "add scope",
            Self::Meter => "add meter",
        }
    }
}

/// What the menu asks of the app.
#[derive(Debug, Clone)]
pub enum Message {
    /// An item was chosen.
    Pick(Probe),
    /// Escape, or a click outside the menu.
    Close,
}

/// An open menu.
pub struct ProbeMenu {
    /// The output to probe.
    pub output: PortRef,
    /// Where it was opened, in world units: the output port, or the point
    /// clicked on a cable.
    pub world: Point,
    /// The menu's top-left corner, in window coordinates.
    at: Point,
}

impl ProbeMenu {
    /// Opens the menu at a window point, moved so it stays inside `area`.
    pub fn open(output: PortRef, world: Point, at: Point, area: Rectangle) -> Self {
        let x = at.x.min(area.x + area.width - WIDTH).max(area.x);
        let y = at.y.min(area.y + area.height - HEIGHT).max(area.y);
        Self {
            output,
            world,
            at: Point::new(x, y),
        }
    }

    /// The menu, placed at its corner. The app lays it over the window.
    pub fn view(&self) -> Element<'_, Message> {
        let item = |label: &'static str, probe: Probe| {
            button(text(label).size(style::CONTROL))
                .on_press(Message::Pick(probe))
                .width(Length::Fill)
                .height(ITEM_HEIGHT)
                .style(style::plain)
        };
        let menu = column![item("Add scope", Probe::Scope), item("Add meter", Probe::Meter)]
            .padding(PADDING)
            .width(WIDTH);
        let menu = container(menu).style(menu_style);
        iced::widget::pin(iced::widget::opaque(menu)).position(self.at).into()
    }
}

/// Where a new probe module goes: its top-left corner 3 grid units right of
/// `anchor`, with its first input as close to the anchor's height as the
/// grid allows, moved down in steps of 2 grid units until it overlaps none
/// of `tiles`.
pub fn place(descriptor: &ModuleDescriptor, anchor: Point, tiles: &[Rectangle]) -> Point {
    let size: Size = tile_size(
        descriptor.inputs.len(),
        descriptor.outputs.len(),
        descriptor.panel.width,
        descriptor.panel.height,
    );
    // The first input's centre, measured from the tile's top, as
    // `ModuleView::input_position` places it.
    let input = loom_canvas::HEADER + loom_canvas::ROW * 0.5;
    let start = loom_canvas::snap(Point::new(anchor.x + GAP, anchor.y - input));
    let mut corner = start;
    // Jump straight to the first step below a blocking tile: the same place
    // stepping would reach. Each jump passes one tile, so this ends after at
    // most one jump per tile. Far from the origin, f32 can no longer add a
    // step, so stop if a jump does not move.
    while let Some(tile) = tiles.iter().find(|tile| tile.intersects(&Rectangle::new(corner, size))) {
        let steps = ((tile.y + tile.height - start.y) / STEP).ceil();
        let next = start.y + steps * STEP;
        if next <= corner.y {
            break;
        }
        corner.y = next;
    }
    corner
}

fn menu_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.base.color.into()),
        border: iced::Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A free spot keeps the corner beside the anchor, with the input at
    /// the anchor's height when that lies on the grid.
    #[test]
    fn places_beside_the_anchor() {
        let anchor = Point::new(132.0, 30.0);
        let corner = place(&loom_modules::display::SCOPE, anchor, &[]);
        assert_eq!(corner, Point::new(168.0, 0.0));
    }

    /// A tile in the way pushes the new module down, in steps of 2 grid
    /// units, until nothing overlaps.
    #[test]
    fn moves_down_past_tiles() {
        let anchor = Point::new(132.0, 30.0);
        let blocker = Rectangle::new(Point::new(200.0, 0.0), Size::new(132.0, 60.0));
        let corner = place(&loom_modules::display::METER, anchor, &[blocker]);
        assert_eq!(corner, Point::new(168.0, 72.0));
        let descriptor = &loom_modules::display::METER;
        let size = tile_size(1, 1, descriptor.panel.width, descriptor.panel.height);
        assert!(!blocker.intersects(&Rectangle::new(corner, size)));
    }

    /// Tiles stacked below each other push the module past all of them.
    #[test]
    fn moves_down_past_a_stack_of_tiles() {
        let anchor = Point::new(132.0, 30.0);
        let column = |y: f32| Rectangle::new(Point::new(200.0, y), Size::new(132.0, 60.0));
        let tiles = [column(0.0), column(60.0), column(120.0)];
        let corner = place(&loom_modules::display::METER, anchor, &tiles);
        assert_eq!(corner, Point::new(168.0, 192.0));
    }

    /// Far from the origin, where f32 cannot add a step, placing still ends.
    #[test]
    fn placing_far_away_ends() {
        let anchor = Point::new(132.0, 1.0e9);
        let blocker = Rectangle::new(Point::new(150.0, 1.0e9 - 100.0), Size::new(132.0, 400.0));
        let corner = place(&loom_modules::display::SCOPE, anchor, &[blocker]);
        assert!(corner.y.is_finite());
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use crate::{App, Args, Message, tests};
    use loom_canvas::CanvasEvent;
    use loom_core::{InputRef, ModuleId, OutputRef};
    use std::path::PathBuf;

    /// The subtractive example patch, with no audio.
    fn app() -> App {
        let patch = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/patches/subtractive.ron"
        ));
        App::new(Args {
            patch: Some(patch),
            ..tests::app_args()
        })
    }

    fn area() -> Rectangle {
        Rectangle::new(Point::ORIGIN, Size::new(1000.0, 800.0))
    }

    /// The first cable of the patch, as the canvas names its source.
    fn first_cable(app: &App) -> (PortRef, InputRef) {
        let (from, to) = app.controller.graph().cables().next().expect("the patch has cables");
        let output = PortRef {
            module: from.module.0,
            output: true,
            index: from.port,
        };
        (output, to)
    }

    /// Opens the menu as a right-click on `output` would, at `world`.
    fn open(app: &mut App, output: PortRef, world: Point) {
        let _ = app.update(Message::Canvas(CanvasEvent::ProbeMenu {
            output,
            world,
            at: Point::new(100.0, 100.0),
            area: area(),
        }));
        assert!(app.probe_menu.is_some());
    }

    /// The new module's cable: from the probed output into its `in`.
    fn feeds(app: &App, module: u32, output: PortRef) -> bool {
        let source = OutputRef {
            module: ModuleId(output.module),
            port: output.index,
        };
        let input = InputRef {
            module: ModuleId(module),
            port: 0,
        };
        app.controller
            .graph()
            .cables()
            .any(|(from, to)| from == source && to == input)
    }

    /// Add scope and Add meter, on an output and on a cable, add a
    /// connected module beside it, keep the cable, select the new module,
    /// and overlap no tile. Undo takes the module and its cable back in one
    /// step (panels.md, acceptance criteria: scope module).
    #[test]
    fn add_scope_and_meter_on_an_output_and_a_cable() {
        for probe in [Probe::Scope, Probe::Meter] {
            for on_cable in [false, true] {
                let mut app = app();
                let (output, to) = first_cable(&app);
                let port = app.scene.port_position(output).unwrap();
                let world = if on_cable {
                    port + iced::Vector::new(20.0, 5.0)
                } else {
                    port
                };
                let modules = app.controller.graph().nodes().count();
                let cables = app.controller.graph().cables().count();
                let tiles: Vec<Rectangle> = app.scene.modules().iter().map(|m| m.bounds()).collect();

                open(&mut app, output, world);
                let _ = app.update(Message::ProbeMenu(super::Message::Pick(probe)));
                assert!(app.probe_menu.is_none());

                let graph = app.controller.graph();
                assert_eq!(graph.nodes().count(), modules + 1);
                assert_eq!(graph.cables().count(), cables + 1);
                let id = *app.selection.iter().next().expect("the new module is selected");
                assert_eq!(app.selection.len(), 1);
                assert_eq!(graph.node(ModuleId(id)).unwrap().descriptor.id, probe.id());
                assert!(feeds(&app, id, output), "{probe:?} is fed by the output");
                let source = OutputRef {
                    module: ModuleId(output.module),
                    port: output.index,
                };
                assert!(graph.cables().any(|c| c == (source, to)), "the clicked cable stays");
                let tile = app.scene.module(id).unwrap().bounds();
                assert!(tile.x > world.x, "the new tile is right of where the menu opened");
                assert!(!tiles.iter().any(|t| t.intersects(&tile)), "{probe:?} overlaps a tile");
                assert_eq!(
                    app.edits.history.undo_step().map(|s| s.name().to_string()).as_deref(),
                    Some(probe.verb())
                );

                app.undo();
                assert_eq!(app.controller.graph().nodes().count(), modules);
                assert_eq!(app.controller.graph().cables().count(), cables);
            }
        }
    }

    /// Escape and a click outside close the menu and add nothing. While it
    /// is open, other keys do not reach the canvas or the shortcuts.
    #[test]
    fn escape_and_a_click_outside_close_the_menu() {
        let mut app = app();
        let (output, _) = first_cable(&app);
        let port = app.scene.port_position(output).unwrap();
        let modules = app.controller.graph().nodes().count();
        let undo = app.edits.history.undo_step().map(|s| s.name().to_string());

        open(&mut app, output, port);
        let press = |key| {
            Message::Key(iced::keyboard::Event::KeyPressed {
                key,
                modified_key: iced::keyboard::Key::Unidentified,
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers: iced::keyboard::Modifiers::default(),
                text: None,
                repeat: false,
            })
        };
        let _ = app.update(press(iced::keyboard::Key::Character("a".into())));
        assert!(
            app.probe_menu.is_some() && app.browser.is_none(),
            "A does not open the browser"
        );
        let _ = app.update(press(iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)));
        assert!(app.probe_menu.is_none());

        open(&mut app, output, port);
        let _ = app.update(Message::ProbeMenu(super::Message::Close));
        assert!(app.probe_menu.is_none());
        assert_eq!(app.controller.graph().nodes().count(), modules);
        assert_eq!(app.edits.history.undo_step().map(|s| s.name().to_string()), undo);
    }

    /// Cut and duplicate do not change the patch behind the open menu.
    #[test]
    fn clipboard_keys_do_nothing_while_the_menu_is_open() {
        let mut app = app();
        let (output, _) = first_cable(&app);
        let port = app.scene.port_position(output).unwrap();
        let _ = app.update(Message::Canvas(CanvasEvent::Select(vec![output.module])));
        let modules = app.controller.graph().nodes().count();
        open(&mut app, output, port);
        let _ = app.update(tests::command_key("d"));
        let _ = app.update(tests::command_key("x"));
        assert_eq!(app.controller.graph().nodes().count(), modules);
        assert!(app.probe_menu.is_some());
    }

    /// If the output's module was removed while the menu was open, picking
    /// an item adds nothing, makes no step, and says why.
    #[test]
    fn a_removed_output_adds_nothing() {
        let mut app = app();
        let (output, _) = first_cable(&app);
        let port = app.scene.port_position(output).unwrap();
        open(&mut app, output, port);
        let _ = app.update(Message::Canvas(CanvasEvent::Delete(vec![output.module])));
        let modules = app.controller.graph().nodes().count();
        let undo = app.edits.history.undo_step().map(|s| s.name().to_string());
        let _ = app.update(Message::ProbeMenu(super::Message::Pick(Probe::Scope)));
        assert_eq!(app.controller.graph().nodes().count(), modules);
        assert_eq!(app.edits.history.undo_step().map(|s| s.name().to_string()), undo);
        assert!(app.status.contains("the output is gone"), "{}", app.status);
    }

    /// The menu stays inside the canvas when opened near its edge.
    #[test]
    fn the_menu_stays_inside_the_canvas() {
        let output = PortRef {
            module: 1,
            output: true,
            index: 0,
        };
        let menu = ProbeMenu::open(output, Point::ORIGIN, Point::new(990.0, 790.0), area());
        assert!(
            menu.at.x + WIDTH <= 1000.0 && menu.at.y + HEIGHT <= 800.0,
            "{:?}",
            menu.at
        );
    }
}
