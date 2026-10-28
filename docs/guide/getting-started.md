# Getting Started

This guide shows how to run the LoomDSP desktop app, play a patch, and change it while it plays.

LoomDSP is early (Phase 1). The app can open, edit, save, and play patches. See [What is not there yet](#what-is-not-there-yet).

![The FM feedback patch in the LoomDSP app](../benchmarks/images/p1-canvas-fm-feedback.png)

## Install and run

### Download (easiest)

1. Open the [latest release](https://github.com/brylie/LoomDSP/releases/latest) and download the file for your computer: `macos-arm64` for Apple Silicon Macs, `macos-x86_64` for Intel Macs, `windows-x86_64` for Windows, or `linux-x86_64` for Linux.
2. Extract it: unzip it, or on Linux unpack the `.tar.gz` file with `tar -xzf`.
3. Open LoomDSP. These early alpha builds are not signed yet, so your system warns the first time:
   - **macOS:** right-click `LoomDSP.app`, choose **Open**, then **Open** again. If macOS still refuses, allow it under System Settings, then Privacy & Security.
   - **Windows:** double-click `loom-app.exe`. On the "Windows protected your PC" screen, choose **More info**, then **Run anyway**.
   - **Linux:** run `./loom-app` in a terminal. If it does not start, run `chmod +x loom-app` first.
4. In the app, open `examples/patches/fm-feedback.ron` from the `examples` folder in the download.

Each download also has a `README.txt` with these steps. To check a download, compare its SHA-256 with the `SHA256SUMS.txt` file on the release page.

### Build from source

If there is no download for your system, or you want the newest changes, you need [Rust](https://rustup.rs) 1.99 or newer.

```sh
git clone https://github.com/brylie/LoomDSP
cd LoomDSP
cargo run --release -p loom-app -- examples/patches/fm-feedback.ron
```

The first build takes a few minutes. Later builds are fast.

On Linux, install the ALSA development package first (`libasound2-dev` on Debian and Ubuntu).

Other example patches are in [`examples/patches`](../../examples/patches). Run the app without a file to start with an empty patch.

## Play a note

The example patches make sound only while a note is held, like most synthesizers.

- **MIDI keyboard:** connect it before you start the app. The app listens to every MIDI input it finds. The bottom bar lists them, for example "MIDI: Keystation 49".
- **No keyboard:** click **Hold note** in the toolbar. Click it again to release the note.

If you hear nothing, check the bottom bar. It shows the audio device and sample rate, or a message that explains why audio did not start.

## The screen

| Area | What it does |
|---|---|
| Canvas (centre) | Your patch: modules as tiles, connected by cables. |
| Inspector (right) | Every parameter of the selected module, with sliders. |
| Toolbar (top) | File name, New, Open, Save, Save As, add a module, hold a note. |
| Bottom bar | Frame rate, module and cable count, audio load, MIDI inputs, and messages. Hover a cut-off message to read all of it. |

## Patching

| To do this | Do this |
|---|---|
| Connect two modules | Drag from an output (right edge of a tile) to an input (left edge), or the other way. |
| Move or remove a cable | Drag it out of the input it feeds. Drop it on another input, or on empty canvas to remove it. |
| Move modules | Drag a tile. Tiles snap to the grid. |
| Select several modules | Shift-click tiles, or drag a box on empty canvas. Cmd/Ctrl+A selects all. |
| Delete modules | Select them and press Delete or Backspace. |
| Add a module | Press **A** with the pointer over the canvas, or double-click empty canvas. Type part of a name, ID, or category, pick with the arrow keys, and press Enter. The module appears where the pointer was. **Add module** in the toolbar does the same at the centre of the canvas. Escape closes the browser. |
| Pan | Right-drag, or scroll with two fingers on a trackpad. |
| Zoom | Mouse wheel, or Cmd/Ctrl and scroll on a trackpad. With the pointer over the canvas, Cmd/Ctrl+0 goes back to 100%, keeping the point under the pointer in place. |
| Show the whole patch | Press F or Home. |

Every change is live. The engine switches to the new patch between two audio buffers, so there is no dropout while you patch.

## Reading a patch

Signals flow from left to right. Each port's colour shows its signal type, and its shape gives a second cue for people who do not see all colours:

| Signal | Colour | Port shape | Typical range |
|---|---|---|---|
| Audio | Red | Round | -1 to 1 |
| Modulation | Blue | Round | -1 to 1, or 0 to 1 |
| Pitch | Orange | Square | 0.1 per octave, 0 is middle C |
| Phase | Purple | Round | 0 to 1, wrapping |
| Gate | Yellow | Square | High above 0.5 |

Types only guide you. Any output can connect to any input, as in Eurorack or Bitwig's Grid. The signal rules are in [ADR-0007](../adr/0007-polymorphic-signal-model.md).

- **Filled ports** have a cable. **Rings** are free.
- **Cables run behind the tiles, dimmed.** Point at a cable, a port, or a module to bring its cables to the front at full brightness. Selected modules keep their cables bright. Nothing else changes as you move the pointer, so you can follow one connection across a busy patch.
- **Dashed cables** are part of a feedback loop. The engine adds a one-sample delay there, because a loop needs one to run. You can place your own delay with the "1-Sample Delay" module.
- **Zoomed out,** tiles hide their text so large patches stay readable and fast.

## Editing parameters

Click a tile to select it. The inspector shows its parameters. Frequencies and times use a logarithmic scale, so the middle of the slider is musically in the middle. Whole-number parameters, such as a MIDI channel, snap to whole values.


## Saving

**New**, **Open**, **Save**, and **Save As** (Ctrl or Cmd with N, O, S, and Shift+S) work on `.ron` patch files, including module positions. The window title shows the file name, with a `*` in front when you have unsaved changes. The app asks before it throws changes away. Save writes to the file you opened, so use Save As to keep an example as it is. On Linux the question and error boxes need `zenity` or `kdialog`; without one, the app tells you in the status bar to save first. Patch files are plain text in [RON](../adr/0008-patch-format-ron.md), so you can read them, edit them, and keep them in git.

## What is not there yet

These are planned for Phase 1. Each has an issue on GitHub.

- Copy and paste.
- Oscilloscopes and level meters inside modules.
- Modulation on parameters, MIDI learn, and a panic button.
- Choosing audio and MIDI devices, and plugging devices in while the app runs.

See the [roadmap](../product/roadmap.md) and the [P1 milestone](https://github.com/brylie/LoomDSP/milestone/1) for the full list. Contributions are welcome: [CONTRIBUTING.md](../../CONTRIBUTING.md).
