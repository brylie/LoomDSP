# Reading a Patch by Ear

How a person who cannot see the canvas can still get the overview a glance gives, and why LoomDSP's outline lists modules in signal-flow order, says little per line, and handles its own keys. For musicians who use a screen reader, and for developers who build the accessibility tree.

- **Kind:** explainer.
- **Used by:** [canvas accessibility specification](../design/canvas-accessibility.md).
- **Related:** [Feedback loops and the unit delay](feedback-loops.md), [Modulation without cables](modulation-without-cables.md), [Abstraction](abstraction.md).
- **Research:** [overviews of node-link diagrams for screen reader users](../research/2026-11-screen-reader-graph-overviews.md).

## A glance is parallel, listening is serial

Look at a patch and you see, at once, that there is one oscillator, a filter, two envelopes, and an output, and that the cables run left to right. Your eyes take in the whole picture first and then go back to the part you care about.

A screen reader speaks one thing at a time. If every knob is a node and the only way to move is "next node", then learning that the saw feeds the filter means listening to the saw's tile, every one of its controls, and then the filter's. On a patch of 30 modules with 8 controls each, that is 270 stops before you know the shape of the patch.

Research on accessible charts and diagrams gives the same answer again and again: give the overview first, let the user choose where to go, and only then the details. Ben Shneiderman put it as "overview first, zoom and filter, then details on demand" for visual displays, and later work on screen reader access to charts (Olli, Data Navigator, and others in the research snapshot) built the same idea as a tree of descriptions: a summary at the top, coarse groups under it, single values at the bottom.

So LoomDSP's outline has four levels: a one-line summary, one line per module, one line per connection, and the controls, which stay on the canvas.

## Why signal-flow order

A list needs an order. A graph has none of its own: modules sit wherever the user put them, and cables run in every direction.

The order a musician describes a patch in is the order the signal flows: "the phase goes into the saw, the saw into the filter, the filter into the amp, the amp to the output". That order has a name in graph theory, a **topological order**: every module comes after all the modules that feed it. The engine already needs it, because a module cannot run before its inputs are ready ([ADR-0002](../adr/0002-graph-execution-and-feedback.md)), so the compiler computes one for every schedule.

When two modules could come in either order (two envelopes that both take the gate), the outline puts them in reading order on the canvas, top to bottom and left to right. That is the order a sighted user would scan them in.

### Loops have no order

A feedback loop breaks the rule: in a loop from the filter through a delay and back, each module comes after the others ([Feedback loops](feedback-loops.md)). The compiler finds loops with Tarjan's algorithm and treats each one as a single unit, which it runs one sample at a time. The outline does the same: a loop is one row, "Feedback loop 1: SVF, Delay, Mix", with its modules inside. The structure tells the truth: this part has no first module.

### Modulation has no cable

In LoomDSP, modulation is an assignment, not a cable ([Modulation without cables](modulation-without-cables.md)). Following cables would never find that an LFO moves the filter's cutoff. The outline counts an assignment as a connection for its order, and names it on the rows of both ends.

## Why each line is short

Screen reader users often listen at two or three times normal speech, and they decide in the first few words whether to stay on a line. A line that starts with what matters, and leaves the rest for later, lets them scan fast.

Platforms give every node two texts for this: a **name**, read at once, and a **description** (macOS calls it help), read after a pause or on request. The outline puts the neighbours in the name ("SVF. From Saw and Multiply 6. To Multiply 8 and Scope.") because that is what tracing needs, and the kind of module and its modulation in the description. It names at most three neighbours on each side, then "and 4 more", because a twelve-input mixer read in full is a sentence nobody waits for.

## Why the app handles its own keys

Accessibility APIs have a role for this kind of outline, a **tree**, and relations between nodes, such as "this flows to that". It would be neat to give the platform the tree and the cables as relations, and let the screen reader do the rest.

In practice, each platform's adapter supports a different part of it. In the AccessKit versions LoomDSP uses, only one relation, `controls`, reaches the screen reader on macOS, Windows, and Linux alike. The relation meant for reading order, "flows to", reaches none of them. Only Windows reports whether a tree row is expanded and how deep it sits (the research snapshot has the details).

So the outline handles its own keys: Up and Down to move, Right and Left to open and close, Enter to go. These are the keys of the common tree pattern, so they feel familiar, and they work the same everywhere, for keyboard users with no screen reader too. The `controls` relation is added from each output port to the inputs it feeds, as an extra path for users who know their screen reader's "jump to linked item" command.

## One map, two views

A sighted user with a large patch has a similar problem: the canvas shows only part of it. A small map of the whole level, with names and connections but no controls, answers "where am I, and what is over there".

The map and the outline say the same thing: which modules there are, and what connects to what. LoomDSP builds both from one model, so they cannot disagree. For the accessibility tree, the drawn map is a single image, and the outline is its text alternative, as WCAG asks for any complex image.

## Further reading

- Ben Shneiderman, "The Eyes Have It: A Task by Data Type Taxonomy for Information Visualizations", IEEE Symposium on Visual Languages, 1996.
- W3C, [WAI-ARIA Authoring Practices: Tree View pattern](https://www.w3.org/WAI/ARIA/apg/patterns/treeview/).
- The [research snapshot](../research/2026-11-screen-reader-graph-overviews.md) lists the studies on accessible charts and diagrams this design draws on.
