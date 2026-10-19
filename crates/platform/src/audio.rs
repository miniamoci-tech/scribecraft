//! Audio output through cpal.
//!
//! The callback is real-time code: it takes the transport lock with `try_lock` (playing
//! silence for one buffer if the UI holds it), converts rate and channels with a converter
//! allocated up front, and never allocates, blocks or panics.

use std::sync::{Arc, Mutex, TryLockError};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use scribecraft_dsp::Converter;
use scribecraft_engine::Transport;

use crate::PlatformError;

/// A running output stream. Dropping it stops the sound.
pub struct AudioOutput {
    _stream: cpal::Stream,
    device_name: String,
    sample_rate: u32,
    channels: u16,
}

impl std::fmt::Debug for AudioOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioOutput")
            .field("device", &self.device_name)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .finish()
    }
}

impl AudioOutput {
    /// Starts rendering `transport` to the default output device.
    pub fn open(transport: Arc<Mutex<Transport>>) -> Result<Self, PlatformError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(PlatformError::NoOutputDevice)?;
        let device_name = device.name().unwrap_or_else(|_| "default output".into());
        let supported = device.default_output_config().map_err(|e| PlatformError::Device(e.to_string()))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let (rate, channels) = (config.sample_rate.0, config.channels);
        let (src_rate, src_channels) = {
            let t = scribecraft_engine::lock_transport(&transport);
            (t.sample_rate(), u16::try_from(t.channels()).unwrap_or(1))
        };
        let mut converter = Converter::new(src_rate, src_channels, rate, channels);
        let on_error = |e: cpal::StreamError| log_error(&e);
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| fill(&transport, &mut converter, out),
                on_error,
                None,
            ),
            cpal::SampleFormat::I16 => {
                // Converted through a scratch buffer sized once for the largest callback seen.
                let mut scratch: Vec<f32> = Vec::with_capacity(16_384);
                device.build_output_stream(
                    &config,
                    move |out: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        if scratch.len() < out.len() {
                            // Grows only on the first callbacks; steady state never allocates.
                            scratch.resize(out.len(), 0.0);
                        }
                        let Some(buf) = scratch.get_mut(..out.len()) else { return };
                        fill(&transport, &mut converter, buf);
                        for (o, s) in out.iter_mut().zip(buf.iter()) {
                            *o = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                        }
                    },
                    on_error,
                    None,
                )
            }
            other => return Err(PlatformError::Device(format!("the device's sample format {other:?} is not supported yet (supported: f32, i16)"))),
        }
        .map_err(|e| PlatformError::Device(e.to_string()))?;
        stream.play().map_err(|e| PlatformError::Device(e.to_string()))?;
        Ok(Self { _stream: stream, device_name, sample_rate: rate, channels })
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

fn fill(transport: &Mutex<Transport>, converter: &mut Converter, out: &mut [f32]) {
    let mut guard = match transport.try_lock() {
        Ok(g) => g,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        Err(TryLockError::WouldBlock) => {
            out.fill(0.0);
            return;
        }
    };
    converter.fill(out, |frame| {
        guard.render(frame);
    });
}

fn log_error(e: &cpal::StreamError) {
    // stderr only: the error describes the device, never the dictation.
    eprintln!("audio output error: {e}");
}
