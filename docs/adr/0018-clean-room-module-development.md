# ADR-0018: Clean-room module development

- **Status:** Accepted
- **Date:** 2026-10-28
- **Related requirements:** NFR-OSS-01, NFR-OSS-06, NFR-OSS-07, FR-LIB-11, FR-SDK-03
- **Related decisions:** ADR-0006 (dependency and licence policy), ADR-0017 (primitives, traits, and abstractions)
- **Plan:** [Module catalogue](../product/module-catalogue.md)

## Context

To plan the module library, we surveyed 15 other modular environments ([prior-art survey](../research/2026-10-prior-art-survey.md)). Studying prior art is normal and useful. But a module library is a place where it is easy to copy without meaning to:

- **Code.** Many modular projects publish source code. Some use copyleft licences (GPL), which ADR-0006 already forbids. Others use permissive licences, but copying from them still brings attribution duties, and code read once tends to come back in what we write.
- **Text.** Vendor manuals describe each module in a sentence or two. Those sentences are someone else's work. Reusing them, or paraphrasing them closely, in our help pages and descriptors would copy that work.
- **Design.** Panel layouts, control names, parameter sets, preset values, and graphics are designs. Copying them makes our modules look like clones and may infringe.
- **AI tools.** LoomDSP is built with heavy AI help (vision, principle 10). An AI agent asked to "implement a module like X" can reproduce code or text it has seen, without saying so.

ADR-0006 covers the licences of dependencies. It does not cover what contributors read, or what they copy from products that are not dependencies.

We want LoomDSP's modules to be our own, so that the project and every exported patch can be shared under a permissive licence without doubt.

## Options considered

1. **No rule beyond ADR-0006.** Simple. But it leaves manual text, design, and permissive code unaddressed, and gives AI agents no guidance.
2. **Ask for attribution when we borrow.** Allows more reuse. But it adds licence bookkeeping to every module, and it does not stop closely paraphrased text or cloned designs.
3. **Clean-room development.** We learn *that* a kind of module exists and *what it is for* from public descriptions. We design and build it from first principles, published theory, and our own tests, without reading other projects' code or copying their text or designs.

## Decision

Option 3. **LoomDSP's modules, their help text, and their panels are designed and written from first principles.**

What contributors may use:

- Public descriptions and demonstrations of other products, to learn that a kind of module exists and what it is for.
- Using other products as a musician: playing them and listening to them.
- Textbooks, papers, standards, and other published DSP theory, cited in the issue or pull request.
- Our own measurements and listening tests.
- The source code of a dependency accepted under ADR-0006, to use that dependency.

What contributors must not do:

- Read the source code of another audio project to write a LoomDSP module, whatever its licence. The exception is a dependency accepted under ADR-0006.
- Copy or closely paraphrase another product's manual or help text.
- Copy another product's panel layout, control names, parameter sets, preset values, or graphics.
- Ask an AI tool to reproduce any of the above.

Where product names may appear:

- In the [prior-art survey](../research/2026-10-prior-art-survey.md) and other dated research snapshots in `docs/research/`, in "Prior art" sections of ADRs, and in explainers in `docs/learn/` that compare how systems handle an idea.
- Not in the module catalogue, specifications' module and parameter names, module descriptors, or help pages. These use our own vocabulary.

How it is checked:

- The new-module issue form asks for the catalogue ID and for the design references (papers, textbooks, our own derivation).
- The pull request checklist asks the author to confirm the rules above.
- The agent guide (AGENTS.md) and the AI contribution policy repeat the rules for AI agents.

## Consequences

- **Clear ownership.** Modules, help text, and panels are LoomDSP's own, under Apache-2.0, and exported patches carry no hidden obligations.
- **Some work is slower.** Contributors cannot port an existing implementation and must work from theory. Good references in each issue make this easier, and the module test harness (FR-SDK-02) checks behaviour without needing anyone else's code.
- **Our own vocabulary.** Module types and parameters get plain, functional names. Users coming from other products may need a short mapping, which explainers can give.
- **Learning stays open.** Explainers can still compare LoomDSP with Eurorack, VCV Rack, or Bitwig Grid. Comparing ideas is not copying.
- **Honest limits.** This is a working practice, not a legal guarantee. When in doubt about a specific case, ask in the issue before building.
