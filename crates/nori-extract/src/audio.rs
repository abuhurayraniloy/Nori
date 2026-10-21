use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

/// 16 kHz sample rate expected by the EmbeddingGemma audio encoder.
pub const SAMPLE_RATE: u32 = 16_000;

/// 30.0-second sliding window duration (480,000 samples at 16 kHz).
pub const WINDOW_SECS: f64 = 30.0;

/// 25.0-second hop size (400,000 samples at 16 kHz), producing a 5.0-second overlap.
pub const HOP_SECS: f64 = 25.0;

/// Minimum leftover tail duration required to output a piece (2.0s = 32,000 samples).
pub const MIN_TAIL_SECS: f64 = 2.0;

/// Root-Mean-Square amplitude threshold for silence filtering (~ -50.46 dBFS).
pub const RMS_SILENCE_THRESHOLD: f64 = 0.003;

/// A temporal slice of media (audio or video keyframe) written to disk for embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaPiece {
    pub path: PathBuf,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Error)]
pub enum AudioExtractError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid audio format: {0}")]
    InvalidFormat(String),
    #[error("Decoder error: {0}")]
    Decode(String),
}

/// Universal audio extractor, silence filter, and WAV slice serializer.
pub struct AudioExtractor;

impl AudioExtractor {
    /// Computes normalized Root-Mean-Square (RMS) amplitude in range [0.0, 1.0].
    pub fn calculate_rms(samples: &[i16]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let mut sum_sq = 0.0f64;
        for &sample in samples {
            let norm = sample as f64 / 32768.0;
            sum_sq += norm * norm;
        }
        (sum_sq / samples.len() as f64).sqrt()
    }

    /// Serializes a canonical 44-byte Linear PCM 16-bit mono WAV file.
    pub fn write_canonical_wav(
        samples: &[i16],
        rate: u32,
        out_path: &Path,
    ) -> Result<(), std::io::Error> {
        let mut file = File::create(out_path)?;
        let byte_rate = rate * 2; // 1 channel * 2 bytes/sample
        let data_size = (samples.len() * 2) as u32;
        let chunk_size = 36 + data_size;

        let mut header = Vec::with_capacity(44);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&chunk_size.to_le_bytes());
        header.extend_from_slice(b"WAVE");
        header.extend_from_slice(b"fmt ");
        header.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size (16 for PCM)
        header.extend_from_slice(&1u16.to_le_bytes());  // AudioFormat (1 = PCM)
        header.extend_from_slice(&1u16.to_le_bytes());  // NumChannels (1 = Mono)
        header.extend_from_slice(&rate.to_le_bytes());   // SampleRate (16,000)
        header.extend_from_slice(&byte_rate.to_le_bytes()); // ByteRate
        header.extend_from_slice(&2u16.to_le_bytes());  // BlockAlign
        header.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample
        header.extend_from_slice(b"data");
        header.extend_from_slice(&data_size.to_le_bytes());

        file.write_all(&header)?;
        for &sample in samples {
            file.write_all(&sample.to_le_bytes())?;
        }
        file.flush()?;
        Ok(())
    }

    /// Linear interpolation resampler from `src_rate` to `target_rate`.
    pub fn resample_linear(samples: &[i16], src_rate: u32, target_rate: u32) -> Vec<i16> {
        if src_rate == target_rate || samples.is_empty() {
            return samples.to_vec();
        }

        let ratio = src_rate as f64 / target_rate as f64;
        let target_len = ((samples.len() as f64) / ratio).round() as usize;
        let mut output = Vec::with_capacity(target_len);

        for j in 0..target_len {
            let t = j as f64 * ratio;
            let i = t.floor() as usize;
            let alpha = t - i as f64;

            if i >= samples.len() {
                break;
            }

            let s0 = samples[i] as f64;
            let s1 = if i + 1 < samples.len() {
                samples[i + 1] as f64
            } else {
                s0
            };

            let interpolated = (1.0 - alpha) * s0 + alpha * s1;
            let clamped = interpolated.round().clamp(-32768.0, 32767.0) as i16;
            output.push(clamped);
        }

        output
    }

    /// Slices raw audio samples into overlapping windows, discarding silence.
    pub fn process_samples(
        samples: &[i16],
        sample_rate: u32,
        window_secs: f64,
        hop_secs: f64,
        min_tail_secs: f64,
        output_dir: &Path,
    ) -> Result<(Vec<MediaPiece>, f64), std::io::Error> {
        let window_samples = (window_secs * sample_rate as f64) as usize;
        let hop_samples = (hop_secs * sample_rate as f64) as usize;
        let min_tail_samples = (min_tail_secs * sample_rate as f64) as usize;

        let mut pieces = Vec::new();
        let mut offset = 0;
        let mut i = 0;

        let mut emit = |slice: &[i16], sample_offset: usize| -> Result<(), std::io::Error> {
            if Self::calculate_rms(slice) > RMS_SILENCE_THRESHOLD {
                let out_file = output_dir.join(format!("{}.wav", Uuid::new_v4()));
                Self::write_canonical_wav(slice, sample_rate, &out_file)?;
                pieces.push(MediaPiece {
                    path: out_file,
                    start: sample_offset as f64 / sample_rate as f64,
                    end: (sample_offset + slice.len()) as f64 / sample_rate as f64,
                });
            }
            Ok(())
        };

        while i + window_samples <= samples.len() {
            emit(&samples[i..i + window_samples], offset)?;
            i += hop_samples;
            offset += hop_samples;
        }

        // Check tail beyond the already-covered overlap
        let already_covered = if offset == 0 {
            0
        } else {
            window_samples - hop_samples
        };
        let remaining = samples.len().saturating_sub(offset);
        if remaining > already_covered && (remaining - already_covered) >= min_tail_samples {
            emit(&samples[offset..], offset)?;
        }

        let duration = samples.len() as f64 / sample_rate as f64;
        Ok((pieces, duration))
    }

    /// Reads a 16-bit PCM WAV file directly, downmixing to mono if needed.
    pub fn read_wav(path: &Path) -> Result<(Vec<i16>, u32), AudioExtractError> {
        let mut file = File::open(path)?;
        let mut header = [0u8; 12];
        file.read_exact(&mut header)?;

        if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
            return Err(AudioExtractError::InvalidFormat("not a RIFF/WAVE file".to_string()));
        }

        let mut channels = 1u16;
        let mut sample_rate = 16_000u32;
        let mut bits_per_sample = 16u16;
        let mut data_bytes = Vec::new();

        loop {
            let mut chunk_header = [0u8; 8];
            if file.read_exact(&mut chunk_header).is_err() {
                break;
            }
            let chunk_id = &chunk_header[0..4];
            let chunk_size = u32::from_le_bytes(chunk_header[4..8].try_into().unwrap()) as usize;

            if chunk_id == b"fmt " {
                let mut fmt_buf = vec![0u8; chunk_size];
                file.read_exact(&mut fmt_buf)?;
                if fmt_buf.len() < 16 {
                    return Err(AudioExtractError::InvalidFormat("invalid fmt chunk".to_string()));
                }
                let format_tag = u16::from_le_bytes(fmt_buf[0..2].try_into().unwrap());
                if format_tag != 1 {
                    return Err(AudioExtractError::InvalidFormat("non-PCM WAV format".to_string()));
                }
                channels = u16::from_le_bytes(fmt_buf[2..4].try_into().unwrap());
                sample_rate = u32::from_le_bytes(fmt_buf[4..8].try_into().unwrap());
                bits_per_sample = u16::from_le_bytes(fmt_buf[14..16].try_into().unwrap());
            } else if chunk_id == b"data" {
                data_bytes = vec![0u8; chunk_size];
                file.read_exact(&mut data_bytes)?;
                break;
            } else {
                // Skip unneeded chunks (e.g. LIST, JUNK)
                let mut skip_buf = vec![0u8; chunk_size];
                file.read_exact(&mut skip_buf)?;
            }
        }

        if bits_per_sample != 16 {
            return Err(AudioExtractError::InvalidFormat(format!(
                "unsupported bit depth {bits_per_sample}, expected 16"
            )));
        }

        let total_samples = data_bytes.len() / 2;
        let num_frames = total_samples / channels as usize;
        let mut mono_samples = Vec::with_capacity(num_frames);

        for f in 0..num_frames {
            let mut sum = 0i32;
            for ch in 0..channels as usize {
                let idx = (f * channels as usize + ch) * 2;
                if idx + 1 < data_bytes.len() {
                    let s = i16::from_le_bytes([data_bytes[idx], data_bytes[idx + 1]]);
                    sum += s as i32;
                }
            }
            let mono = (sum / channels as i32) as i16;
            mono_samples.push(mono);
        }

        Ok((mono_samples, sample_rate))
    }

    /// End-to-end extraction from a WAV file: decodes, resamples to 16 kHz mono, and slices into windows.
    pub fn windows_from_wav(
        input_wav: &Path,
        output_dir: &Path,
    ) -> Result<(Vec<MediaPiece>, f64), AudioExtractError> {
        let (samples, rate) = Self::read_wav(input_wav)?;
        let resampled = Self::resample_linear(&samples, rate, SAMPLE_RATE);
        let result = Self::process_samples(
            &resampled,
            SAMPLE_RATE,
            WINDOW_SECS,
            HOP_SECS,
            MIN_TAIL_SECS,
            output_dir,
        )?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_silence_and_signals() {
        let silence = vec![0i16; 16000];
        assert_eq!(AudioExtractor::calculate_rms(&silence), 0.0);

        // Low noise below -50 dBFS threshold (~ amplitude 50)
        let low_noise = vec![50i16; 16000];
        let low_rms = AudioExtractor::calculate_rms(&low_noise);
        assert!(low_rms < RMS_SILENCE_THRESHOLD);

        // Audible signal (~ amplitude 1000)
        let signal = vec![1000i16; 16000];
        let signal_rms = AudioExtractor::calculate_rms(&signal);
        assert!(signal_rms > RMS_SILENCE_THRESHOLD);

        // Max amplitude square wave
        let max_wave = vec![32767i16; 16000];
        let max_rms = AudioExtractor::calculate_rms(&max_wave);
        assert!((max_rms - 1.0).abs() < 1e-3);
    }

    #[test]
    fn test_canonical_wav_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("nori_audio_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let wav_path = temp_dir.join("test_tone.wav");

        let original_samples: Vec<i16> = (0..16000)
            .map(|i| ((i as f64 * 0.1).sin() * 10000.0) as i16)
            .collect();

        AudioExtractor::write_canonical_wav(&original_samples, 16000, &wav_path).unwrap();

        let (loaded, rate) = AudioExtractor::read_wav(&wav_path).unwrap();
        assert_eq!(rate, 16000);
        assert_eq!(loaded, original_samples);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_sliding_window_counts_and_overlaps() {
        let temp_dir = std::env::temp_dir().join(format!("nori_window_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 60 seconds of audible audio at 16 kHz: 960,000 samples
        let samples: Vec<i16> = (0..960_000).map(|_| 5000i16).collect();

        let (pieces, duration) = AudioExtractor::process_samples(
            &samples,
            16000,
            30.0,
            25.0,
            2.0,
            &temp_dir,
        )
        .unwrap();

        assert_eq!(duration, 60.0);
        // Window 0: 0.0s..30.0s
        // Window 1: 25.0s..55.0s
        // Tail: 50.0s..60.0s (10.0s >= 2.0s min tail)
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].start, 0.0);
        assert_eq!(pieces[0].end, 30.0);
        assert_eq!(pieces[1].start, 25.0);
        assert_eq!(pieces[1].end, 55.0);
        assert_eq!(pieces[2].start, 50.0);
        assert_eq!(pieces[2].end, 60.0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_short_audio_below_window() {
        let temp_dir = std::env::temp_dir().join(format!("nori_short_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 10 seconds of audible audio (160,000 samples)
        let samples: Vec<i16> = (0..160_000).map(|_| 5000i16).collect();

        let (pieces, duration) = AudioExtractor::process_samples(
            &samples,
            16000,
            30.0,
            25.0,
            2.0,
            &temp_dir,
        )
        .unwrap();

        assert_eq!(duration, 10.0);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].start, 0.0);
        assert_eq!(pieces[0].end, 10.0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_silent_audio_emits_no_pieces() {
        let temp_dir = std::env::temp_dir().join(format!("nori_silent_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 60 seconds of complete silence
        let silence: Vec<i16> = vec![0i16; 960_000];

        let (pieces, duration) = AudioExtractor::process_samples(
            &silence,
            16000,
            30.0,
            25.0,
            2.0,
            &temp_dir,
        )
        .unwrap();

        assert_eq!(duration, 60.0);
        assert!(pieces.is_empty()); // Zero pieces emitted for dead air!

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
