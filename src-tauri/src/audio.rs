use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use std::sync::{Arc, Mutex};

const TARGET_SAMPLE_RATE: u32 = 16_000;
const MAX_RECORDING_SECONDS: usize = 120;

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
}

impl AudioRecorder {
    pub fn start() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No microphone is available.".to_string())?;
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
        })
    }

    pub fn finish(self) -> Result<CapturedAudio, String> {
        drop(self.stream);

        if let Some(error) = self.stream_error.lock().unwrap().take() {
            return Err(error);
        }

        let interleaved = std::mem::take(&mut *self.samples.lock().unwrap());
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
                *stream_error.lock().unwrap() = Some(format!("Microphone error: {error}"));
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
