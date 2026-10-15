## What and why

<!-- What does this change do, and why? Plain language is best. -->

Related issue / requirement IDs:

Specification (for features): <!-- link to docs/design/..., and the acceptance criteria this meets -->

## Checklist

- [ ] Tests added or updated
- [ ] Behaviour matches the specification, or the specification is updated in this pull request
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all --check` pass
- [ ] Audio-thread code does not allocate, lock, or wait (see CONTRIBUTING.md)
- [ ] New dependencies are permissively licensed (ADR-0006)
- [ ] If AI tools helped, I have read, understood, and tested every line (see AI_POLICY.md)
