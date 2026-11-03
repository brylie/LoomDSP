---
name: ux-review
description: Review the LoomDSP desktop app as its users meet it, by running it and looking at it, and report findings on accessibility, usability, functionality, consistency, convention, gestalt, and aesthetics, with evidence and a direction for each. Use whenever the user runs /ux-review, or asks for a UX, UI, design, accessibility, or look-and-feel review or audit, asks "how does this look?", "is this usable?", "does this feel right?", or wants a new view, a changed panel, or a UI pull request judged as a user would meet it, even if they don't say "UX". Accepts a view or feature, a persona, a branch or pull request, or the whole app. Not for correctness review of code (code-review) or for plans not yet built (review-plan).
argument-hint: "[view, feature, persona, PR, or \"whole app\"]"
---

# UX review

Tests prove that the app does what the code says. They do not show whether a musician on a dark stage can find Panic, whether someone who cannot use a mouse can finish a patch, or whether the app looks like a finished instrument. Only looking at it and using it shows that. So this review starts from the running app, not from the source.

Aesthetics count as much as function here. Musicians judge an instrument by how it feels to play and to read at a glance (product principle 9), so "it looks unfinished" is a real finding, if it says what looks wrong and what would fix it.

## Scope

$ARGUMENTS

Use the narrowest scope that fits:

- **A view, feature, or persona named above:** review that, and the path a user takes to reach it.
- **A branch or pull request:** review what its UI changes do to a user, and compare with `main` where the change is visual. Check how the new thing fits beside its neighbours, because a change can be fine alone and still break consistency.
- **Nothing named:** the current branch if it changes `crates/loom-app`, `crates/loom-canvas`, or a panel in a descriptor. Otherwise the whole app.

## Ground rules

- **Report, do not fix.** Do not change code, specs, or issues unless the user asks, because they decide which findings to act on. Filing issues is a separate step (section 6).
- **Every finding rests on evidence you collected in this run.** A screenshot you looked at, a number you measured, or a line you read. Say which. A review written from the code alone mostly guesses what the screen shows.
- **Look before you read.** Record your first impressions from the images before reading specs or earlier reviews. Once you know why a thing is the way it is, you stop seeing it the way a new user does.
- **Clean room (ADR-0018).** Name another product only to explain an idea or a convention. Never propose copying its layout, control names, or panel. Suggest directions from our use cases and from published guidance.
- **Accessibility is never out of scope.** However narrow the review, apply that lens to what you looked at, and give it its own section in the report.

## 1. Capture the evidence

Run the bundled script from the repository root, with a folder outside the repository:

```sh
.claude/skills/ux-review/scripts/capture.sh OUT_DIR [PATCH] [MODULE]
```

It defaults to `examples/patches/subtractive.ron` and module 4. `MODULE` is the module to select, so choose one that exists in `PATCH`. The script needs a display, and takes under a minute once the app is built. It writes:

| File | Shows |
|---|---|
| `01-open.png` | The first thing a user sees |
| `02-inspector.png` | A selected module |
| `03-matrix.png` | The Matrix tab |
| `04-browser.png` | The module browser |
| `05-help.png` | Help for the selected module |
| `06-preferences.png` | Preferences |
| `07-focus.png` | Keyboard focus on a control |
| `08-learn.png` | MIDI learn mode |
| `09-playing.png` | A held note, with live meters and scope |
| `10-first-launch.png` | The app started with no patch, as a new user meets it |
| `11-first-browser.png` | The module browser opened from the toolbar on that first launch |
| `12-browser-search.png` | The same browser after typing a search |
| `gallery.png` | Every built-in module at 100% zoom |
| `gallery.json`, `patch.json` | What the UI shows, as JSON (`--dump-ui`) |
| `measures.txt` | Cut labels and the port hit area (from `measure.py`) |
| `tour.log`, `first-launch.log` | Each reply from the tours |

**Open and look at the images you will judge.** Capturing them is not the review.

For anything the tour misses, drive the app yourself through the [control channel](../../../docs/design/control-channel.md): write a JSON-lines script and pass it with `--control-script`, or use `--control SOCKET`. Always pass `--fake-audio`, `--no-recovery`, and `--preferences FILE`, so the run touches no real device, autosave copy, or setting. For close-ups, use `--screenshot FILE --zoom 2 --scale 2 --focus MODULE:PARAM`, with `--hover-text` for hover text, or `--wire` and `--pointer` for a cable being dragged. AGENTS.md lists more. A `screenshot` with `rect` or `target` saves one area, which costs far less to read than a whole window. A montage saves more, but is too small to judge text.

Do not use desktop automation to fill a gap unless the user asks, because it runs on their screen. Report the gap in Limits.

### Traps that give false findings

Each of these has misled a review, or would have:

- **A canvas `--screenshot` without `--zoom` fits the patch to the image.** Below about 83% zoom, labels draw as grey bars. That is level of detail, not a bug in a large patch image. For a large patch, also set `--size`, because the image is cropped otherwise. The script does both for the gallery.
- **Prefer live captures to judge chrome.** `--screenshot-window` does not always open the patch as the app does. Use the control channel's `screenshot` for anything about the whole window.
- **The tour's patch is not the only state.** Check large (`--generate 300`), and error states when they matter to the scope. Polish problems tend to live there. The first-launch images cover the empty state.
- **`ok:true` means the command was delivered, not that it did anything.** `key a` with no pointer and a `scroll` that misses the panel both reply `ok:true`. After any step you rely on, compare the images (`shasum` finds identical ones quickly). A missed scroll is your script's error, not a bug in the app.
- **Use `type` for text, not `key`.** `key` presses one key wherever the keys go now: with no text field in focus, a letter runs a shortcut, and the reply is still `ok:true`. `type` takes a whole string, and is refused when no text field has focus. A placeholder is not text that `locate` can find, so aim at an empty field with `x` and `y`.
- **`add` is not the browser.** It places a module below the patch, while a real add goes at the pointer or a default spot. Use `add` to build a patch for a test. Never use it to judge placement: click Add module, or press A, instead.
- **Not everything is in the snapshot.** The module browser's results, for example, are not, so read those from images. Do not take an absence from the snapshot to mean an absence from the window.
- **Hover text from a script is not reliable.** The spec says pop-ups get the real cursor, but a trial saw a button's tooltip appear after a scripted `press`. Draw no conclusion from a tooltip showing or not showing in a live capture. Use `--hover-text` for the image, and the snapshot's `text` fields for a tile's words. Panels drawn in the window view, such as the module browser, do get scripted pointer commands.
- **`key` has no F1.** Reach help through the inspector's Help button.
- **Dialogs are not reachable.** The control channel never opens a file dialog. Say that they were not seen.
- **A right-click on a tile's centre may hit a control.** After a right-click, check that the menu is in the image. `locate` finds the first match for its text, so "Help" can resolve to the inspector's button.
- **A widget's `bounds` in the JSON is its grid cell, not its target.** A segment is 11 px high inside a 24 px cell. Do not measure targets from `bounds`. See the target question in the accessibility lens.
- **A real pointer can interfere with a live run.** The window opens on the user's screen, and a real pointer event takes over from the script. A wheel or a click there can pan, zoom, or select. If a capture shows a change your script did not ask for, read `pointer` in the snapshot: a position you did not send means a real pointer. Run it again before reporting a finding.
- **Preferences shows the machine's real device names.** Do not commit these captures, and do not treat your own device list as the only case.

## 2. First look

Before reading any spec, look at the images and write down, in plain words:

- Where did your eye go first? Where should it go?
- What does this seem to do, and what can you do next?
- What looks unfinished, crowded, misaligned, or out of place?

## 3. Walk through it with the lenses

Read [references/lenses.md](references/lenses.md) now, because it says what to try during the walks. Work through **accessibility** and **usability** first and in full, because they decide whether a person can use the app at all. Then apply functionality, consistency, convention, gestalt, and aesthetics. A lens with nothing to say gets one line saying what you checked. Do not invent findings to fill it.

Walk through a real task as a person, using `docs/product/personas-and-use-cases.md`. Choose the persona that fits the scope. If the scope names a role with no persona, such as a new user, use the nearest use case and say so. UC3, Tomas performing live, is the highest priority, so include it in any whole-app review. For each step ask: what does the user see, what do they try, what happens, and how do they know it worked?

**Always do one walk with only the keyboard**, using `key` commands on the live app, and take screenshots of where the focus goes. Tab, Enter, arrows, and Escape are enough to find out what cannot be reached. Some keys depend on the pointer, and a real person's pointer is always somewhere, while a script's starts nowhere. So when a key does nothing, try it again after a `hover` over the canvas, and report both results. That separates a real barrier from an effect of the script.

## 4. Check what you found against the record

Now read, for the areas you found problems in:

- The feature's own spec in `docs/design/` first. Then `visual-system.md` for anything visual, and `panels.md` and `panel-widgets.md` when tiles are involved.
- Decisions in `docs/adr/` (ADR-0025 one design language, ADR-0028 screen readers deferred, ADR-0029 families and palette), and the requirements (`NFR-UX-*`).
- Earlier reviews in `docs/research/` matching `*ux*`. A keyword search is enough for a narrow scope.
- Open issues: `gh issue list --label area:ui --state open --limit 300`, the same for `--label accessibility`, and `gh issue list --search "KEYWORD" --state all` for each finding, because claiming "not filed" needs the whole list.

Then sort each finding:

| It is | Then |
|---|---|
| The app differs from an agreed spec | A bug. Cite the spec's rule |
| The spec itself causes it | Say the spec should change, and why. Specs are living documents |
| An accepted ADR causes it | Say so plainly, and that it needs a new ADR. Do not argue it again if the ADR already weighed this point |
| Already filed | Move it to "Already filed", with the link. Add new evidence as a note if you have any |
| The earlier review said fixed, but it is back | Say so. A regression is worth more than a new finding |

Do not drop a finding only because a document explains it. A reason can be real and the user can still be stuck.

## 5. Report

Priority follows the project's audits:

- **High:** blocks a task or misleads the user. **An accessibility barrier that stops someone finishing a task is High, however few people it affects.**
- **Medium:** slows the user or looks broken.
- **Low:** polish. Aesthetic findings are often Low one at a time, so when several share a cause, report the cause as one finding and rank it by their joint effect.

Write in this order:

1. **Summary:** two or three sentences on how the scope feels and where it is weak.
2. **Table of findings:** number, area, lens, finding, priority, ordered by priority.
3. **Accessibility:** always its own section, findings or not, and what you could not check.
4. **Findings:** one short block each, as below.
5. **What works well:** specific things to keep. It tells the next person what must not regress.
6. **Already filed:** with links.
7. **Limits:** what you did not see, hear, or try. Always say that no audio was heard (with `--fake-audio`), that no assistive technology was used, and that no real user was watched, because these limit every review made this way.

For each finding give:

- **What a user meets,** in their terms, not the code's.
- **Why it matters,** and to whom. Name the lens and any standard or rule.
- **Evidence,** and which kind: **seen** (`09-playing.png`, at the top right), **measured** (3.1:1), or **read** (`path:line`). If you worked out a behaviour from code and did not see it, label it read, and say "not seen".
- **Direction,** specific enough to act on: the token, the widget, the rule. Give one recommendation. If it changes behaviour, say that it needs a spec first, because AGENTS.md does not allow guessing behaviour.

Keep taste apart from defects. "I would prefer more space here" is a suggestion: mark it **Taste**, and do not rank it above a defect. Report real findings only, because a long list hides what matters. Use short sentences and plain words, with no em dashes, as AGENTS.md asks.

**Where it goes.** Write the report in your reply. When the scope is the whole app or a listed persona, also draft a snapshot at `docs/research/YYYY-MM-<scope>-ux-review.md`, using `docs/research/2026-11-accessibility-ux-review.md` as the model: Kind, Date with the commit, Method, Limits, Summary. Research snapshots do not change later. Leave the draft uncommitted for the user, and leave images out of it.

## 6. After the report

Ask which findings to act on, then do only what the user chooses. Before filing issues, show the list. When filing:

- Group findings by what would be fixed together, as the earlier audits did. A backlog of forty small issues helps nobody.
- Label with `area:ui`, one of `must`, `should`, or `could`, `accessibility` when it applies, and `needs spec agreed` when behaviour must be settled first.
- Link the snapshot, and list the issue numbers in its "Used by" line while it is still a draft.
