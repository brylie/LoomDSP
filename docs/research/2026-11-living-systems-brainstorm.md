# Living Systems: A Module Brainstorm

- **Kind:** Research snapshot. Non-normative. A brainstorm, so it holds ideas, not decisions.
- **Date:** 2026-11-28.
- **Sources:** two notes from the design agent that Brylie passed on (a "Systems" family with five groups, and 28 module ideas); the [VCV collections survey](2026-11-vcv-collections-survey.md) of the same date; the [module catalogue](../product/module-catalogue.md) as merged with pull request 935; the [generative UX review](2026-11-generative-ux-review.md).
- **Steers from Brylie on this date:** the engine and catalogue grow to serve these ideas, and natural systems are a main user story; the flower of life and other beautiful geometries should become signals; sacred geometries should be sources of modulation and pitch.
- **Used by:** nothing yet. It is input for planning M3, "Living systems".

The vision calls LoomDSP "a grid for building living musical systems", and M3 is named "Living systems": patches that play, evolve, and arrange themselves. Modules that behave like natural systems are one of the project's main user stories (Brylie, 2026-11-28). Conventional modules and the utilities found in the VCV survey give a patch its body. This snapshot looks for what gives it life: modules that are small systems with a state of their own.

## How to use this document

- Every idea here comes from published mathematics and science, which is what clean-room design asks for (ADR-0018). References are at the end. They are from general knowledge and were not checked against the papers on this date; check each one when its module is designed.
- Names are working names in our own words. A scientific name (Lorenz, Kuramoto) belongs in help and explainers, as the source of the idea.
- "Type", "mode", and "abstraction" follow ADR-0017: a mode is an option on a type, and an abstraction is a sub-patch of other modules.
- Nothing here changes the catalogue.
- **The ideas lead.** The engine and the catalogue grow to serve these plans; that is what the project is for. Where an idea needs something the engine cannot do today, this document says what the engine must gain. It is a list of work, never a reason to drop the idea.

## What makes a patch feel alive

Five properties, from looking at what the proposed systems share. A module that has several of them is worth more than one that has a new equation.

1. **It has memory.** What it does next depends on what happened, not only on its knobs.
2. **It listens.** Something in the patch can push it: a trigger, a level, another system. A system with no way in is only a complicated LFO.
3. **It moves on more than one time scale.** Fast detail inside slow change. The generative review found that our slow controls stop at seconds (finding 1); living systems need minutes.
4. **Its surprises have a character.** Never random, never fixed. A player can learn how it tends to behave.
5. **It has an arc.** It can build, settle, tire, or tip over, so a piece gets long-form shape without an arranger.

**A test for any candidate:** what does the player hear change when they turn its one main control from low to high? If the answer is "it goes from still, to regular, to wild", the module is playable. If the answer needs the equation, it is a demonstration, not an instrument.

## Atoms: traits for system modules

An earlier draft of this section gave every system module one fixed shape. Brylie's reading (2026-11-28) was that this would constrain the collection too early: the design system brings familiarity through consistent design, while each module keeps its own inputs, parameters, panel, and outputs. Tested against the ideas, that is right. Most parts fit a cluster of modules, not all of them.

So these are **atoms**, in the sense the catalogue already uses for traits: a set of parameters and behaviour defined once, and declared only by the modules that need it (FR-SDK-07). A player learns an atom once, and it looks and behaves the same wherever it appears. Clusters of modules then share a form because they share atoms, not because a template says so. Together the atoms are a small language for describing a dynamical system as a module.

| Atom | What it is | Status | Fits | Does not fit |
|---|---|---|---|---|
| **Rate** | How fast it runs: free, synced to the bar, or driven by an external phase. One step per trigger is a fourth way that these modules would add | Timebase trait (TR-02) for the first three. Stepping per trigger is not in the trait today; it is a proposed extension | Attractors, orbits, fields | Regulator, Tiring, and Leaky bucket have time constants of their own; Shuffle steps per trigger |
| **Seed** | Where it starts. The same seed gives the same piece | Seed trait (TR-07) | Anything with chance or a chosen start | Spring, Regulator, Leaky bucket |
| **Reset** | A gate input that returns it to its start | Retrigger trait (TR-03) | Nearly all | |
| **Push** | An input that disturbs the state: a kick, a drop, a gust | Candidate trait | Driven oscillator, Ripple tank, Chain, Flock | Shuffle, Figure, fixed rhythms |
| **Energy** | One main control from calm to wild | Candidate trait | Logistic map, Driven spring, Bounce | Flock and Turing patterns, whose two or three real controls should stay visible |
| **Probe** | Where the outputs read from, as X and Y inputs | Candidate trait; Voronoi field (MT-MOD-09) has it | Fields and pattern fields | Everything else |
| **State space face** | The point, its trail, the shape it lives on | A face archetype (ADR-0053), not a trait | Attractors, orbits, fields, figures | Leaky bucket (event lanes), Spring (time trace), Rewriting (lanes) |

Notes on the atoms:

- **Energy must not hide the lesson.** A player learns what growth rate does in the logistic map by turning growth rate. Where a system has one natural control, Energy is that control under its own name. Where it has several, they all show.
- **Push is also a question to ask of every design:** can the patch reach into this module? A system with no way in is only a complicated LFO. Some modules will rightly answer no.
- **More atoms will appear.** Likely ones: Coupling (how strongly parts pull on each other, as in Coupled phases and Flock), Damping (how fast it settles, as in Spring, Chain, and Bounce), and Count (how many bodies, cells, or birds).
- **Find them by building.** When the same control cluster shows up in three modules, make it a trait then. Until then it is a candidate.

Two conventions hold for every system module, because they are about signals, not about a module's shape:

- **Angles come out as phases.** The design agent proposed this. It ties every rotating system into the phase modules, the centre of our design.
- **Events come out as gates or triggers,** so any system can clock, reset, or push any other.

**Traits and roles.** The catalogue has two words, and both may apply. A *trait* is shared parameters and behaviour, as above. A *role* is a contract of port names that lets one module be swapped for another in a slot, as a filter is swapped in the synth voice. A system module that gives one modulation output could fill the existing Modulator role, so a player could swap an LFO for an attractor inside any shipped sub-patch without repatching.

The generative review's finding 4, "every restart plays the same piece", belongs here too. A seed gives repeatability; a living piece sometimes wants a new seed on each start. The review's direction is a choice for the whole patch, "Same piece each run" or "New piece each run", with the seed shown so a good run can be repeated. System modules should follow that choice, not add their own.

## The 28 ideas, sorted by mechanism

The design agent's five groups (physical, chaotic, fractal and rewriting, random, emergent) sort by where an idea comes from. That is right for teaching. For building, sort by mechanism: ideas that share a mechanism share a type, a face, and most of their code. The 28 ideas, plus the systems already in the catalogue, come to about twelve types.

| Mechanism | Ideas it holds | Catalogue today | Likely form |
|---|---|---|---|
| **Flow:** a point moving smoothly under a few equations | Lorenz, Rössler attractor, Double scroll, Predator and prey | Attractor (MT-MOD-06) | Modes of Attractor |
| **Map:** a point that hops once per trigger | Logistic map, Hénon map, Clifford | Iterated map (MT-MOD-11), Attractor | Modes |
| **Driven oscillator:** a simple oscillator pushed by an input | Driven spring, Heartbeat | None | **New type.** Its input is the Push, and it locks to it or stumbles |
| **Spring:** one mass that follows its input | Spring follower | Slew (built), character trait | Mode of Slew, or a small new type |
| **Chain:** many masses in a row | Mass-spring chain | None (a gap in the VCV survey) | **New type**, shared with the audio-rate resonator |
| **Bodies:** a few objects under gravity | Orbits, Orbital resonance, Pendulum, Bounce, Wind chimes | Pendulum (MT-MOD-07), Bounce (MT-MOD-05) | **Orbits** is a new type; Orbital resonance is a preset of it; Wind chimes is an abstraction of Noise, Pendulum, and Compare |
| **Field:** a surface with a rule, read by a probe | Ripple tank, Turing patterns, Voronoi field, Mandelbrot and Julia | Voronoi field (MT-MOD-09), Escape-time fractal (MT-MOD-12) | **One interface, several types.** The costliest group; see below |
| **Grid automaton:** cells that follow a local rule on a clock | Cellular automaton, Langton's ant, Sandpile | Automaton (MT-SEQ-09), one row | Modes of Automaton, or one two-dimensional type |
| **Rewriting:** a sequence grown by replacing symbols | L-system melody, Cantor rhythm, Golden rhythm, Thue-Morse | Generator (MT-SEQ-04) names L-systems | **One new type**, with the three fixed rhythms as presets |
| **Chance:** randomness with a shape | Rain, Shuffle, Weighted dice, Lévy flight, Brownian, Markov | Random (built), Brownian (MT-MOD-08), Markov (MT-SEQ-10) | Modes of Random, except Rain, which is a mode of Noise or a small type. These appear in the VCV survey's gap list and its smaller additions |
| **Agents:** many parts that steer by their neighbours | Flock, Murmuration, Fireflies, Spiking neurons | Coupled phases (MT-PHASE-10) | **Flock** and **Neurons** are new types; Murmuration is Flock with coupled phases |
| **Population:** amounts that grow, compete, and fade | Predator and prey, Outbreak | None | **New type**, or modes of Attractor. Outbreak is a one-shot, so it sits closer to an envelope |
| **Selection:** variation plus a player's choice | Evolving melody | None | **New type.** The only one where the player is inside the loop |

Sharing a mechanism is how we can afford all 28, not a way to cut them. Each idea should stay something a player finds by its own name, with its own preset, picture, and lesson. The module browser cannot do that yet: its list has four kinds of row (built-in abstractions, modules, slots, and library items), and a mode or preset of a module is none of them. That is work to plan; see the engine list below. Underneath, about eight new types carry them (ADR-0017), so each new idea after that costs a rule and a preset, not a module built from nothing.

## More ideas

Twenty-two more, in the same form. They lean toward properties 2 and 5 above: systems that listen, adapt, and have an arc. The five groups are kept so they fit the design agent's sections.

**Physical**

| Module | What it does musically | Teaches | Likely form |
|---|---|---|---|
| Billiards | A ball in a shaped table fires a trigger at each wall. A round table gives steady rhythm; a stadium shape never repeats | Why shape decides between order and chaos | New type |
| Dripping tap | Drips come evenly at a low flow, in pairs as it rises, then irregularly | Period doubling in something everyone has heard | Mode of Driven oscillator |
| Stick and slip | A level builds slowly, then lets go in a jump of uneven size | Friction, earthquakes, a bowed string | New type, small |
| Tides | Several slow cycles whose lengths share no common multiple, summed. It rises and falls but never repeats | Beating, why tides differ each day | Abstraction of LFOs; worth shipping as one |
| Seasons | A slow cycle that changes the depth and rate of a faster one, nested two or three deep | Time scales inside time scales | Abstraction of phase modules |
| Cooling | A random source that starts wild and settles as its temperature falls. A push reheats it | Annealing: how metals and crystals find order | Mode of Random or Brownian |

**Chaotic**

| Module | What it does musically | Teaches | Likely form |
|---|---|---|---|
| Chemical clock | Three linked amounts that cycle in turn, with sharp switches: three irregular LFOs a third of a cycle apart | Oscillating chemical reactions | Mode of Attractor |
| Rock, paper, scissors | Three species where each beats the next. They take turns leading, and the turns grow longer | Cyclic dominance in ecology | Mode of Population |
| Excitable ring | A ring of cells that fire and then rest. A pulse travels round; a badly timed push starts a second wave or stops both | Heart tissue, waves that chase their tails | New type, close to Neurons |

**Fractal and rewriting**

| Module | What it does musically | Teaches | Likely form |
|---|---|---|---|
| Landscape walker | A probe walks a fractal landscape. Height is a value, slope is another, and crossing a contour fires a trigger | Terrain, and reading a surface as a signal | A Field type on Fractal noise (MT-MOD-10) |
| Subdivide | Each bar, one random step of a rhythm splits in two or merges with its neighbour | How rhythm trees grow | Mode of the Rewriting type |
| Life | A two-dimensional grid of cells that live or die by their neighbours. Rows or regions fire gates | Gliders, and structure from four rules | Mode of the grid automaton |

**Random**

| Module | What it does musically | Teaches | Likely form |
|---|---|---|---|
| Leaky bucket | Triggers fill a bucket that leaks. When it is full it fires and empties. Dense input gives fast firing; sparse input gives none | Thresholds, the simplest neuron | New type, small. One cell of Neurons |
| Cooldown | After it passes a trigger, it ignores the next ones for a while. Chance can shorten or stretch the rest | Refractory periods; why nerves cannot fire twice at once | Mode of Gate shaper (MT-GATE-01) |
| Tiring | Each trigger gets a weaker response than the last. Leave it alone and it recovers | Habituation, the simplest kind of learning | New type, small |
| Clusters | Events arrive in clusters: each one makes more likely for a short time | Aftershocks, and why buses come in threes | Mode of Rain |

**Emergent**

| Module | What it does musically | Teaches | Likely form |
|---|---|---|---|
| Regulator | It watches a level and slowly moves an output to bring that level back to a target. Push the patch and it leans back | Negative feedback, body temperature, a thermostat | New type. The clearest "it listens" module |
| Follower clock | A phase that speeds up or slows down to line up with incoming triggers, with a set stiffness | Entrainment: how players and fireflies find a pulse | Clock to phase (MT-PHASE-09) with a looseness control |
| Call and answer | Two voices that avoid speaking at once. Each waits for the other to finish, then replies | Turn-taking in frogs, birds, and conversation | Abstraction of two Leaky buckets, or a type |
| Voices that lean | Each voice's value is pulled toward its neighbours' values. One control goes from independent to unison | Diffusion and consensus | New type. It couples the voices of one cable |
| Learner | It listens to the notes played and builds its table of what follows what. Then it plays in that style | Markov chains as memory | Mode of Markov (MT-SEQ-10) |
| Life span | One long gesture: it grows, matures, and declines over minutes, and a push at any point changes how it ends | Growth curves, and form over a whole piece | New type, close to Outbreak |

**Three to look at first for live work:** Regulator, Tiring, and Leaky bucket. They are tiny, cheap, and they make any patch react to itself. A patch with one Regulator in a feedback path already behaves like a living thing.

## Geometry as signal

Brylie asked for the flower of life, "and any other beautiful geometries that could somehow be made into signals" (2026-11-28). A figure is not a system: it has no state that moves by itself. But it becomes a signal in six plain ways, and all of them fit what we already have.

| Way | What happens | What comes out |
|---|---|---|
| **Trace** | A phase moves a point along the figure | X and Y values, the angle as a phase, the distance from the centre |
| **Cross** | The point crosses a line or enters a region | A trigger or a gate |
| **Cell** | A probe sits in one region of the figure | A stepped value: which region, or how many shapes cover it |
| **Symmetry** | The figure repeats by turning or mirroring | N copies of a signal, as outputs or as voices |
| **Ratio** | The figure is set by a ratio of two numbers | The same ratio as a polyrhythm or a musical interval |
| **Scan** | A curve visits every part of a surface | A way for a phase to read any field |

Trace is the important one. An oscillator is a shape read by a phase; a traced figure is the same idea in two dimensions. So these modules sit at the centre of our design, not at its edge. Patch the X and Y outputs into the XY Scope, which is built, and the figure draws itself. Run the phase at audio rate and the figure is also a sound.

| Module | What it does musically | Teaches | Likely form |
|---|---|---|---|
| Flower of life | Circles of one size on a six-sided grid, each centred on its neighbours' edges. A point travels from circle to circle. Each petal it enters fires a gate, the number of circles covering it is a stepped value, and six outputs follow the six directions. A Rings control grows it from one circle, to two, to seven, to nineteen | Six-fold symmetry, and why circles pack in sixes | A pattern field with a path on it |
| Rose | A flower-shaped curve whose petal count comes from one ratio. Whole ratios give few petals; others fill in slowly | How a ratio becomes a shape | Preset of Path |
| Two pendulums | Two swings at right angles draw a figure. Simple ratios give still figures, and each figure is a musical interval: 2 to 1 is an octave, 3 to 2 a fifth. A little detune makes it turn; decay makes it spiral inward | Intervals as shapes; why consonance looks simple | Preset of Path |
| Rolling circles | A point on a wheel that rolls inside or outside another wheel. The two sizes set the number of lobes | Gear ratios, and the loops planets seem to draw | Preset of Path |
| Circles on circles | A chain of turning circles, each riding on the last. With enough of them it draws any closed shape, and the same sum makes any wave | Fourier series, shown as a machine | Type: Path built from parts. Also an explainer |
| Polygon and star | A point walks the edges of a polygon, or of a star that joins every k-th corner. Each corner fires a trigger | Star polygons; rhythm as a necklace of points | Preset of Path. At audio rate it is a wave with corners |
| Knot | A path wound p times one way and q times the other round a ring. It gives three values, and p against q is a polyrhythm you can see | Winding numbers | Preset of Path, with a third output |
| Seed spiral | Each trigger places the next seed, turned by the golden angle from the last. The angle is a phase and the distance a value, so a melody spreads without bunching | Why sunflowers and pine cones look as they do | New type, small |
| Times table | Points round a circle, each joined to k times itself. Stepping along the lines gives an order of steps; k draws a heart shape, then two lobes, then more | Counting round a circle (modular arithmetic) | New type, small. A way to reorder any sequence |
| Tiling | A probe walks across tiles. Each kind of tile is a value, and each edge a trigger. A square or six-sided tiling repeats; a five-fold tiling of two tile shapes never does | Order without repetition | A pattern field. The same interface as Voronoi field |
| Circle packing | A large circle filled with ever smaller ones, each touching three others. Sizes give a series of pitches or levels that thin out in a set way | Curvature, and a pattern inside every gap | A pattern field |
| Plate figures | A plate that vibrates in one of its modes. A probe reads how much the plate moves at one point, and the still lines between regions fire triggers. Two controls choose the mode | Standing waves in two dimensions | A pattern field. The same modes ring in Modal resonator (MT-SRC-06) at audio rate |
| Moire | Two fine gratings, one turning slowly over the other. A probe reads light and dark: slow, wide bands from two fast patterns | Beating, and why fine patterns shimmer | A pattern field |
| Turning solid | A cube or a twenty-sided solid turns in space, and each corner is one voice: its position on screen gives X and Y per voice | Symmetry, and seeing depth from a flat picture | New type. A system that makes voices |
| Kaleidoscope | Takes any X and Y pair and gives N copies, turned or mirrored round the centre. It is a processor, so it works on any system or path | Symmetry groups: the kaleidoscope | New type, small |
| Tone lattice | Notes laid out so that each step in one direction is a fifth and in another a third. A walk across it gives chords, and each small move changes one note | Harmony as geometry | New type. The most musical of the set |
| Dance of two orbits | A line between two orbiting bodies draws a rose. The ratio of the two years sets the petals | Why orbit ratios look like flowers | Preset of Orbits |
| Filling curve | A curve that visits every cell of a square and keeps nearby cells close in time. A phase goes in; X and Y come out, ready to steer any probe | How a line can fill a plane | Preset of Path. It lets one phase read any field |

### Sacred geometry as modulation and pitch sources

Brylie asked for these in particular (2026-11-28). The traditional figures share one useful property: each is a small set of points, lines, and regions built from a few exact ratios. That gives four kinds of output from every figure, and one module type can carry them all.

**One type, working name Figure.** A figure has nodes (points), edges (lines between them), and regions. A phase walks it. Every preset gives the same outputs, so a player learns them once:

| Output | Type | Where it comes from |
|---|---|---|
| X, Y | Modulation | Where the walking point is |
| Distance | Modulation | How far the point is from the centre. On most of these figures it breathes in and out |
| Angle | Phase | The direction of the point from the centre, so it patches into every phase module |
| Cover | Modulation, stepped | How many of the figure's shapes cover the point |
| Node | Pitch | The pitch of the node the point last reached |
| Arrive | Gate | A trigger at each node, and a gate while inside a chosen region |
| Per-shape phases | Phase, as voices | One phase per circle or corner, each offset by the figure's symmetry: seven voices for seven circles |

**How a figure gives pitch.** Three ways, and the module offers all three:

1. **Ratios of its lengths.** Pitch is 0.1 per octave, so a length ratio r is a pitch step of 0.1 times log2 of r. Each figure has a few lengths, and they make a small scale:
   - A square: side to diagonal is the square root of 2, exactly half an octave (0.05).
   - A six-sided figure and the flower of life: 1, the square root of 3, and 2. That is an octave (0.1) and a step of about 0.079 in between.
   - A five-pointed star: each length is in the golden ratio to the next shorter one, a step of about 0.069. It is not a note of any common scale, which is its charm.
2. **Counts as harmonics.** The numbers in a figure are read as members of the harmonic series. A cube has 6 faces, 8 corners, and 12 edges. The ratios 6 to 8 to 12 are a fourth, a fifth, and an octave, and 8 is the harmonic mean of 6 and 12. This is the oldest link between geometry and tuning, and it gives pure intervals.
3. **Nodes as notes.** Each node holds a note the player sets, or the module fills them from the patch's scale (the scale awareness trait, TR-08). Then the figure is a map of a melody, and the edges are the moves allowed.

Ways 1 and 2 give pitches outside the patch's tuning on purpose. A switch sends them through the patch's scale for players who want them in key.

| Figure | Its parts | What it is good for |
|---|---|---|
| Two circles (the vesica) | Two circles, each through the other's centre, and the lens between them | The simplest: two phases a fixed distance apart, and a gate while both overlap. A crossfade source |
| Seed of life | Seven circles: one, and six round it | Seven phases as voices, six-fold. A slow chord of modulation |
| Flower of life | Nineteen circles in the same pattern | The same, with rings: the Cover output steps as the point moves outward |
| Fruit of life | Thirteen circles that touch but do not overlap | Thirteen nodes for the Node output: a melody map with six-fold symmetry |
| Thirteen-circle star (often called Metatron's cube) | The thirteen centres, joined by every straight line | The richest edge set: many paths between the same notes. Outlines of the regular solids are traditionally traced in it |
| Five-pointed star | Five points, five crossing lines | Golden-ratio pitch steps. The line goes twice round the centre to visit five corners, so one walk is five against two |
| Six-pointed star | Two triangles, one pointing each way | Two walkers, one per triangle, a sixth of a turn apart: a three-step pattern and its offbeat answer |
| Nine triangles (the Sri Yantra) | Nine interlocked triangles making 43 small ones, in rings | A long path inward through rings, so Distance and Cover make a slow arc |
| Labyrinth | One path with no branches that winds to the centre and back | Long-form modulation: the path swings near the centre and away many times before it arrives. One phase over many bars gives a whole piece its shape |
| Tree of ten nodes | Ten nodes and 22 paths | A melody network: a walk with chance at each fork, close to Markov (MT-SEQ-10) but drawn |
| Golden spiral | A spiral that grows by the golden ratio each quarter turn | A rising pitch with an Angle phase: each turn is a fixed interval higher |
| Ring (torus) | A path wound round a ring, as in Knot above | Two phases in a set ratio, and a third value |

The counts in this table (circles, nodes, paths, triangles) are from general knowledge and were not checked on this date.

**Naming and wording.** These figures come from many cultures and mean a great deal to many people. We draw them ourselves, name them plainly, and in help we explain their geometry and the tuning ideas that follow from it. We make no claims beyond that. Where a figure has a traditional name, help gives it with its origin.

**Three types carry most of this section:**

- **Path:** a figure read by a phase, with X, Y, angle, and distance outputs, and a trigger at each marked point. Its presets are the rose, two pendulums, rolling circles, polygon and star, knot, and filling curve. A drawn path uses the curve data we already plan. It is the two-dimensional sibling of the Curve reader (MT-MOD-02).
- **Figure:** nodes, edges, and regions walked by a phase, as described above. It joins the other two, and adds pitch.
- **Pattern field:** a figure made of regions, read by a probe, with a region value, a distance to the nearest edge, and a crossing trigger. Flower of life, tilings, circle packing, plate figures, and moire are its kinds. Voronoi field (MT-MOD-09) already has this interface.

A Path output patched into a Pattern field's probe inputs joins the two: a rose walking across a flower of life.

**Where they belong.** These are not systems by the rule proposed below, because nothing in them moves by itself. But for a player they are part of the same story: forms from nature and mathematics that make music. A sixth group, Geometry, beside the design agent's five is the simplest home. That stretches the family from "systems" toward "systems and forms", which is a naming question for the family decision.

## Undecided things, and more from biology

Two more requests from Brylie (2026-11-28): a Schrödinger's box, "to remind us to not get too rigid in our thinking", and ideas from biology such as the double helix, marching ants, and foraging.

### Undecided until you look

These are classical models of famous stories from quantum physics, not quantum mechanics. Help must say so. What they share is musically new: an output that is not settled until something asks.

| Module | What it does musically | Teaches | Likely form |
|---|---|---|---|
| The box | Two inputs, A and B. Close the box and the output is a blend of both, drifting from all A toward all B as the odds change with time. A Look trigger settles it: the output snaps to A or to B by those odds, and an Alive or a Gone gate goes high. It is a crossfader that turns into a switch when observed | Being in two states at once, measurement, half-life | New type, small. Seeded, so a run repeats |
| Linked pair | Two outputs, each undecided. Look at either and both settle at once, always opposite (or always the same). Patch them far apart and two parts of a piece stay in step by chance | Correlation without a cable between them | Mode of The box |
| Half-life | A crowd of N units, each with the same chance of firing once and going quiet. Triggers come thick, then thin out, by the same share in each stretch of time | Exponential decay | Mode of Rain: a rate that falls as the crowd shrinks |
| Levels | A value that may only sit on set levels. It holds until pushed far enough, then jumps, and sends a trigger whose size is the jump | Where the word "quantise" comes from | Mode of Pitch quantiser (MT-PITCH-02) with a jump output |
| Soft wall | A threshold that a signal below it sometimes passes anyway, less often the further below it is | Tunnelling; and a comparator that feels human | Mode of Compare |
| Fringes | Events fall across a bar by a striped pattern of chance: likely here, never there. Two controls move and widen the stripes | Interference of two paths | Mode of Weighted dice, with a striped shape |
| Sharp or sure | One control trades how exactly a note's pitch is fixed against how exactly its time is fixed | The trade between time and frequency, which is real in every sound | New type, small |

**Collapse: a phrase that settles step by step.** Brylie asked for a wave function collapse module (2026-11-28). The physics idea is The box above. The name also belongs to a method for generating patterns from local rules, and that one makes a strong sequencer:

- Every step of a phrase starts able to be any note of the scale.
- Rules say which notes may sit next to which. The module learns them from a phrase the player plays or draws: whatever was a neighbour there is allowed here.
- On each trigger, the step with the fewest choices left settles on one note. That removes choices from its neighbours, and so on down the phrase.
- If a step runs out of choices, a Stuck trigger fires and the phrase starts again.

What makes it more than a Markov module:

- **Rules work in both directions.** A note limits what comes before it as well as after.
- **The player can pin steps.** Fix the first and last notes, or every downbeat, and the rest fills in to fit. That is composing by constraint.
- **It settles in public.** The face shows every step as a stack of faint possible notes that thins out as the phrase decides. The undecided state is the picture.

| | |
|---|---|
| Inputs | Collapse (trigger), Reset, a phase to play the settled phrase |
| Outputs | Pitch, Gate, Stuck (trigger), Choices left (a value that falls as it settles) |
| Atoms | Look, Seed, Reset |
| Teaches | How local rules make a whole pattern; solving by removing what cannot be |
| Likely form | New type. Two-dimensional use (steps by voices, for harmony that fits both along and across) comes later |
| Engine | One step settles per trigger, over at most 64 steps by 12 notes, so the work per trigger has a fixed upper limit |

### Electricity and magnetism

Brylie asked for anything from Maxwell's equations (2026-11-28). They describe electric and magnetic fields and how each makes the other. Several pieces are already here under other names, which is a lesson in itself: the same equations turn up in springs, circuits, and strings.

| Module | What it does musically | Teaches | Likely form |
|---|---|---|---|
| Charges | A few positive and negative charges on a plane. A probe reads the level of the field, its strength, and its direction as a phase. Charges can be moved by inputs, and crossing a contour fires a trigger | Fields and field lines; like repels, unlike attracts | A pattern field |
| Charged particle | A particle in a magnetic field circles at a rate set by the field's strength, and drifts. Between two strong ends it bounces back and forth while it spirals | Why a charge circles a field line; the aurora | New type in the Bodies group. Its angle is a phase |
| Polarisation | Two outputs that swing at the same rate, with one control for the offset between them: a line, then an ellipse, then a circle | What polarised light is | Preset of Path (the two pendulums) |
| Polariser | Takes an X and Y pair and passes only the part that lies along a chosen angle. Two of them at right angles pass nothing; a third between them lets some through again | Projection, and a famous surprise | New type, small. A sibling of Kaleidoscope |
| Induction | It answers only to change. A steady input gives nothing; a fast move gives a pulse, and the pulse opposes the move | A changing magnetic field makes a current | Mode of Slope detector (MT-ENV-05) |
| Spark | Charge builds until it jumps the gap, then starts again | The same build and release as a dripping tap or a nerve | This is Leaky bucket. One explainer covers all of them |
| Coil and capacitor | Energy swings between two stores and rings at one rate | The same equation as a weight on a spring | This is Spring. An explainer, not a module |
| Line and echo | A pulse travels down a line and part of it bounces back wherever the line changes | Reflection at a boundary; why cables are matched | Chain, or an abstraction of Delay Line |
| Radiation pattern | A phase goes in as a direction; the output is how strongly an aerial sends that way: lobes and nulls | Why an aerial has blind spots | Preset of Path, read for distance only |
| The demon | It watches values go by and sends the high ones one way and the low ones another, making order out of noise. A Memory control limits how long it can keep doing so before it must forget | Maxwell's demon: sorting is not free | New type, small. Mostly for the lesson |

### Prime numbers

Brylie also asked about the primes, and a result about primes on a particular line that makes an interesting curve.

**A correction first.** The famous statement is the Riemann hypothesis: the zeta function's nontrivial zeros all lie on one line, called the critical line. (It also has simple, well-understood zeros at the negative even numbers, which the hypothesis is not about. "Zeros" below means the nontrivial ones.) To our knowledge it has not been proved. What was in the news is likely the 2024 result of Guth and Maynard, which narrowed how many zeros could lie away from the line. It is progress, not a proof. Check this before any help text says anything about it.

The curve is real, and it is a good signal. Follow the zeta function along the critical line and its size rises and falls in an irregular wave that never repeats. Each place it touches zero is one of the zeros. Their spacing has a known character: they keep apart from each other, so they are never bunched like raindrops and never even like a clock. That is a kind of rhythm nothing else here makes.

There is a second, deeper link to sound. Riemann's formula builds the staircase of the primes by adding waves, one wave for each zero. The zeros act as the frequencies of the primes. It is the same idea as building a wave from harmonics, which is why writers speak of the music of the primes.

| Module | What it does musically | Teaches | Likely form |
|---|---|---|---|
| Zeta curve | A phase moves along the critical line. The output is the curve; each zero fires a trigger. One control sets how far along the line to start | The zeros, and spacing that is neither regular nor random | New type. It needs two things: a stored table of zeros for the triggers, and the curve's values between them, from a stored table of samples or from a known formula worked out off the audio thread. Either way the audio thread only reads a table |
| Prime staircase | A staircase that steps at every prime, and beside it the smooth curve made by adding waves. A Waves control adds them one at a time, and the smooth curve grows corners at the primes | That the primes hide a spectrum | New type. The face is the lesson |
| Sieve | A stack of clock dividers, one per prime: every 2nd step, every 3rd, every 5th. Each is a rhythm, and the steps that none of them hit are primes, up to a limit: with dividers for 2, 3, and 5, every unhit step from 7 to 47 is prime, and 49 is the first wrong one, because it is 7 times 7. Each new divider moves the limit to the square of the next prime. Step 1 is never hit and is not a prime | The sieve of Eratosthenes, how rhythm layers make gaps, and why the sieve needs each new prime | Abstraction of Clock Divide and Logic, both built. Patchable today for a few dividers, within that limit |
| Prime steps | A counter that fires on prime-numbered steps, and gives the gap since the last one as a value | That gaps between primes grow, but small ones keep turning up | New type, small |
| Number spiral | The counting numbers wound in a square spiral, with primes marked. Lines of primes appear along diagonals. A path walks it and fires at each prime | A pattern in the primes anyone can see | A pattern field (the Ulam spiral) |
| Factors | Each count plays a voice for every prime that divides it: 12 plays the 2 voice twice and the 3 voice once. Primes play alone | Every number is built from primes in one way | New type. A system that makes voices |
| Prime partials | A tone with only its prime-numbered harmonics, or a tuning that uses ratios of primes up to a chosen limit: 3, then 5, then 7 | Just tuning by prime limit | Mode of Additive oscillator (MT-SRC-03) and of Scale and tuning (MT-PITCH-03) |
| Hailstones | Halve an even number; triple an odd one and add one. The values climb and crash before reaching 1, and nobody has proved they always do | An unsolved problem a child can state | Mode of Iterated map (MT-MOD-11) |
| Sums of two primes | For each even number, how many ways it can be written as two primes. The counts make a ragged, rising cloud | Another unsolved problem with a visible shape | Mode of Prime steps |

**The zeta curve and tuning.** Brylie marked the zeta curve as especially important for natural-sounding music (2026-11-28). There is a concrete reason in tuning theory. Read along the critical line, the height of the curve measures how well an equal division of the octave fits the harmonic series, with each position on the line standing for a number of steps per octave. The tall peaks fall at the divisions musicians already know to sound good: 12 steps, and also 19, 22, 31, 41, and 53. So the same curve that hides the primes also ranks tunings by how closely they match the pure intervals of natural sound. This is from general knowledge of the tuning literature and was not checked on this date; check it before it goes into a spec or help.

That gives the Zeta curve module a second job beside modulation and rhythm:

- **A tuning finder.** A Steps output gives the number of steps per octave at the current position, and a Fit output gives the height of the curve there. Sweep along the line and stop at a peak, and the patch's tuning (Scale and tuning, MT-PITCH-03) can take that division.
- **Tuning that moves.** Let the position drift slowly and the tuning passes through good divisions and the rough ground between them.

It moves the Zeta curve up the list of what to build: it is the one module here that links the mathematics of the primes to how a chord sounds.

The Sieve also has a history in music: composers have used sieves of numbers to build scales and rhythms. That belongs in its explainer.

### From biology

| Module | What it does musically | Teaches | Likely form |
|---|---|---|---|
| Double helix | A stored string of four letters, read along one strand as notes. The second strand always plays the partner letter, so two voices move in fixed answer to each other, half a turn apart. Read three letters at a time and 64 codes give about 20 notes, so several spellings play the same note. A Copy trigger rewrites the string with rare mistakes | Paired strands, the genetic code, mutation | New type. Its copy step is shared with Evolving melody |
| Ant trails | Walkers cross a Figure from node to node and leave a scent on each edge. Scent fades. Walkers prefer strong scent, so short and well-used routes win, and a melody settles into habits. Block a route and it finds another | How a colony finds a short path with no leader | New type on top of Figure |
| Forager | A walker searches an input or a field for high values. It lingers where things are rich, uses them up, and leaves when the next place looks better | When to stay and when to move on | New type. It listens |
| Seeker | It moves straight, then turns at random. When things are getting better it turns less often, so it drifts uphill on any input | How a bacterium finds food with no map | New type, small. A noisy hill-climber |
| Pulse and breath | Heartbeat, with a slower breath cycle that speeds the pulse on the way in and slows it on the way out | Why a healthy heart is not a metronome | Abstraction of Heartbeat and an LFO |
| Gait | Four gates for four legs. As Speed rises the pattern changes by itself: walk, then trot, then gallop | How a body switches rhythm with speed | New type. A polyrhythm that follows tempo |
| Day clock | A slow cycle with a period of its own that drifts until a light input pulls it into step. Move the light and it takes several cycles to catch up | Body clocks and jet lag | Follower clock at a long period |
| Division | One voice becomes two, each a slightly changed copy. They divide again, up to sixteen, and old voices fade | Growth by doubling, inheritance | New type. A system that makes voices |
| Wiring | Triggers that often arrive together grow a link, and then one starts to fire the other. Links not used fade | How practice builds habits in a brain | New type, close to Neurons |
| Drift | A pool of values copied each cycle with random loss. Variety shrinks until one value is left; a push adds new ones | Chance alone can remove variety | Mode of Shuffle |
| Slime net | A net grows between chosen nodes of a Figure, then thins to the routes that carry the most | How a simple organism builds an efficient network | Mode of Ant trails |
| Partners | Two cycles that each run only while the other does. Starve one and both fade | Living things that depend on each other | Mode of Population |

### Atoms these suggest

Four more candidates for the atoms table. Each already appears in three or more ideas, which is the test for promoting one.

| Atom | What it is | Seen in |
|---|---|---|
| **Look** | The output is settled only when a trigger asks | The box, Linked pair, Levels, Collapse; and Sample and Hold, which is built |
| **Trail** | What is used grows stronger, and what is left alone fades | Ant trails, Slime net, Wiring, Tiring |
| **Copy with error** | Each copy differs a little from its source | Double helix, Division, Drift, Evolving melody |
| **Seek** | It moves toward more of something on an input | Seeker, Forager, Regulator, Follower clock |

Trail and Seek are the two that most make a patch feel alive, because both make the patch's own history shape what it does next.

## What the engine needs to grow

These modules ask more of the engine than the conventional ones do. Each point below is work to plan, so that it is ready when the modules are. The hard rules in `AGENTS.md` (real-time safety, block-size invariance, determinism) stay; the list says how these modules meet them. Items marked "to check" were not verified in code on this date.

1. **Chaos and determinism.** Hard rule 3 asks for bit-identical output from the same patch and input, and NFR-SND-03 asks that desktop and exported output match "within a documented tolerance". A chaotic system multiplies any tiny difference until the two runs have nothing in common. So a chaotic module must use arithmetic that gives the same bits everywhere: a fixed step counted in samples, and no maths function whose result differs between platforms. Where that is not possible, the tolerance rule needs an exception written down. This is a decision to take early, before the first Attractor is built, so it does not hold the others up.
2. **Block-size invariance** (hard rule 2). A system that steps every N samples must count samples, never blocks.
3. **Real-time safety** (hard rule 1). All state is allocated when the module is made: a fixed number of bodies, cells, or birds, chosen by a parameter with a fixed upper limit.
4. **Cost.** Fields are the expensive group. A ripple tank or a reaction on a 64 by 64 grid is 4,096 cells per step. Run the field at a slow internal rate and smooth the probe's output, so the cost does not depend on the sample rate. Fields are worth this work: they are the most striking modules in the set. Give them a shared field engine and a benchmark, so every later field type starts from a known cost.
5. **Staying finite.** A simulation pushed hard can blow up. Each one needs a bound that holds for any input, as ADR-0037 asks.
6. **Voices.** Two different things, and each module must say which it is:
   - **One system per voice:** every note gets its own pendulum. This is what polyphony gives any module.
   - **One system that makes voices:** each bird of a Flock is one voice of its output. Note input already makes voices from a parameter, so the engine allows it.
   - "Voices that lean" reads all voices of one cable and couples them. A module can read every voice of an input (to check: whether it can do so within one voice's processing).
7. **Faces need more than four numbers.** A state space face draws a trail and often a background. Display values carry four numbers per module. The XY scope's probe path can carry a trail. A field's picture is state, not a signal, so it needs a new route to the screen. Plan it with the per-voice display work: both are "show the system's state", which is the design's stated aim.

8. **Finding an idea by its name.** A player should be able to type "heartbeat" or "flower of life" and get it. Today the browser lists module types, built-in abstractions, slots, and library items. Either a preset of a module becomes a fifth kind of row, or each named idea ships as a built-in abstraction that wraps its type. The second works today.

## What can be patched today

Some of these are small enough to build as sub-patches from modules we have, as the synth voice was. That gives players the idea now and tells us which ones deserve a module of their own. None of these patches was built and tried on this date.

| Idea | From built modules | What a real module would add |
|---|---|---|
| Leaky bucket | Add and 1-Sample Delay as a running sum, Scale for the leak, Compare for the threshold, Multiply to empty it | Timing that does not change with the sample rate; one tile |
| Regulator | Subtract for the error, Scale, then a running sum from Add and 1-Sample Delay, and Clamp | The same |
| Tiring | A Leaky bucket whose level scales an Amplifier | The same |
| Spring | Two running sums in a loop, with Scale for stiffness and damping | The same, and it stays stable at any setting |
| Rain | Noise into Compare with a high threshold | A density in events per second, and a seed that repeats |
| Tides, Seasons | LFOs and phase modules | Nothing but convenience; ship them as abstractions |
| Wind chimes | Noise, Slew, Compare | Real pendulums, once Pendulum exists |

A sub-patch that feeds a sum back through a 1-Sample Delay runs once per sample, so its speed depends on the sample rate. That is the main reason the small ones still want a module in the end. If Rain works well as a patch, the survey's gap for random impulses is smaller than it says.

## A "Systems" family?

The design agent proposes a ninth module family, Systems, with its five groups as sections in the module browser.

**Facts.**

- ADR-0029 fixes eight families, and every descriptor must name one. It chose eight for two reasons: eight colours stay distinct, and people hold about seven groups in mind.
- ADR-0052 removes family colours. So the first reason is gone, and the second remains.
- The module browser lists modules by family, then by name. It has no sections inside a family. Search already matches a module's category, a finer label under the family.
- Families today sort by what a module is for in a patch: Sources, Modulation, Time and rhythm, Notes and pitch. The system modules are spread across those: an Attractor makes modulation, an Automaton makes rhythm, a Markov module makes notes, and a Chain can make sound.

**Options.**

1. **A ninth family, Systems.** Easy to find and it teaches, as the design agent says. But it sorts by how a module works, where the other eight sort by what it is for. A player looking for "something to make a rhythm" must then look in two places, and each new module starts an argument about where it goes.
2. **Keep the eight families, and make Systems a second way to browse.** Each module stays in the family of what it makes. Its category is one of the five groups, which search already finds. The browser gains a "Systems" view that lists them by group.
3. **A section inside Modulation.** Simple, but wrong for the ones that make rhythm, notes, or sound.

**A leaning, not a decision:** option 1. An earlier draft of this document leaned to option 2. It changed because natural systems are a main user story, and a family is how the module browser says "this is a main thing we offer"; option 2 would make the project's central idea a filter. Brylie's steer was about the user story, not about families, so this is a reading of it, and Brylie can overrule it.

**The boundary rule, and where it fails.** A candidate rule: *a module is in Systems when what it does next depends on its own history, not only on its settings and the time.* An LFO, an envelope, and a step sequencer are out: their output can be read off their settings. An Attractor, a Flock, and an Automaton are in, even when what they put out is rhythm or notes. Each still names what it makes in its category, so a search for "rhythm" finds it. The rule fails in three places, and the decision must cover them:

- **Geometry.** A traced figure has no history, so the rule leaves it out. The user story puts it in. A sixth group would be an exception made on purpose.
- **Fixed sequences.** Golden rhythm and Thue-Morse never change once chosen. They belong with the Rewriting type that makes them, which the rule admits only because the type can also grow.
- **Modes of other types.** Rain, Shuffle, Cooldown, and Spring are proposed as modes of Random, Noise, Gate shaper, and Slew. A mode shares its type's family, so they would not appear under Systems unless they ship as abstractions of their own.

**The name.** The vision uses "living systems" for whole patches. A family called Systems would use the same word for a kind of module. Other names to weigh: Nature, Natural systems, Processes, Systems and forms.

This changes ADR-0029, so it needs a decision record of its own.

## A first set

If M3 builds a handful first, these give the most life for the least work, and between them they try out most of the atoms.

| Order | Module | Why first |
|---|---|---|
| 1 | Regulator | The clearest "it listens" module; tiny |
| 2 | Leaky bucket, Cooldown, Tiring | Three small event modules that make rhythm react to density |
| 3 | Rain, Shuffle, Weighted dice | Close two gaps and one smaller addition from the VCV survey as modes of what exists |
| 4 | Spring | A slew that bounces; familiar, and it sets up Chain |
| 5 | Figure, starting with the seed and flower of life | A main request, cheap to run, and it gives modulation, phases, gates, and pitch from one module |
| 6 | Path, with Kaleidoscope | Cheap, beautiful, and visible at once on the XY Scope we have. It proves the Trace idea, and Kaleidoscope works on every later system |
| 7 | Driven oscillator | The first chaotic module with a Push, so it settles the determinism question |
| 8 | Rewriting | One type gives four of the proposed rhythm and melody modules |
| 9 | Attractor modes | Planned already; adds Rössler, Double scroll, and Chemical clock as modes |
| 10 | Zeta curve | Brylie's priority. Modulation, a rhythm found nowhere else, and a tuning finder, read from stored tables |
| In parallel | Fields, Flock, Neurons, Evolving melody | The flagship modules. They need the engine work above, so that work starts at the same time as the small modules, not after them |

## Open

- A use case for natural systems. The personas include Ilmari, the generative musician, but no use case yet says "build a patch from systems that behave like nature". Since it is a main user story, it should be written down in `docs/product/`.
- Which candidate atoms (Push, Energy, Probe, Coupling, Damping, Count) become catalogue traits. Decide each when three modules share it.
- Whether system modules with one modulation output declare the Modulator role, so they can be swapped into slots.
- The determinism rule for chaotic modules (engine point 1).
- How a field's picture reaches the screen (engine point 7).
- The Systems family question: whether it is a family, its name, its boundary rule, and whether Geometry is its sixth group.
- Which figures ship first, and under what names.
- Which of these become catalogue entries, and when. Nothing in the catalogue is changed here.
- Explainers this would need in `docs/learn/`: chaos and sensitivity, feedback and balance, rewriting systems, and coupled oscillators.

## References

From general knowledge, not checked on this date. Each is the published source of an idea named above.

- E. N. Lorenz, "Deterministic Nonperiodic Flow", 1963. O. E. Rössler, "An Equation for Continuous Chaos", 1976. M. Hénon, "A Two-dimensional Mapping with a Strange Attractor", 1976.
- R. M. May, "Simple Mathematical Models with Very Complicated Dynamics", 1976 (the logistic map).
- B. van der Pol, on relaxation oscillations, 1926. G. Duffing, on forced oscillations, 1918.
- A. Lindenmayer, "Mathematical Models for Cellular Interactions in Development", 1968.
- A. M. Turing, "The Chemical Basis of Morphogenesis", 1952.
- Y. Kuramoto, on coupled oscillators, 1975. A. T. Winfree, on biological rhythms and synchrony, 1967.
- C. W. Reynolds, "Flocks, Herds, and Schools: A Distributed Behavioral Model", 1987.
- A. J. Lotka, 1925, and V. Volterra, 1926, on predator and prey. W. O. Kermack and A. G. McKendrick, 1927, on epidemics.
- P. Bak, C. Tang, and K. Wiesenfeld, "Self-Organized Criticality", 1987 (the sandpile).
- C. G. Langton, "Studying Artificial Life with Cellular Automata", 1986. M. Gardner, on Conway's Game of Life, Scientific American, 1970. S. Wolfram, on elementary cellular automata, 1983.
- W. R. Ashby, "Design for a Brain", 1952 (the homeostat, for Regulator).
- E. F. F. Chladni, on figures in vibrating plates, 1787. H. Vogel, "A Better Way to Construct the Sunflower Head", 1979. R. Penrose, on tilings that never repeat, 1974.
- L. Euler, 1739, for the lattice of fifths and thirds. G. T. Toussaint, "The Euclidean Algorithm Generates Traditional Musical Rhythms", 2005, for rhythm as points on a circle.
- C. Hohnerlein, M. Rest, and J. O. Smith III, "Continuous Order Polygonal Waveform Synthesis", 2016, for a polygon read as a wave.
- P. Merrell, "Example-Based Model Synthesis", 2007, and M. Gumin, 2016, for generating patterns by settling cells under local rules.
- For the zeta function and equal divisions of the octave: the writing of G. W. Smith and others in the tuning community, to be found and checked when the module is designed.
- J. C. Maxwell, "A Dynamical Theory of the Electromagnetic Field", 1865. B. Riemann, 1859, on the number of primes below a given size. S. Ulam, 1963, for the number spiral. L. Guth and J. Maynard, 2024, on zeros of the zeta function. M. du Sautoy, "The Music of the Primes", 2003, for the general reader. I. Xenakis, "Formalized Music", for sieves in composition.
- E. Schrödinger, 1935, for the cat in the box. J.-L. Deneubourg and others, 1990, on ants choosing the shorter of two bridges; M. Dorigo, 1992, on ant colony methods.
- E. L. Charnov, "Optimal Foraging, the Marginal Value Theorem", 1976. H. C. Berg, on how bacteria swim toward food. D. O. Hebb, "The Organization of Behavior", 1949.
- J. J. Collins and I. N. Stewart, 1993, on coupled oscillators and animal gaits. J. D. Watson and F. H. C. Crick, 1953, for the double helix.
- S. Kirkpatrick, C. D. Gelatt, and M. P. Vecchi, "Optimization by Simulated Annealing", 1983.
