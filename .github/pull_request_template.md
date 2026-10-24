## What and why

<!-- What does this change do, and why? Plain language is best. -->

Related issue / requirement IDs:

Specification (for features): <!-- link to docs/design/..., and the acceptance criteria this meets -->

## Checklist

- [ ] Tests added or updated
- [ ] Behaviour matches the specification, or the specification is updated in this pull request
- [ ] `mise run check` passes (format, Clippy, tests with at least 80% line coverage, docs, WebAssembly, licences, spelling)
- [ ] Audio-thread code does not allocate, lock, or wait (see CONTRIBUTING.md)
- [ ] New dependencies are permissively licensed (ADR-0006)
- [ ] Clean room (ADR-0018): no other audio project's source code was read, except to use a dependency accepted under ADR-0006, and no manual text, panel layout, or presets were copied. New modules name their catalogue ID and cite their design references
