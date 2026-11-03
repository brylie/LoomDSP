# The lenses

Ask these of what you captured. They are prompts for looking, not a form to fill in. Skip what does not apply, and follow anything else you notice.

## Accessibility

Can a person with a disability, or in a hard setting such as a dark stage, do the task? The working target is WCAG 2.2 level AA where it fits a desktop app, plus the project's `NFR-UX-*` requirements. WCAG was written for the web, so treat its numbers as a tested guide, and say when you apply one by analogy.

**Keyboard and motor**

- Can every action be done without a mouse? Try it with `key` commands (NFR-UX-01). Name what cannot be reached.
- Is focus always visible, and is the order sensible? (WCAG 2.4.3, 2.4.7, 2.4.11)
- Can focus get stuck, or leave a dialog behind it? (2.1.2)
- Does every drag have another way? (2.5.7) Cables are the hard case.
- Do timed gestures have another way? A double-click within 400 ms and 4 px does not suit a tremor or a switch.
- Are targets at least 24 by 24 px? (2.5.8) A smaller target still passes if a 24 px circle centred on it touches no other target, if another control on the same page does the same job at full size, if it sits inside a line of text, or if its small size is essential. So check the space around it before reporting a failure. `measures.txt` gives the port hit area. For a widget, the control channel's `locate` replies with a size in window pixels, and hit tests are in `crates/loom-canvas/src/controls.rs`. Stage use needs more: a trackpad in the dark is not a precise tool.
- Can a stray key do something costly? Stopping the transport on stage is costly.

**Vision**

- Does text have 4.5:1 contrast, or 3:1 for large text, which starts at 24 px, or 18.66 px bold? (1.4.3) The largest app text token is 18 px, so all app text needs 4.5:1. Do controls, focus rings, and meaningful graphics have 3:1? (1.4.11) Measure with `measure.py contrast`. Disabled controls are exempt, but must still read as present.
- Take colours from `crates/loom-app/src/style.rs` and `crates/loom-canvas/src/theme.rs`, not by eye. Chrome fills come from Iced's palette and are in neither file, so sample those from a capture with `measure.py pixel`. Text drawn over a fill needs a check against that fill.
- Is colour ever the only signal? (1.4.1, NFR-UX-03) Ports, signal types, on and off, errors. Look at the image and ask what would remain in greyscale.
- Do colour pairs survive colour blindness? Family colours are tested in `theme.rs`. Anything new is not. The method is Machado 2009 with CIE76 ΔE, as in `visual-system.md`.
- Can text grow? (1.4.4, #525) What breaks at a larger scale? Is text under 11 px in the app's own chrome? Tile text scales with zoom, so judge it at the zoom the patch opens at.
- Does it hold up at low brightness? Faint text and thin lines are the first to go on a stage.

**Hearing and cognition**

- Is anything shown only by sound? Is a state a person must know shown only briefly? Status messages get replaced by the next one.
- Are errors plain, and do they say what to do next? (NFR-UX-05)
- Does anything keep moving with no way to calm it? (2.3.3) Scopes are content, so this applies less to them than to chrome.
- Do words stay simple? Many users read English as a second language.

**Screen readers**

- ADR-0028 defers the accessibility tree until Iced has AccessKit support, so do not report its absence again.
- Do check that new view models carry a name, a value with its unit ("800 Hz", not "800"), and a state, because the tree will be built from them (NFR-UX-06). `patch.json` shows them.

## Usability

Can a person who wants this reach their goal, without help? Judge with Nielsen's heuristics and the task walk.

- **Status:** Does the app say what it is doing? After an action, what changed on screen? Is learn mode, or a held note, obvious at a glance?
- **Match with the user's world:** Are words a musician's, not a developer's? No IDs, internal terms, or raw values in user text.
- **Control and undo:** Can a person leave any state? Is each edit one undo step? Is anything lost with no warning?
- **Prevention:** Does the design stop the slip before it happens? Are costly and common actions far apart?
- **Recognition over recall:** Are actions visible, or does the user have to remember a key or a right-click? Does a disabled control say why?
- **First minute:** What does a new user see in an empty patch? Is the next step obvious? The success measure is a playable synth in under 20 minutes.
- **On stage:** Can it be read at arm's length, in the dark, under pressure? Is the emergency control found without reading? (UC3)
- **Empty, large, and broken:** What about no modules, 300 modules, no audio device, a long name, an unknown module?
- **Learning:** Can a learner find out what this does and why, from where they are? (Principle 11)

## Functionality

Does it work, from the user's side? This is not a code review, so only report what a person would meet.

- Does each control do what its label says, and does the sound or display respond? Check with `sample`, `snapshot`, and `--record` with `loom analyze`.
- Does the screen tell the truth? Do values, units, lights, and meters match the engine? Does the inspector match the tile?
- Does the feature finish the job, or stop short? What does the user do next?
- Is anything cut off, overlapping, or off the window? Check the `shown` fields, and the window at 1280 px wide.
- Does the state survive? Save and reopen, undo and redo, change device.
- Does anything respond slowly enough to feel broken? `--measure-idle` shows wasted frames.

## Consistency

Does the same thing look and work the same everywhere? Principle 12 makes this a rule, not a preference.

- Do sizes come from `style.rs`, and do buttons keep their roles? Is there more than one primary in a view? (`visual-system.md`)
- Does one colour keep one meaning? Orange is for editable values, pink for modulation, light green for lights, red for danger. Note a new colour, or a reserved one used for something else.
- Does one setting use one widget? Two choices should not be a radio on one tile and a toggle on the next (`panels.md`).
- Do tiles follow the same layout grammar? Compare neighbours in `gallery.png`.
- Do the same words mean the same thing? Compare the tile, inspector, help, browser, and status bar. Are names in Title Case?
- Do the same gestures work the same on the canvas, inspector, and matrix? Is Escape always "back out"?
- Do the spacing, corners, and borders match between dialogs and panels?

## Convention

Does it do what people already expect? Departing from a convention is allowed, but must be a choice with a reason, so flag each one.

- **Desktop:** Cmd on macOS and Ctrl elsewhere. Standard Save, Undo, Close, and Escape behaviour. Does a dialog's main action sit where this platform puts it?
- **Modular and audio:** Knobs turn clockwise to increase, signal flows left to right, gain in dB, frequency on a log scale, pitch and units as musicians write them, 1 V per octave style pitch. Eurorack and patching tools set these habits.
- **Canvas tools:** Scroll or pinch to zoom, drag to pan, drag a box to select, Delete to remove, F to frame.
- **Controls:** Does it look clickable if it is? Does a checkbox mean a setting and a button mean an action? Double-click to reset is common in audio tools; is it discoverable?
- Name the convention, and who sets it. "Most apps do this" is weak evidence. A platform guideline or a standard is strong.

## Gestalt

Does what is drawn match how things are related? People see grouping before they read a word.

- **Proximity:** Are related things nearer each other than to unrelated things? Is a caption clearly closer to its own knob than to the next one? Is space between groups wider than inside them?
- **Similarity:** Do things that look alike act alike? Do things that act differently look different? A button and a static label should not share a look.
- **Common region:** Do panels, tiles, and dialogs show what belongs together? Are there boxes that add nothing?
- **Continuity:** Can a cable be followed through a crossing? Do aligned rows and columns lead the eye, or do small offsets break them?
- **Figure and ground:** Does the patch stand out from the canvas, and the content from the chrome? Is a dialog clearly in front?
- **Focal point and hierarchy:** Does the eye land on what matters? Is there one strongest thing in each view? Squint at the image, or view it small: what is left?
- **Common fate:** Do things that change together look linked? A modulated knob and its source, for example.
- **Closure and balance:** Do tiles feel settled, or do they have holes and lopsided space?

## Aesthetics

Does it look like a finished instrument that someone cared about? Look at the image at 100%, then zoomed in, then small. Principle 9 sets the bar: the clarity and finish of a well-made instrument. Say what looks wrong, and name what would fix it.

- **Rhythm and alignment:** Is there a visible grid? Do baselines and edges line up across neighbours? One pixel off reads as careless, even when nobody can say why.
- **Spacing:** Is padding even? Is the density right, neither cramped nor empty? Is unused space on a tile a choice, or left over?
- **Type:** Is the hierarchy clear with few sizes? Is the text crisp at 1x and 2x? Are lines a comfortable length?
- **Colour:** Do the colours sit together, in one temperature and at one saturation? Do the accents stay rare enough to mean something? Does anything vibrate against its background?
- **Depth and edges:** Are corners, borders, and elevation one family? Does the chrome stay quiet, so the patch can lead?
- **Drawing quality:** Zoom to 2x and 4x. Are curves smooth, arcs clean, and strokes one weight? Any stair steps, beading, or uneven line weight?
- **Motion and life:** Does live data feel alive, or does it jitter? Use `sample` for numbers, and say that a person must judge the feel.
- **Whole view:** Does it look like one app across canvas, side column, help, and dialogs? Does anything look like a default widget nobody styled? Would you put it on stage, or show it to a friend?

Back taste with a reason a designer would accept: alignment, hierarchy, contrast, rhythm, or the visual system's rules. Where the only reason is preference, mark the finding **Taste**.
