# ADR-0027: Prebuilt release downloads from a tagged GitHub Actions build

- **Status:** Proposed
- **Date:** 2026-10-25
- **Related requirements:** NFR-PORT-01, NFR-OSS-03
- **Specification:** [Release downloads](../design/releases.md)

## Context

Today the only way to run LoomDSP is to install Rust and build it from source. That is fine for contributors. It stops most musicians, sound designers, and modular fans, who want to download an app and play. The README and the website are written for them, so the download step has to match.

This is not the plugin and standalone export that the roadmap defers (ADR-0004, "Later"). That work turns a patch into a plugin or an app. This decision only ships the existing desktop app as a file people can download.

Constraints:

- CI already builds and tests on Linux, macOS (Apple Silicon), and Windows, so all three targets compile.
- The project is early alpha. Nothing is released, and breaking changes are expected.
- Real code signing needs paid certificates (an Apple Developer ID for notarisation, and a Windows signing certificate or a signing service). The project does not have them yet.
- Every dependency must stay permissive (ADR-0006), and a release must be reproducible from a tag.

## Options considered

1. **Build from source only (today).** No cost, but only people with a Rust toolchain can use it.
2. **A release workflow that builds each target on its own runner and attaches archives to a GitHub release.** Needs no servers or accounts beyond GitHub. Unsigned builds show a warning on first launch.
3. **Native installers and signing from the start** (a notarised macOS disk image, a Windows installer, a Linux AppImage). The best first launch, but it needs paid certificates and more build code before anyone can download anything.
4. **Distribute through package managers** (Homebrew, winget, Flathub). Reaches people where they already install software, but each needs a stable release to point at, so it follows option 2.

## Decision

Option 2. A tag that starts with `v` triggers a GitHub Actions workflow. It builds the desktop app and the `loom` command-line tool on macOS, Windows, and Linux, packs each into a plain archive (a zip for macOS and Windows, a tar.gz for Linux), writes SHA-256 checksums, and attaches everything to a GitHub release. Tags with a pre-release suffix, such as `v0.1.0-alpha.1`, publish as pre-releases.

Builds are unsigned for now. Every build goes through one packaging script, so signing can be added there later without a design change. Installers (option 3) and package managers (option 4) come later, on top of the same archives.

## Consequences

- Non-developers can download, unzip, and run LoomDSP. The README, the getting-started guide, and the website lead with downloads.
- Unsigned builds make macOS Gatekeeper and Windows SmartScreen warn on first launch. The docs explain how to open the app, and say why.
- A release is one pushed tag. The same workflow, run by hand, does a dry run that builds and uploads artifacts but publishes nothing.
- The Linux build runs on an older Ubuntu runner so the binary works with the C library on more distributions.
- Each release must be checked by a person on each OS, because audio devices cannot be tested in CI (see the spec).
- Early alpha means a downloaded build can stop opening old patches. Release notes list every break (AGENTS.md).
