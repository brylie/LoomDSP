# Canvas Keyboard Shortcuts Specification

Keyboard shortcuts let patch authors navigate and edit a LoomDSP canvas without reaching for the pointer.

- **Kind:** living specification.
- **Status:** Implemented
- **Decision behind it:** None yet.
- **Use case and requirements:** FR-CANVAS-06 (keyboard navigation and shortcuts for common actions).
- **Issues:** [#16](https://github.com/brylie/LoomDSP/issues/16).
- **Code:** `crates/loom-canvas/src/widget.rs`; inspector help text in `crates/loom-app/src/main.rs`.

## Goal

A patch author can make small, precise layout changes, recover from an in-progress cable gesture, and return the canvas to a known zoom level using the keyboard. The shortcuts should be discoverable in the inspector help text and should behave consistently with existing canvas keyboard actions.

## Behaviour

| Input | Proposed behaviour |
|---|---|
| Arrow key | Move the selected module or modules by one grid step in the indicated direction. |
| Shift + Arrow key | Move the selected module or modules by four grid steps in the indicated direction. |
| Escape while dragging a cable | Cancel the cable drag without creating a connection. |
| Escape with no cable drag and a non-empty selection | Clear the selection. |
| Cmd/Ctrl + 0 | Reset canvas zoom to 100%. |

The platform modifier is Command on macOS and Control on other platforms. The inspector help text lists these shortcuts and their actions.

## Rules

1. **Movement follows the canvas grid.** Arrow keys move the current selection by one grid step; holding Shift changes the distance to four grid steps. *Why:* keyboard movement should be predictable and align with the existing snap grid.
2. **Movement preserves the selection.** Nudging changes selected modules' positions, not which modules are selected. *Why:* repeated nudges should remain easy.
3. **Escape resolves the active interaction first.** If a cable drag is in progress, Escape cancels that drag; otherwise, if modules are selected, it clears the selection. *Why:* cancellation should not accidentally change the patch or leave a selection behind.
4. **Zoom reset is explicit.** Cmd/Ctrl + 0 sets the zoom level to 100%. *Why:* users need a quick, known baseline after zooming.
5. **Shortcuts are discoverable.** The inspector help text documents the supported keys and their effects. *Why:* keyboard support is useful only when people can find it.

## Acceptance criteria

- [x] With one module selected, each arrow key moves it exactly one grid step in that direction.
- [x] With one or more modules selected, Shift + an arrow key moves the selection exactly four grid steps in that direction.
- [x] Nudging preserves the selected modules and their relative arrangement.
- [x] Escape during a cable drag cancels the drag and does not create a connection.
- [x] Escape when no cable drag is active clears a non-empty selection.
- [x] Cmd/Ctrl + 0 resets zoom to exactly 100%.
- [x] The inspector help text lists all shortcuts and their meanings.
- [x] Automated tests cover movement distance and direction, Escape handling, and zoom reset.

## Out of scope

- Adding new shortcuts beyond the five behaviours listed above.
- Reworking existing Delete, F, Home, or Cmd/Ctrl + A behaviour.
- A general keyboard-only navigation model for every canvas control.

## Implementation notes

- Shortcuts are handled only while the pointer is over the canvas, matching the widget's existing keyboard shortcuts.
- Zoom reset preserves the world point at the centre of the viewport.
- Nudges emit the existing `CanvasEvent::Move` event, so the application updates selected module positions through its normal model path.

## Open points

- **Undo/redo:** Should a single nudge (including a Shift nudge) create one undoable edit, and should repeated key presses be grouped? Align this with the project's undo model.
