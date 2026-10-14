//! Builds the canvas scene from the engine graph.

use iced::{Color, Point};
use loom_canvas::{CableView, ModuleView, PortShape, PortView, Scene};
use loom_core::{Controller, InputRef, PortSpec, SignalType};
use std::collections::{BTreeMap, BTreeSet};

/// Port and cable colour for a signal type (ADR-0007).
pub fn signal_colour(signal: SignalType) -> Color {
    let [r, g, b] = signal.colour();
    Color::from_rgb8(r, g, b)
}

/// Marker shape for a signal type. Pitch and gate are square, so they differ
/// from audio and modulation by shape as well as colour (NFR-UX-03).
fn signal_shape(signal: SignalType) -> PortShape {
    match signal {
        SignalType::Pitch | SignalType::Gate => PortShape::Square,
        SignalType::Audio | SignalType::Modulation | SignalType::Phase => PortShape::Round,
    }
}

/// Header colour by module category. Muted, so cable colours stand out.
pub fn category_colour(category: &str) -> Color {
    match category {
        "Phase" => Color::from_rgb8(0x5A, 0x4A, 0x82),
        "Oscillator" => Color::from_rgb8(0x8A, 0x45, 0x45),
        "Filter" => Color::from_rgb8(0x3D, 0x68, 0x86),
        "Envelope" => Color::from_rgb8(0x3E, 0x7A, 0x58),
        "MIDI" => Color::from_rgb8(0x86, 0x6A, 0x3A),
        "I/O" => Color::from_rgb8(0x6E, 0x6E, 0x78),
        "Time" => Color::from_rgb8(0x3E, 0x76, 0x78),
        _ => Color::from_rgb8(0x50, 0x54, 0x60),
    }
}

fn port_view(spec: &PortSpec) -> PortView {
    PortView {
        label: spec.name.to_string(),
        colour: signal_colour(spec.signal),
        shape: signal_shape(spec.signal),
    }
}

/// Builds the scene for the controller's graph. Modules without a position
/// are placed at the origin; call [`crate::layout::auto_layout`] first.
pub fn build(controller: &Controller, positions: &BTreeMap<u32, Point>) -> Scene {
    let graph = controller.graph();
    let delayed: BTreeSet<InputRef> = controller.last_report().inserted_delays().map(|(_, to)| *to).collect();

    let modules = graph
        .nodes()
        .map(|(id, node)| {
            let d = node.descriptor;
            ModuleView {
                id: id.0,
                title: d.name.to_string(),
                accent: category_colour(d.category),
                position: positions.get(&id.0).copied().unwrap_or(Point::ORIGIN),
                inputs: d.inputs.iter().map(port_view).collect(),
                outputs: d.outputs.iter().map(port_view).collect(),
            }
        })
        .collect();

    let cables = graph
        .cables()
        .filter_map(|(from, to)| {
            let source = graph.node(from.module)?.descriptor.outputs.get(from.port)?;
            let target = graph.node(to.module)?.descriptor.inputs.get(to.port)?;
            Some(CableView {
                from: (from.module.0, from.port),
                to: (to.module.0, to.port),
                colour_from: signal_colour(source.signal),
                colour_to: signal_colour(target.signal),
                delayed: delayed.contains(&to),
            })
        })
        .collect();

    Scene::new(modules, cables)
}
