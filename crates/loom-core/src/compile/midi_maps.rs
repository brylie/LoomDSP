//! MIDI mappings compiled into the operations the engine applies.

use super::*;

/// The graph's MIDI mappings, sorted by control so the engine can find a
/// control change's mapping with a binary search.
pub(super) fn compile_midi_maps(graph: &Graph, synced: &BTreeMap<MidiMappingId, u32>) -> Vec<MidiMapOp> {
    let mut maps: Vec<MidiMapOp> = graph
        .midi_mappings
        .iter()
        .filter_map(|(&id, mapping)| {
            let node = graph.nodes.get(&mapping.target)?;
            Some(MidiMapOp {
                control: mapping.control,
                id,
                synced: synced.get(&id).copied(),
                slot: node.slot? as u32,
                param: u16::try_from(mapping.param).ok()?,
                spec: node.spec(mapping.param)?,
            })
        })
        .collect();
    maps.sort_by_key(|map| map.control);
    maps
}
