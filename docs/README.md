# LoomDSP Documentation

This folder holds the guides, product requirements, and architecture decisions for LoomDSP.

Status: **Phase 1, early** (2026-10-14). Requirements are a draft and open for discussion.

## Guides

| Document | For | Purpose |
|---|---|---|
| [Getting started](guide/getting-started.md) | Musicians and testers | Run the desktop app, play a patch, and patch while it plays |
| [Architecture](dev/architecture.md) | Developers | How the crates, threads, compiler, and canvas fit together, and where to start |
| [Contributing](../CONTRIBUTING.md) | Developers | Build, test, hooks, CI, and pull requests |

## Product

| Document | Purpose |
|---|---|
| [Vision](product/vision.md) | Why LoomDSP exists, goals, non-goals, and success measures |
| [Personas and use cases](product/personas-and-use-cases.md) | Who we build for and what they need to do |
| [Functional requirements](product/requirements.md) | What the system must do, with IDs and priorities |
| [Non-functional requirements](product/non-functional-requirements.md) | Performance, real-time safety, portability, quality |
| [Roadmap](product/roadmap.md) | Phases, scope per phase, and exit criteria |
| [Open questions](product/open-questions.md) | Unresolved decisions and known risks |
| [Glossary](product/glossary.md) | Shared vocabulary |

## Architecture

| Document | Purpose |
|---|---|
| [ADR index](adr/README.md) | Architecture Decision Records and how to write them |

## Benchmarks

| Document | Purpose |
|---|---|
| [Phase 0 engine](benchmarks/p0-engine.md) | Feedback loop cost, example patch cost, live and offline stress results |
| [Phase 0 UI spikes](benchmarks/p0-ui-spikes.md) | Iced + iced_nodegraph and egui-snarl, measured on the reference laptop |
| [Phase 1 canvas](benchmarks/p1-canvas.md) | Our own canvas: how to measure it, and the results |

## How to read requirement IDs

Each requirement has an ID such as `FR-GRAPH-03` or `NFR-RT-01`.

- `FR` means functional requirement. `NFR` means non-functional requirement.
- The middle part is the area (for example `GRAPH`, `UI`, `WAM`).
- IDs are stable. If a requirement is dropped, mark it **Withdrawn** instead of reusing the ID. This keeps links from issues and commits valid.

Priorities use MoSCoW:

- **Must**: the phase fails without it.
- **Should**: important, but the phase can ship without it.
- **Could**: nice to have if time allows.
- **Won't (now)**: explicitly out of scope for this version.
