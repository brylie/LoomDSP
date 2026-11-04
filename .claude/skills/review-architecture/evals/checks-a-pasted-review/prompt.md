---
tags: [review-architecture]
max_turns: 40
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, Skill, Write, Bash]
---

A contractor sent us this architecture review of LoomDSP. Can you check it against the code and tell me which parts hold up before I file anything?

1. `App::no_overlay` in crates/loom-app/src/main.rs checks seven overlay fields and forgets `probe_menu` and `tile_menu`, while `App::view` checks all nine, so shortcuts can leak behind the context menus.
2. `Eq`, `Crossover`, and `Dynamics` in loom-modules declare `.smooth(...)` parameters but never implement `Module::param_glide`, so the engine steps when the first modulation assignment lands mid-glide.
3. `loom-analysis` depends on the whole `loom-core` crate just to read `safety::CEILING`.
4. `Patch::save` in loom-patch writes files with `std::fs`, which breaks the rule that the engine crates stay portable to wasm.
