# Release Downloads Specification

How a tagged version of LoomDSP becomes downloads for macOS, Windows, and Linux, for people who do not build from source.

- **Kind:** living specification.
- **Status:** Draft
- **Decision behind it:** [ADR-0027](../adr/0027-prebuilt-release-downloads.md)
- **Use case and requirements:** NFR-PORT-01 (the app builds on all three systems), NFR-OSS-03.
- **Code:** `.github/workflows/release.yml`, `scripts/bundle-macos.sh`, `scripts/package-release.sh`.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

A musician opens the GitHub releases page, downloads one file for their computer, unpacks it, and opens LoomDSP. They never install Rust.

A maintainer pushes one tag, and the downloads appear on the release page with checksums.

## Behaviour

### Tags and releases

| Tag | Release kind |
|---|---|
| `v0.1.0-alpha.1` (any tag with a `-` suffix) | Pre-release |
| `v1.0.0` (no suffix) | Full release |

The tag, without the `v` and without any `-` suffix, must equal the `version` in the workspace `Cargo.toml`. The workflow stops if they differ. So `v0.1.0-alpha.1` needs version `0.1.0`. *Why:* the version shown in the app must not disagree with the tag. The suffix is a release label and does not change the code version.

The file names use the whole tag without the `v` (`0.1.0-alpha.1`), so the downloads say which release they are from.

### Downloads

Each release carries these files, with `VERSION` as the tag without the `v`.

| File | Contents |
|---|---|
| `LoomDSP-VERSION-macos-arm64.zip` | `LoomDSP.app` for Apple Silicon, `examples/`, `LICENSE`, `README.txt` |
| `LoomDSP-VERSION-macos-x86_64.zip` | The same for Intel Macs |
| `LoomDSP-VERSION-windows-x86_64.zip` | `loom-app.exe`, `loom.exe`, `examples/`, `LICENSE`, `README.txt` |
| `LoomDSP-VERSION-linux-x86_64.tar.gz` | `loom-app`, `loom`, `examples/`, `LICENSE`, `README.txt` |
| `SHA256SUMS.txt` | One SHA-256 line per file above |

`README.txt` is short and plain. It says how to open the app on that system (see "First launch") and links to the [getting-started guide](../guide/getting-started.md).

The release notes are generated from the merged pull requests, followed by a fixed note that early alpha builds are unsigned and can break old patches.

### Workflow

1. **Trigger.** A push of a tag matching `v*`, or a manual run (`workflow_dispatch`).
2. **Check.** Confirm the tag matches the `Cargo.toml` version (see above). A manual run uses the `Cargo.toml` version.
3. **Build.** One job per target builds `loom-app` and `loom` in release mode.
4. **Smoke test.** Each job runs the binary it just built, with no audio device and no window:
   - `loom-app examples/patches/subtractive.ron --dump-ui --frames 4800` exits with code 0 and prints JSON.
   - `loom render examples/patches/subtractive.ron -o smoke.wav --note 60` writes a non-empty file.
5. **Package.** Pack the files in the table, using `scripts/package-release.sh`.
6. **Upload.** Upload each archive as a workflow artifact.
7. **Publish** (tag runs only, not manual runs). One final job downloads the artifacts, writes `SHA256SUMS.txt`, and creates the GitHub release with all files.

A manual run stops after step 6. It uploads artifacts so a maintainer can download and test them, and it publishes nothing.

### Signing

The first releases are unsigned. The workflow has no signing steps, because signing needs paid certificates the project does not have, and signing code that cannot be tested should not run in a job that can write releases.

When the certificates exist, signing is added in a later change that updates this section:

| System | Needs | Where it goes |
|---|---|---|
| macOS | Developer ID certificate and notarisation credentials | `scripts/package-release.sh` signs `LoomDSP.app` before it is zipped. The workflow notarises the zip and staples the ticket. |
| Windows | A signing certificate, or a signing service account | The workflow signs `loom-app.exe` and `loom.exe` before packaging. |

*Why later, not now:* the unsigned path is the one we can test today. The ADR keeps the door open, because every build already goes through one packaging script.

### First launch

Unsigned builds make the operating system warn. `README.txt`, the project README, and the getting-started guide give these steps.

| System | What happens | What to do |
|---|---|---|
| macOS | "LoomDSP cannot be opened" because the developer is not verified | Right-click `LoomDSP.app`, choose **Open**, then **Open** again. Or allow it under System Settings, Privacy & Security. |
| Windows | SmartScreen shows "Windows protected your PC" | Choose **More info**, then **Run anyway**. |
| Linux | No warning | Run `chmod +x loom-app` if the file is not executable, then `./loom-app`. |

Each page says why the warning appears (the build is not signed yet) and how to check the download: compare its SHA-256 with `SHA256SUMS.txt`.

## Rules

1. **One tag, one release.** The files come only from the tagged commit. *Why:* anyone can rebuild the same release from source.
2. **Checksums are published.** *Why:* people can check that a download is the file we built.
3. **A manual run never publishes.** *Why:* a dry run must be safe to start at any time.
4. **Permissions are minimal.** Build jobs have read access. Only the final job can write releases. *Why:* a compromised build step must not be able to change releases.
5. **Actions are pinned** to versions, as in `ci.yml`. *Why:* the release workflow runs with write access, so it must not change under us.
6. **The Linux build uses the oldest Ubuntu runner we support.** *Why:* a binary built against a newer C library does not start on older systems.

## Acceptance criteria

- [ ] Given a tag that matches `Cargo.toml`, when it is pushed, then a release appears with the five files in the table.
- [ ] Given a tag that does not match `Cargo.toml`, when it is pushed, then the workflow fails before building.
- [ ] Given a tag with a `-` suffix, when the release is made, then it is marked as a pre-release.
- [ ] Given a manual run, when it ends, then artifacts exist and no release was created.
- [ ] Given a built binary, when the smoke test runs, then it exits with code 0 on each system.
- [ ] Manual, on each system: download the release, check the checksum, open the app following `README.txt`, play `subtractive.ron` with a MIDI keyboard or **Hold note**, and hear sound.
- [ ] Manual: `loom --help` runs from the unpacked Windows and Linux archives.

## Out of scope

- Signing and notarisation, and getting the certificates (see Signing above).
- Installers: a macOS disk image, a Windows MSI, a Linux AppImage.
- Package managers (Homebrew, winget, Flathub).
- Linux on ARM, and Windows on ARM.
- Automatic updates inside the app.
- Plugin and standalone export of patches (ADR-0004, roadmap "Later").

## Open points

- **Which Ubuntu runner is the oldest we support?** The leaning is the oldest hosted runner that still gets the ALSA and graphics packages the build needs. Decide when the workflow is first run.
- **App icon.** The macOS bundle has no icon yet. An icon needs a design.
- **Intel macOS.** If the Intel build is slow or fails, ship Apple Silicon only and say so.
