---
tags: [review-architecture]
max_turns: 40
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, Skill, Write, Bash]
---

do an architecture review of crates/loom-patch for me. it's grown a bit and I want to know if it still respects the portable-crate rule and whether anything in there belongs somewhere else before we start on sub-patches
