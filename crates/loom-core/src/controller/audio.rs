//! Audio output and input: one group of [`Controller`]'s methods.

use super::*;

/// # Audio output and input
///
/// Output safety, fades, panic, and the audio input
/// (`docs/design/output-safety.md`).
impl Controller {
    /// Changes the safety stage on the final output (ADR-0019). The output
    /// dips to silence for a moment and fades back in, so the change never
    /// clicks.
    pub fn set_output_safety(&mut self, mode: OutputSafety) -> Result<(), LoomError> {
        self.send(Command::SetOutputSafety(mode))
    }

    /// Asks the engine to fade its output to silence over at most 10 ms and
    /// stay silent, before the host stops it (output safety, rule 14). The
    /// engine then reports [`Engine::is_silent`](crate::Engine::is_silent).
    /// There is no way back: build a new engine to play again.
    pub fn fade_out(&mut self) -> Result<(), LoomError> {
        self.send(Command::FadeOut)
    }

    /// Runs panic (`docs/design/panic.md`): the engine fades its output to
    /// silence over 10 ms, releases every held note and resets every
    /// module at the silent frame, and fades back in over 10 ms. It is not
    /// an edit: the graph, the parameters, and the transport do not change.
    /// A panic sent while one is running is ignored.
    pub fn panic(&mut self) -> Result<(), LoomError> {
        self.send(Command::Panic)
    }

    /// Replaces the input ring at the next audio buffer boundary. `None`
    /// disconnects the input; removed rings are freed on the control thread.
    pub fn set_audio_input(&mut self, input: Option<crate::audio_input::InputReader>) -> Result<(), LoomError> {
        self.collect_garbage();
        self.send(Command::SetAudioInput(input))
    }
}
