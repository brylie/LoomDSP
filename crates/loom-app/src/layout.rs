//! Automatic placement for modules without a saved position, and a
//! generator for large test patches.

use iced::Point;
use loom_canvas::{GRID, TILE_WIDTH, tile_size};
use loom_core::{Controller, LoomError, ModuleId, ModuleRole, Node};
use std::cmp::{Ordering, Reverse};
use std::collections::BTreeMap;

/// Horizontal gap between layout columns, for cables.
const COLUMN_GAP: f32 = 4.0 * GRID;
/// Vertical gap between tiles in a column.
const GAP: f32 = 2.0 * GRID;

/// Size of a module's tile in world units.
fn size_of(node: &Node) -> iced::Size {
    let d = node.descriptor;
    if node.unknown.is_some() {
        tile_size(node.input_count(), node.output_count(), 10, 4)
    } else {
        tile_size(d.inputs.len(), d.outputs.len(), d.panel.width, d.panel.height)
    }
}

/// Left edge of each column: every column is as wide as its widest tile,
/// plus a gap for cables, so tiles in neighbouring columns never overlap.
/// Columns without a tile are `TILE_WIDTH` wide.
fn column_starts(widths: &[f32]) -> Vec<f32> {
    let mut x = 0.0;
    widths
        .iter()
        .map(|w| {
            let start = x;
            x += w + COLUMN_GAP;
            start
        })
        .collect()
}

/// Each module's sources, as `(input index, source module)` in input
/// order. Cables from a module to itself are left out.
type Sources = BTreeMap<u32, Vec<(usize, u32)>>;

/// Places modules without a position, by the rules in
/// `docs/design/auto-placement.md`.
///
/// Columns run left to right by signal flow ([`columns_by_flow`]), and
/// audio outputs go in the last one. Each column is as wide as its widest
/// tile (even tiles that already have a position count), so a wide panel
/// never overlaps the next column. Within a column, tiles are sorted by the
/// average height of their placed sources, and each one lines up with its
/// main source (the one feeding its first connected input) when there is
/// room. New modules go below the modules that already have positions, so
/// nothing overlaps.
pub fn auto_layout(controller: &Controller, positions: &mut BTreeMap<u32, Point>) {
    let graph = controller.graph();
    let mut sources: Sources = graph.nodes().map(|(id, _)| (id.0, Vec::new())).collect();
    for (from, to) in graph.cables() {
        if from.module != to.module
            && let Some(list) = sources.get_mut(&to.module.0)
        {
            list.push((to.port, from.module.0));
        }
    }
    for list in sources.values_mut() {
        list.sort_unstable();
    }
    let mut depth = columns_by_flow(&sources);

    // Rule 3: audio outputs go in the last column, one right of everything
    // else.
    let is_output = |id: u32| {
        graph
            .node(ModuleId(id))
            .is_some_and(|node| matches!(node.descriptor.role, ModuleRole::AudioOutput))
    };
    let last = depth
        .iter()
        .filter(|(id, _)| !is_output(**id))
        .map(|(_, d)| d + 1)
        .max()
        .unwrap_or(0);
    for (id, d) in depth.iter_mut() {
        if is_output(*id) {
            *d = last;
        }
    }

    let columns = depth.values().max().map_or(0, |d| d + 1);
    let mut widths = vec![TILE_WIDTH; columns];
    let mut by_column: Vec<Vec<u32>> = vec![Vec::new(); columns];
    for (id, node) in graph.nodes() {
        let column = depth.get(&id.0).copied().unwrap_or(0);
        widths[column] = widths[column].max(size_of(node).width);
        if !positions.contains_key(&id.0) {
            by_column[column].push(id.0);
        }
    }
    let starts = column_starts(&widths);

    let top = positions.values().map(|p| p.y + 10.0 * GRID).fold(0.0, f32::max);
    for (column, ids) in by_column.into_iter().enumerate() {
        // Rule 4: near the tiles that feed it. Tiles with no placed source
        // come last, by ID.
        let mut keyed: Vec<(Option<f32>, u32)> = ids
            .into_iter()
            .map(|id| (mean_source_y(&sources[&id], positions), id))
            .collect();
        keyed.sort_by(|(a, a_id), (b, b_id)| match (a, b) {
            (Some(a), Some(b)) => a.total_cmp(b).then(a_id.cmp(b_id)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => a_id.cmp(b_id),
        });
        let mut next_y = top;
        for (_, id) in keyed {
            // Rule 5: level with the main source, or lower if a tile above
            // is in the way.
            let main = sources[&id].iter().find_map(|(_, s)| positions.get(s)).map(|p| p.y);
            let y = main.map_or(next_y, |y| y.max(next_y));
            positions.insert(id, Point::new(starts[column], y));
            let height = graph.node(ModuleId(id)).map_or(0.0, |node| size_of(node).height);
            next_y = y + height + GAP;
        }
    }
}

/// The column of every module: one right of its furthest-right source,
/// and 0 for a module with no sources (rule 1).
///
/// A module is given its column once all of its sources have one. When
/// every module left is waiting on a feedback loop, the next one is the
/// module with the most sources already placed, lowest ID on a tie, so the
/// loop is opened where the signal enters it (rule 2).
fn columns_by_flow(sources: &Sources) -> BTreeMap<u32, usize> {
    let mut depth: BTreeMap<u32, usize> = BTreeMap::new();
    while depth.len() < sources.len() {
        let next = {
            let placed = |list: &[(usize, u32)]| list.iter().filter(|(_, s)| depth.contains_key(s)).count();
            let waiting = sources.iter().filter(|(id, _)| !depth.contains_key(id));
            waiting
                .clone()
                .find(|(_, list)| placed(list) == list.len())
                .or_else(|| waiting.max_by_key(|(id, list)| (placed(list), Reverse(**id))))
                .map(|(id, _)| *id)
        };
        let Some(id) = next else {
            break;
        };
        let d = sources[&id]
            .iter()
            .filter_map(|(_, s)| depth.get(s))
            .map(|d| d + 1)
            .max()
            .unwrap_or(0);
        depth.insert(id, d);
    }
    depth
}

/// The average top of the sources in `list` that already have a position,
/// or `None` if none has.
fn mean_source_y(list: &[(usize, u32)], positions: &BTreeMap<u32, Point>) -> Option<f32> {
    let ys: Vec<f32> = list.iter().filter_map(|(_, s)| positions.get(s)).map(|p| p.y).collect();
    (!ys.is_empty()).then(|| ys.iter().sum::<f32>() / ys.len() as f32)
}

/// Builds about `n` modules in voices of five: note, phase, saw, filter,
/// gain. Only the first voice is audible. For performance tests.
pub fn generate(controller: &mut Controller, n: usize) -> Result<(), LoomError> {
    let c = controller;
    let out = c.add_module("loom.io.audio-out")?;
    let note = c.add_module("loom.midi.note")?;
    let mut made = 2;
    let mut voice = 0;
    while made + 4 <= n {
        let phase = c.add_module("loom.phase.generator")?;
        let saw = c.add_module("loom.osc.saw")?;
        let svf = c.add_module("loom.filter.svf")?;
        let mul = c.add_module("loom.math.mul")?;
        c.connect_named(note, "pitch", phase, "pitch")?;
        c.connect_named(phase, "phase", saw, "phase")?;
        c.connect_named(saw, "out", svf, "in")?;
        c.connect_named(svf, "lowpass", mul, "a")?;
        c.connect_named(note, "gate", mul, "b")?;
        c.set_param_named(mul, "gain", if voice == 0 { 0.2 } else { 0.0 })?;
        if voice == 0 {
            c.connect_named(mul, "out", out, "in")?;
        }
        made += 4;
        voice += 1;
    }
    c.commit()?;
    Ok(())
}

/// Places voices of a generated patch in a grid of rows, one voice per row,
/// wrapping into blocks so the patch is roughly as wide as it is tall.
pub fn generated_layout(controller: &Controller, positions: &mut BTreeMap<u32, Point>) {
    let ids: Vec<u32> = controller.graph().nodes().map(|(id, _)| id.0).collect();
    let row_height = 6.0 * GRID + GAP;
    // One pitch for every column, wide enough for the widest tile.
    let widest = controller
        .graph()
        .nodes()
        .map(|(_, node)| size_of(node).width)
        .fold(TILE_WIDTH, f32::max);
    let column = widest + COLUMN_GAP;
    // The first two modules (output and note) sit on the left.
    for (i, id) in ids.iter().take(2).enumerate() {
        positions.insert(*id, Point::new(0.0, i as f32 * row_height));
    }
    let voices: Vec<&[u32]> = ids[2.min(ids.len())..].chunks(4).collect();
    let per_block = ((voices.len() as f32).sqrt() * 1.6).ceil().max(1.0) as usize;
    for (v, voice) in voices.iter().enumerate() {
        let block = v / per_block;
        let row = v % per_block;
        for (j, id) in voice.iter().enumerate() {
            let x = column * (1.5 + (block * 5 + j) as f32);
            positions.insert(*id, Point::new((x / GRID).round() * GRID, row as f32 * row_height));
        }
    }
}

#[cfg(test)]
mod tests {
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
        let mut registry = loom_core::Registry::new();
        loom_modules::register_all(&mut registry).unwrap();
        loom_test::modules::register(&mut registry).unwrap();
        let mut parts = create_engine(std::sync::Arc::new(registry), EngineConfig::default());
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
}
