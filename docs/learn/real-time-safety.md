# Real-time safety

Why the audio thread must never allocate memory, take a lock, wait, print, or touch a file, and how LoomDSP is built so that it never has to. Written for musicians first, with the code rules at the end.

- **Kind:** explainer.
- **Used by:** [ADR-0001](../adr/0001-lock-free-control-audio-boundary.md) (lock-free boundary), [architecture](../dev/architecture.md), [AGENTS.md](../../AGENTS.md#hard-rules) (hard rule 1).
- **Related:** [Block-size invariance](block-size-invariance.md), [Stopping a stream without a click](stopping-a-stream.md).
- **Code:** `crates/loom-core/src/engine.rs`, `crates/loom-modules/tests/no_alloc.rs`.

## The deadline

The audio device plays sound at a steady rate, and it asks for the next buffer at a steady rate. With 128 frames at 48 kHz, a new buffer is due every **2.67 ms**. The engine must fill it before the device needs it, every time, for hours.

If one buffer is late, the device has nothing to play. It plays silence or repeats the last samples. You hear a click or a gap, called a **dropout**. On stage, one is too many.

Compare a video game. A frame that takes 20 ms instead of 16 ms is a small stutter. An audio buffer that is 2 ms late is a loud pop. And you cannot catch up later, because the sound has already played.

So the rule is not "be fast on average". It is **never be late**. The slowest buffer matters, not the typical one.

## What can make a buffer late

Four things are fast most of the time and very slow once in a while.

| Action | Why it can be slow |
|---|---|
| **Allocating memory** | The memory manager may need to ask the operating system for more, or search for a free block. It may also take an internal lock that another thread holds. |
| **Taking a lock** | If another thread holds it, you wait for that thread. The UI thread might be drawing a window and holding it. This is called **priority inversion** when a low-priority thread blocks a high-priority one. |
| **Waiting** (sleeping, waiting for a channel) | By definition, you give up your turn and the system decides when you get it back. |
| **Printing, files, network** | Each is a request to the operating system, and the system may make you wait for the disk or the terminal. |

Freeing memory counts as allocation, because it does the same bookkeeping. Even a quick operation on average is dangerous: a lock that is free 99.99% of the time is not free when it matters.

The deadline does not change when the CPU is busy elsewhere. A laptop that is also running a browser, a DAW, and a video call is the normal case, not the exception.

## How LoomDSP avoids all four

Every other kind of work still has to happen: the UI changes parameters, loads patches, draws meters. LoomDSP splits the work between two threads ([ADR-0001](../adr/0001-lock-free-control-audio-boundary.md)):

- The **control thread** can do anything. It owns the editable graph, builds the next schedule, allocates all the memory, and loads modules.
- The **audio thread** only executes. It never creates and never destroys.

They talk through **bounded queues** that neither side ever waits on:

| Direction | What passes | How it avoids the problem |
|---|---|---|
| Control to audio | Parameter changes, MIDI, "here is a new schedule" | Messages are small and fixed-size. A full queue means the control side tries again. The audio side only reads what is there. |
| Audio to control | Retired schedules and removed modules, meter readings, scope chunks | The audio thread **hands old memory back** to be freed on the control thread. If a telemetry queue is full, the data is dropped and counted, never waited for. |

New memory is built on the control thread and **moved** to the audio thread. Old memory moves back to be freed there. The audio thread uses it but never makes it. Module instances sit in slots made once at start. A new module is created on the control thread and moved into a free slot.

A UI drawing a meter does the same in the other direction: the engine writes the latest number into an atomic, and the UI reads it when it draws. Neither waits.

## Proving it

A promise in a document is not enough. `crates/loom-modules/tests/no_alloc.rs` runs the engine under `assert_no_alloc`, a tool that stops the test if any allocation or free happens inside the guarded code. The test edits the patch between buffers, so schedule swaps and parameter changes are covered. If a module author adds a `Vec::push` or a `format!` to `process`, the test fails.

## Rules for code on the audio thread

Everything reachable from `Engine::process` or `Module::process` must:

- Not allocate or free. Make buffers in the module's constructor, and size them for the largest case. `Vec::push`, `Box::new`, `format!`, and `clone` of a vector can all allocate.
- Not lock (`Mutex`, `RwLock`) and not wait (`sleep`, blocking channels, `join`).
- Not print, log, or touch files, sockets, or the clock in a way that may block.
- Avoid unbounded loops. A loop that depends on the data (for example a search that can run long) can miss the deadline as surely as a lock can.

When a module needs a big table, it receives it as memory the control thread allocated. When it needs to report something, it writes to a latest-value table, such as `Module::displays`.

## In other systems

Every serious audio engine follows these rules. Software that breaks them is the software that crackles under load. There is a good account of the reasons in Ross Bencina's "Real-time audio programming 101: time waits for nothing" (rossbencina.com).

## Further reading

- Ross Bencina, "Real-time audio programming 101: time waits for nothing", on why allocation, locks, and system calls break real-time code.
