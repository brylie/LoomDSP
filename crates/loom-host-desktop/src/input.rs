//! Desktop audio input, connected to the engine by a bounded SPSC ring.

use crate::{AudioOptions, device_name, find_device};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use loom_core::audio_input::{InputReader, InputStatus, InputWriter, MAX_INPUT_CHANNELS, input_ring};

/// Why an input stream could not open.
///
/// The categories come from `cpal`'s error kinds and the macOS permission
/// check, never from the words of a message. CoreAudio, ALSA, and PulseAudio
/// report a refused permission as a kind. WASAPI does not, so a refusal there
/// arrives as [`InputError::Other`], with the backend's text.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    /// The system refused access to the microphone.
    #[error("microphone permission denied")]
    PermissionDenied,
    /// No device matched, or the device is gone or busy.
    #[error("{}", unavailable_text(.device, .detail))]
    Unavailable {
        /// The device asked for, if one was named.
        device: Option<String>,
        /// The backend's text, when it reported the problem.
        detail: Option<String>,
    },
    /// The device does not offer the output's sample rate.
    #[error("\"{device}\" does not offer {rate} Hz")]
    UnsupportedRate {
        /// Device name.
        device: String,
        /// Requested rate.
        rate: u32,
    },
    /// The device has no 32-bit float configuration.
    #[error("audio device `{0}` does not support 32-bit float input")]
    UnsupportedFormat(String),
    /// The backend refused the chosen configuration.
    #[error("audio error: {0}")]
    UnsupportedConfig(String),
    /// Any other backend failure, with the backend's text.
    #[error("audio error: {0}")]
    Other(String),
}

impl InputError {
    /// Sorts a backend error by its kind, keeping its text for diagnostics.
    fn from_backend(error: &cpal::Error, device: Option<&str>) -> Self {
        let detail = error.to_string();
        match error.kind() {
            cpal::ErrorKind::PermissionDenied => Self::PermissionDenied,
            cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::DeviceBusy => Self::Unavailable {
                device: device.map(str::to_string),
                detail: Some(detail),
            },
            cpal::ErrorKind::UnsupportedConfig => Self::UnsupportedConfig(detail),
            _ => Self::Other(detail),
        }
    }
}

/// The text of [`InputError::Unavailable`].
fn unavailable_text(device: &Option<String>, detail: &Option<String>) -> String {
    match (detail, device) {
        (Some(detail), _) => format!("audio error: {detail}"),
        (None, Some(device)) => format!("no audio input device found matching `{device}`"),
        (None, None) => "no audio input device found".to_string(),
    }
}

/// Whether the operating system has refused microphone access to this app.
/// Other platforms report permission errors when opening the stream.
#[cfg(not(target_os = "macos"))]
pub fn microphone_access_denied() -> bool {
    false
}

/// CoreAudio can deliver silent buffers after microphone access is denied,
/// without reporting an error. Ask AVFoundation for the actual permission.
#[cfg(target_os = "macos")]
pub fn microphone_access_denied() -> bool {
    use objc2::{class, msg_send, runtime::AnyObject};

    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {
        static AVMediaTypeAudio: *const AnyObject;
    }
    // SAFETY: AVMediaTypeAudio is AVFoundation's static NSString constant;
    // the class method accepts that constant and returns an NSInteger.
    let status: isize =
        unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: AVMediaTypeAudio] };
    matches!(status, 1 | 2) // restricted or denied
}

/// Request access once when a bundled app first opens input. The completion
/// runs asynchronously; the input watcher observes the resulting status.
#[cfg(target_os = "macos")]
fn request_microphone_access() {
    use block2::RcBlock;
    use objc2::{class, msg_send, runtime::AnyObject};
    use objc2_foundation::ns_string;
    use std::sync::atomic::{AtomicBool, Ordering};

    static REQUESTED: AtomicBool = AtomicBool::new(false);
    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {
        static AVMediaTypeAudio: *const AnyObject;
    }
    // SAFETY: NSBundle returns the process bundle and its Info.plist value;
    // neither is retained beyond the call. A missing key means this is an
    // unbundled developer executable, which cannot request access itself.
    let has_description = unsafe {
        let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
        let value: *mut AnyObject =
            msg_send![bundle, objectForInfoDictionaryKey: ns_string!("NSMicrophoneUsageDescription")];
        !value.is_null()
    };
    if has_description && !REQUESTED.swap(true, Ordering::AcqRel) {
        let completion = RcBlock::new(|_: objc2::runtime::Bool| {});
        // SAFETY: AVMediaTypeAudio is a static NSString, the block has the
        // documented BOOL signature, and AVFoundation copies it for the
        // asynchronous completion. No UI or audio work runs in the block.
        unsafe {
            let _: () = msg_send![class!(AVCaptureDevice), requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*completion];
        }
    }
}

/// Other platforms request permission through their input backend.
#[cfg(not(target_os = "macos"))]
fn request_microphone_access() {}

/// A running input stream. Dropping it closes the device.
pub struct AudioInput {
    _stream: cpal::Stream,
    name: String,
    channels: usize,
    status: InputStatus,
}

impl AudioInput {
    /// Opens and starts an input at the output's sample rate. The returned
    /// reader is installed on the controller. `output_buffer` sizes the ring
    /// and must be the output buffer size in frames.
    pub fn start(options: &AudioOptions, output_buffer: usize) -> Result<(Self, InputReader), InputError> {
        request_microphone_access();
        if microphone_access_denied() {
            return Err(InputError::PermissionDenied);
        }
        let host = cpal::default_host();
        let device = match &options.device {
            None => host.default_input_device(),
            Some(wanted) => {
                let devices = host
                    .input_devices()
                    .map_err(|error| InputError::from_backend(&error, Some(wanted)))?;
                find_device(devices, wanted, options.exact)
            }
        }
        .ok_or_else(|| InputError::Unavailable {
            device: options.device.clone(),
            detail: None,
        })?;
        let name = device_name(&device);
        let backend = |error: cpal::Error| InputError::from_backend(&error, Some(&name));
        let default = device.default_input_config().map_err(backend)?;
        let config = choose_input_config(
            &name,
            default,
            || device.supported_input_configs().map_err(backend),
            options,
        )?;
        let device_channels = usize::from(config.channels.max(1));
        let channels = device_channels.min(MAX_INPUT_CHANNELS);
        let input_buffer = options.buffer_size.map_or(output_buffer, |size| size as usize);
        let (writer, reader) = input_ring(output_buffer.max(input_buffer).max(1), channels);
        let status = writer.status();
        let failed = status.clone();
        let mut callback = input_callback(writer, device_channels);
        let stream = device
            .build_input_stream::<f32, _, _>(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| callback(data),
                move |_: cpal::Error| failed.fail(),
                None,
            )
            .map_err(backend)?;
        stream.play().map_err(backend)?;
        Ok((
            Self {
                _stream: stream,
                name,
                channels,
                status,
            },
            reader,
        ))
    }

    /// The device name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Number of channels exposed to patches, at most eight.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// False after the stream reports an error.
    pub fn is_live(&self) -> bool {
        self.status.is_live()
    }

    /// A handle for watching stream failure from another thread.
    pub fn status(&self) -> InputStatus {
        self.status.clone()
    }
}

/// Chooses the float input format exposing the most usable channels at the
/// output rate (audio-input.md, One duplex stream, or two?).
fn choose_input_config<I>(
    name: &str,
    default: cpal::SupportedStreamConfig,
    ranges: impl FnOnce() -> Result<I, InputError>,
    options: &AudioOptions,
) -> Result<cpal::StreamConfig, InputError>
where
    I: IntoIterator<Item = cpal::SupportedStreamConfigRange>,
{
    let rate = options.sample_rate.unwrap_or(default.sample_rate());
    let valid_default =
        default.sample_format() == cpal::SampleFormat::F32 && default.sample_rate() == rate && default.channels() > 0;
    let mut best = valid_default.then_some(default);
    // Prefer the requested buffer when bounds are known, then maximize
    // usable channels without opening unnecessary channels beyond eight.
    let score = |channels: u16, buffer: &cpal::SupportedBufferSize| {
        let compatible = match (options.buffer_size, buffer) {
            (Some(frames), cpal::SupportedBufferSize::Range { min, max }) => (*min..=*max).contains(&frames),
            _ => true,
        };
        (
            compatible,
            usize::from(channels).min(MAX_INPUT_CHANNELS),
            std::cmp::Reverse(channels),
        )
    };
    match ranges() {
        Ok(ranges) => {
            for range in ranges {
                if range.channels() == 0
                    || range.sample_format() != cpal::SampleFormat::F32
                    || !(range.min_sample_rate()..=range.max_sample_rate()).contains(&rate)
                {
                    continue;
                }
                if best.as_ref().is_none_or(|current| {
                    score(range.channels(), range.buffer_size()) > score(current.channels(), current.buffer_size())
                }) {
                    best = Some(range.with_sample_rate(rate));
                }
            }
        }
        Err(error) if best.is_none() => return Err(error),
        Err(_) => {} // The default is usable even when enumeration fails.
    }
    let supported = best.ok_or_else(|| {
        if options.sample_rate.is_some() {
            InputError::UnsupportedRate {
                device: name.to_string(),
                rate,
            }
        } else {
            InputError::UnsupportedFormat(name.to_string())
        }
    })?;
    let mut config = supported.config();
    if let Some(frames) = options.buffer_size {
        config.buffer_size = cpal::BufferSize::Fixed(frames);
    }
    Ok(config)
}

/// Converts interleaved device samples into fixed-size frames without allocation.
fn input_callback(mut writer: InputWriter, channels: usize) -> impl FnMut(&[f32]) + Send + 'static {
    move |data| {
        writer.callback_received();
        for frame in data.chunks_exact(channels) {
            let mut input = [0.0; MAX_INPUT_CHANNELS];
            let count = channels.min(MAX_INPUT_CHANNELS);
            input[..count].copy_from_slice(&frame[..count]);
            let _ = writer.push(input);
        }
    }
}

#[cfg(test)]
mod tests;
