# Canvas Keyboard Shortcuts

- **Status:** Implemented
- **Requirement:** FR-CANVAS-06
- **Issue:** [#16](https://github.com/brylie/LoomDSP/issues/16)

## Behaviour

- **Cmd/Ctrl + 0** resets canvas zoom to 100%, preserving the world point at the centre of the viewport.
- The inspector help text lists Cmd/Ctrl+0, Cmd/Ctrl+A, Delete, F, and Home.

The platform modifier is Command on macOS and Control on other platforms.

## Acceptance checks

- A unit test verifies zoom reset sets the zoom to exactly 1.0 while keeping the viewport centre anchored.
- Manual check: verify that the inspector help text lists Cmd/Ctrl+0, Cmd/Ctrl+A, Delete, F, and Home.

## Out of scope

Arrow-key nudging of selected modules and Escape clearing the selection are deferred until the key map in issue #101 is defined. Arrow keys and Escape also control panel focus; Escape cancellation of cable wiring is out of scope for this requirement.
