//! NFR-RT-06 for the transport: filling transport buffers, reading
//! `ProcessContext::transport`, and applying transport changes never
//! allocate or free on the audio thread.
//!
//! This test binary installs an allocator that aborts if any allocation or
//! deallocation happens inside `assert_no_alloc`. The check is active in
//! debug builds, which is how `cargo test` runs. `assert_no_alloc` has no
//! `AllocDisabler` in release builds, so this file is empty there: a release
//! build could not abort on allocation anyway.

#![cfg(debug_assertions)]

mod common;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use common::*;
use loom_core::{ProbeKind, TimeSignature};

#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

#[test]
fn transport_never_allocates_on_the_audio_thread() {
    let mut parts = engine(128);
    let mut left = vec![0.0; 1024];
    let mut right = vec![0.0; 1024];
    let mut followers = Vec::new();
    for i in 0..300usize {
        // Control thread: allocation allowed here.
        if i % 7 == 0 {
            let id = parts.controller.add_module("test.follower").unwrap();
            if i % 2 == 0 {
                // In a feedback loop, so it runs a frame at a time.
                parts.controller.connect_named(id, "feedback", id, "feedback").unwrap();
            }
            let bar = parts.controller.output_ref(id, "ctx_bar").unwrap();
            parts.controller.add_probe(bar, ProbeKind::MeterAndScope).unwrap();
            followers.push(id);
        }
        if i % 11 == 0
            && followers.len() > 3
            && let Some(id) = followers.pop()
        {
            parts.controller.remove_module(id).unwrap();
        }
        match i % 5 {
            0 => {
                parts.controller.set_tempo(40.0 + i as f64).unwrap();
            }
            1 => parts.controller.set_running(i % 3 != 0).unwrap(),
            2 => parts.controller.reset_transport().unwrap(),
            3 => parts.controller.set_time_signature(TimeSignature::new(3, 4)).unwrap(),
            _ => {}
        }
        parts.controller.commit().unwrap();
        parts.telemetry.meters().for_each(drop);
        parts.telemetry.scope().for_each(drop);
        let frames = [1, 32, 128, 129, 1024][i % 5];

        // Audio thread: no allocation allowed.
        assert_no_alloc(|| {
            parts.engine.process(&mut left[..frames], &mut right[..frames]);
        });
    }
}
