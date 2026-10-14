//! Automatic placement for modules without a saved position, and a
//! generator for large test patches.

use iced::Point;
use loom_canvas::{GRID, TILE_WIDTH};
use loom_core::{Controller, LoomError};
use std::collections::BTreeMap;

/// Horizontal distance between layout columns: a tile plus a gap for cables.
const COLUMN: f32 = TILE_WIDTH + 4.0 * GRID;
/// Vertical gap between tiles in a column.
const GAP: f32 = 2.0 * GRID;

/// Places modules without a position in columns by signal-flow depth.
///
/// Layering with cycle breaking: a module is placed once all of its sources
/// are placed. When only cycles remain, the lowest-ID unplaced module goes
/// next. Its column is one right of its furthest-right source. New modules
/// go below the modules that already have positions, so nothing overlaps.
pub fn auto_layout(controller: &Controller, positions: &mut BTreeMap<u32, Point>) {
    let graph = controller.graph();
    let mut sources: BTreeMap<u32, Vec<u32>> = graph.nodes().map(|(id, _)| (id.0, Vec::new())).collect();
    for (from, to) in graph.cables() {
        if from.module != to.module
            && let Some(list) = sources.get_mut(&to.module.0)
        {
            list.push(from.module.0);
        }
    }
    let mut depth: BTreeMap<u32, usize> = BTreeMap::new();
    while depth.len() < sources.len() {
        let Some(ready) = sources
            .iter()
            .find(|(id, srcs)| !depth.contains_key(id) && srcs.iter().all(|s| depth.contains_key(s)))
            .or_else(|| sources.iter().find(|(id, _)| !depth.contains_key(id)))
            .map(|(id, _)| *id)
        else {
            break;
        };
        let d = sources[&ready]
            .iter()
            .filter_map(|s| depth.get(s))
            .map(|d| d + 1)
            .max()
            .unwrap_or(0);
        depth.insert(ready, d);
    }

    let top = positions.values().map(|p| p.y + 10.0 * GRID).fold(0.0, f32::max);
    let mut next_y: BTreeMap<usize, f32> = BTreeMap::new();
    for (id, node) in graph.nodes() {
        if positions.contains_key(&id.0) {
            continue;
        }
        let column = depth.get(&id.0).copied().unwrap_or(0);
        let y = next_y.entry(column).or_insert(top);
        positions.insert(id.0, Point::new(column as f32 * COLUMN, *y));
        let rows = node.descriptor.inputs.len().max(node.descriptor.outputs.len()).max(1) as f32;
        let height = ((22.0 + rows * 16.0 + 6.0) / GRID).ceil() * GRID;
        *y += height + GAP;
    }
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
            c.connect_named(mul, "out", out, "left")?;
            c.connect_named(mul, "out", out, "right")?;
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
            let x = COLUMN * (1.5 + (block * 5 + j) as f32);
            positions.insert(*id, Point::new((x / GRID).round() * GRID, row as f32 * row_height));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
