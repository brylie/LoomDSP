use super::*;
use loom_canvas::tile_size as canvas_tile_size;
use loom_core::{EngineConfig, create_engine};

#[test]
fn places_every_module_without_overlap() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    generate(&mut parts.controller, 22).unwrap();
    let mut positions = BTreeMap::new();
    auto_layout(&parts.controller, &mut positions);
    assert_eq!(positions.len(), parts.controller.graph().len());
    let points: Vec<_> = positions.values().collect();
    for (i, a) in points.iter().enumerate() {
        for b in &points[i + 1..] {
            assert!(a != b, "two modules at {a:?}");
        }
    }
}

/// Whether the tile at `a` with size `sa` overlaps the one at `b`.
fn overlaps(a: Point, sa: iced::Size, b: Point, sb: iced::Size) -> bool {
    a.x < b.x + sb.width && b.x < a.x + sa.width && a.y < b.y + sb.height && b.y < a.y + sa.height
}

/// Asserts that no two tiles in the graph overlap.
fn assert_no_overlap(controller: &Controller, positions: &BTreeMap<u32, Point>) {
    let tiles: Vec<_> = controller
        .graph()
        .nodes()
        .map(|(id, node)| {
            let d = node.descriptor;
            let size = canvas_tile_size(d.inputs.len(), d.outputs.len(), d.panel.width, d.panel.height);
            (id.0, positions[&id.0], size)
        })
        .collect();
    for (i, (ia, pa, sa)) in tiles.iter().enumerate() {
        for (ib, pb, sb) in &tiles[i + 1..] {
            assert!(!overlaps(*pa, *sa, *pb, *sb), "tiles {ia} and {ib} overlap");
        }
    }
}

#[test]
fn wide_panels_do_not_overlap_the_next_column() {
    let mut parts = create_engine(crate::registry(true), EngineConfig::default());
    let c = &mut parts.controller;
    let wide = c.add_module("test.panel").unwrap();
    let next = c.add_module("loom.math.mul").unwrap();
    let after = c.add_module("loom.math.mul").unwrap();
    c.connect_named(wide, "uni", next, "a").unwrap();
    c.connect_named(next, "out", after, "a").unwrap();
    c.commit().unwrap();
    let mut positions = BTreeMap::new();
    auto_layout(c, &mut positions);
    assert_no_overlap(c, &positions);
}

/// A new engine with the built-in modules.
fn engine() -> loom_core::EngineParts {
    create_engine(loom_modules::registry(), EngineConfig::default())
}

/// Places every module of the graph and returns the positions.
fn place(controller: &Controller) -> BTreeMap<u32, Point> {
    let mut positions = BTreeMap::new();
    auto_layout(controller, &mut positions);
    positions
}

/// A chain sits on one row, each module one column right of the last
/// (rules 1 and 5).
#[test]
fn a_chain_sits_on_one_row() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let note = c.add_module("loom.midi.note").unwrap();
    let phase = c.add_module("loom.phase.generator").unwrap();
    let saw = c.add_module("loom.osc.saw").unwrap();
    let svf = c.add_module("loom.filter.svf").unwrap();
    c.connect_named(note, "pitch", phase, "pitch").unwrap();
    c.connect_named(phase, "phase", saw, "phase").unwrap();
    c.connect_named(saw, "out", svf, "in").unwrap();
    c.commit().unwrap();
    let p = place(c);
    let chain = [note, phase, saw, svf].map(|id| p[&id.0]);
    assert!(chain.iter().all(|q| q.y == chain[0].y), "{chain:?}");
    assert!(chain.windows(2).all(|w| w[0].x < w[1].x), "{chain:?}");
}

/// A loop is opened where the signal enters it, even when the loop's
/// modules have lower IDs than the module feeding it (rule 2).
#[test]
fn a_loop_opens_where_the_signal_enters() {
    let mut parts = engine();
    let c = &mut parts.controller;
    // Made first, so they have the lowest IDs.
    let delay = c.add_module("loom.time.delay").unwrap();
    let back = c.add_module("loom.time.delay1").unwrap();
    let add = c.add_module("loom.math.add").unwrap();
    let noise = c.add_module("loom.osc.noise").unwrap();
    c.connect_named(noise, "out", add, "a").unwrap();
    c.connect_named(add, "out", delay, "in").unwrap();
    c.connect_named(delay, "out", back, "in").unwrap();
    c.connect_named(back, "out", add, "b").unwrap();
    c.commit().unwrap();
    let p = place(c);
    let x = |id: ModuleId| p[&id.0].x;
    assert!(x(noise) < x(add), "the source comes first");
    assert!(x(add) < x(delay), "the loop starts where the noise enters");
    assert!(x(delay) < x(back), "only the return cable runs backwards");
}

/// The audio output goes in the last column, even when its source is
/// early in the patch (rule 3).
#[test]
fn the_audio_output_goes_last() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let note = c.add_module("loom.midi.note").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    let phase = c.add_module("loom.phase.generator").unwrap();
    let saw = c.add_module("loom.osc.saw").unwrap();
    c.connect_named(note, "gate", out, "in").unwrap();
    c.connect_named(note, "pitch", phase, "pitch").unwrap();
    c.connect_named(phase, "phase", saw, "phase").unwrap();
    c.commit().unwrap();
    let p = place(c);
    for id in [note, phase, saw] {
        assert!(p[&id.0].x < p[&out.0].x, "module {} is right of the output", id.0);
    }
}

/// A tile sits beside the source that feeds it, not in ID order:
/// the target of the top source goes on top (rule 4).
#[test]
fn a_tile_sits_beside_its_source() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let upper = c.add_module("loom.math.constant").unwrap();
    let lower = c.add_module("loom.math.constant").unwrap();
    // The lower source's target has the lower ID.
    let from_lower = c.add_module("loom.math.mul").unwrap();
    let from_upper = c.add_module("loom.math.mul").unwrap();
    c.connect_named(lower, "out", from_lower, "a").unwrap();
    c.connect_named(upper, "out", from_upper, "a").unwrap();
    c.commit().unwrap();
    let p = place(c);
    assert!(p[&upper.0].y < p[&lower.0].y);
    assert_eq!(p[&from_upper.0].y, p[&upper.0].y, "level with its source");
    assert!(p[&from_upper.0].y < p[&from_lower.0].y, "no crossing cables");
}

/// Modules that already have a position keep it, and new ones go
/// below them.
#[test]
fn placed_modules_stay_and_new_ones_go_below() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let a = c.add_module("loom.math.constant").unwrap();
    let b = c.add_module("loom.math.mul").unwrap();
    c.connect_named(a, "out", b, "a").unwrap();
    c.commit().unwrap();
    let mut positions = BTreeMap::from([(a.0, Point::new(240.0, 480.0))]);
    auto_layout(c, &mut positions);
    assert_eq!(positions[&a.0], Point::new(240.0, 480.0));
    assert!(positions[&b.0].y > 480.0, "{:?}", positions[&b.0]);
}

/// Every example patch is placed with no overlap and on the grid
/// (rule 6).
#[test]
fn example_patches_are_placed_cleanly() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "ron") {
            continue;
        }
        let mut parts = engine();
        loom_patch::Patch::load(&path)
            .unwrap()
            .load_into(&mut parts.controller)
            .unwrap();
        let positions = place(&parts.controller);
        assert_eq!(positions.len(), parts.controller.graph().len(), "{path:?}");
        assert_no_overlap(&parts.controller, &positions);
        for p in positions.values() {
            assert_eq!((p.x % GRID, p.y % GRID), (0.0, 0.0), "{path:?}: {p:?}");
        }
        checked += 1;
    }
    assert!(checked >= 5, "found {checked} example patches");
}

#[test]
fn generated_layout_has_no_overlap() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    generate(&mut parts.controller, 40).unwrap();
    let mut positions = BTreeMap::new();
    generated_layout(&parts.controller, &mut positions);
    assert_no_overlap(&parts.controller, &positions);
}

#[test]
fn generated_layout_is_on_the_grid() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    generate(&mut parts.controller, 300).unwrap();
    assert!(parts.controller.graph().len() >= 298);
    let mut positions = BTreeMap::new();
    generated_layout(&parts.controller, &mut positions);
    assert_eq!(positions.len(), parts.controller.graph().len());
    for p in positions.values() {
        assert_eq!(p.x % GRID, 0.0);
        assert_eq!(p.y % GRID, 0.0);
    }
}
