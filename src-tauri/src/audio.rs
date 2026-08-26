use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use serde::Serialize;
use std::sync::{Arc, Mutex, MutexGuard};

const TARGET_SAMPLE_RATE: u32 = 16_000;
pub const MAX_RECORDING_SECONDS: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InputDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub struct CapturedAudio {
    pub samples: Vec<f32>,
    pub duration_seconds: f32,
    pub rms: f32,
}

pub struct AudioRecorder {
    stream: Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    stream_error: Arc<Mutex<Option<String>>>,
    channels: usize,
    sample_rate: u32,
    microphone: InputDeviceInfo,
    used_fallback: bool,
}

impl AudioRecorder {
    pub fn start(preferred_device_id: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let default_device = host.default_input_device();
        let default_id = default_device
            .as_ref()
            .and_then(|device| device.id().ok())
            .map(|id| id.to_string());

        let selected_device = if let Some(preferred_id) = preferred_device_id {
            host.input_devices()
                .map_err(|error| format!("Could not list microphones: {error}"))?
                .find(|device| device.id().is_ok_and(|id| id.to_string() == preferred_id))
        } else {
            None
        };
        let used_fallback = preferred_device_id.is_some() && selected_device.is_none();
        let device = selected_device
            .or(default_device)
            .ok_or_else(|| "No microphone is available.".to_string())?;
        let device_id = device
            .id()
            .map_err(|error| format!("Could not identify the microphone: {error}"))?
            .to_string();
        let device_name = device
            .description()
            .map(|description| description.name().to_string())
            .unwrap_or_else(|_| device.to_string());
        let microphone = InputDeviceInfo {
            is_default: default_id.as_deref() == Some(device_id.as_str()),
            id: device_id,
            name: device_name,
        };
        let supported_config = device
            .default_input_config()
            .map_err(|error| format!("Could not read the microphone configuration: {error}"))?;

        let channels = supported_config.channels() as usize;
        let sample_rate = supported_config.sample_rate();
        let sample_format = supported_config.sample_format();
        let config: StreamConfig = supported_config.into();
        let samples = Arc::new(Mutex::new(Vec::with_capacity(
            sample_rate as usize * channels * 15,
        )));
        let stream_error = Arc::new(Mutex::new(None));
        let max_samples = sample_rate as usize * channels * MAX_RECORDING_SECONDS;

        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(
                &device,
                &config,
                samples.clone(),
                stream_error.clone(),
                max_samples,
            ),
            SampleFormat::I16 => build_stream::<i16>(
                &device,
                &config,
                samples.clone(),
                stream_error.clone(),
                max_samples,
            ),
            SampleFormat::I32 => build_stream::<i32>(
                &device,
                &config,
                samples.clone(),
                stream_error.clone(),
                max_samples,
            ),
            format => Err(format!("Unsupported microphone sample format: {format}")),
        }?;

        stream
            .play()
            .map_err(|error| format!("Could not start the microphone: {error}"))?;

        Ok(Self {
            stream,
            samples,
            stream_error,
            channels,
            sample_rate,
            microphone,
            used_fallback,
        })
    }

    pub fn microphone(&self) -> &InputDeviceInfo {
        &self.microphone
    }

    pub fn used_fallback(&self) -> bool {
        self.used_fallback
    }

    pub fn finish(self) -> Result<CapturedAudio, String> {
        drop(self.stream);

        if let Some(error) = lock(&self.stream_error).take() {
            return Err(error);
        }

        let interleaved = std::mem::take(&mut *lock(&self.samples));
        if interleaved.is_empty() {
            return Err("The microphone did not produce any audio.".to_string());
        }

        let mono = mix_to_mono(&interleaved, self.channels);
        let duration_seconds = mono.len() as f32 / self.sample_rate as f32;
        let samples = resample(&mono, self.sample_rate, TARGET_SAMPLE_RATE);
        let rms = root_mean_square(&samples);

        Ok(CapturedAudio {
            samples,
            duration_seconds,
            rms,
        })
    }
}

pub fn input_devices() -> Result<Vec<InputDeviceInfo>, String> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let mut devices = host
        .input_devices()
        .map_err(|error| format!("Could not list microphones: {error}"))?
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            let name = device
                .description()
                .map(|description| description.name().to_string())
                .unwrap_or_else(|_| device.to_string());
            Some(InputDeviceInfo {
                is_default: default_id.as_deref() == Some(id.as_str()),
                id,
                name,
            })
        })
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(devices)
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
    stream_error: Arc<Mutex<Option<String>>>,
    max_samples: usize,
) -> Result<Stream, String>
where
    T: Sample + SizedSample + Copy,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            *config,
            move |input: &[T], _| {
                let Ok(mut output) = samples.try_lock() else {
                    return;
                };
                let remaining = max_samples.saturating_sub(output.len());
                output.extend(input.iter().take(remaining).copied().map(f32::from_sample));
            },
            move |error| {
                *lock(&stream_error) = Some(format!("Microphone error: {error}"));
            },
            None,
        )
        .map_err(|error| format!("Could not open the microphone: {error}"))
}

fn mix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }

    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn resample(input: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if input.is_empty() || source_rate == target_rate {
        return input.to_vec();
    }

    let output_len = input.len() * target_rate as usize / source_rate as usize;
    if output_len == 0 {
        return Vec::new();
    }

    // Microphones generally provide 44.1 or 48 kHz audio. Averaging each
    // source interval provides a small low-pass filter while downsampling to
    // Whisper's required 16 kHz input.
    if source_rate > target_rate {
        return (0..output_len)
            .map(|index| {
                let start = index * source_rate as usize / target_rate as usize;
                let mut end = (index + 1) * source_rate as usize / target_rate as usize;
                end = end.max(start + 1).min(input.len());
                input[start..end].iter().sum::<f32>() / (end - start) as f32
            })
            .collect();
    }

    let ratio = source_rate as f64 / target_rate as f64;
    (0..output_len)
        .map(|index| {
            let position = index as f64 * ratio;
            let left = position.floor() as usize;
            let right = (left + 1).min(input.len() - 1);
            let fraction = (position - left as f64) as f32;
            input[left] + (input[right] - input[left]) * fraction
        })
        .collect()
}

fn root_mean_square(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mean_square =
        samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32;
    mean_square.sqrt()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_stereo_to_mono() {
        assert_eq!(mix_to_mono(&[1.0, -1.0, 0.5, 0.5], 2), vec![0.0, 0.5]);
    }

    #[test]
    fn downsamples_to_expected_length() {
        let input = vec![0.25; 48_000];
        let output = resample(&input, 48_000, 16_000);
        assert_eq!(output.len(), 16_000);
        assert!(output.iter().all(|sample| (*sample - 0.25).abs() < 0.0001));
    }
}
