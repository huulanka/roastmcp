//! Reading recorded sound: WAV and FLAC, mixed down to mono.

use std::path::Path;

/// A failure reading a sound file.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("cannot read the WAV file: {0}")]
    Wav(#[from] hound::Error),
    #[error("cannot read the FLAC file: {0}")]
    Flac(#[from] claxon::Error),
    #[error("unknown sound format (use .wav or .flac): {0}")]
    Format(String),
    #[error("cannot write the FLAC file: {0}")]
    Encode(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Mono samples (full scale ±1) and their rate.
#[derive(Debug, Clone)]
pub struct Sound {
    pub rate: u32,
    pub samples: Vec<f32>,
}

impl Sound {
    /// Length in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.rate.max(1))
    }
}

/// Reads a `.wav` or `.flac` file.
///
/// # Errors
/// When the file cannot be read or has another format.
pub fn read(path: &Path) -> Result<Sound, FileError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "wav" => read_wav(path),
        "flac" => read_flac(path),
        _ => Err(FileError::Format(path.display().to_string())),
    }
}

fn mono(interleaved: impl Iterator<Item = f32>, channels: usize) -> Vec<f32> {
    let channels = channels.max(1);
    let mut out = Vec::new();
    let mut acc = 0.0;
    let mut n = 0;
    for s in interleaved {
        acc += s;
        n += 1;
        if n == channels {
            out.push(acc / channels as f32);
            acc = 0.0;
            n = 0;
        }
    }
    out
}

fn read_wav(path: &Path) -> Result<Sound, FileError> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    let channels = usize::from(spec.channels);
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => {
            let v: Result<Vec<f32>, _> = r.samples::<f32>().collect();
            mono(v?.into_iter(), channels)
        }
        hound::SampleFormat::Int => {
            let full = (1i64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            let v: Result<Vec<i32>, _> = r.samples::<i32>().collect();
            mono(v?.into_iter().map(|s| s as f32 / full), channels)
        }
    };
    Ok(Sound {
        rate: spec.sample_rate,
        samples,
    })
}

fn read_flac(path: &Path) -> Result<Sound, FileError> {
    let mut r = claxon::FlacReader::open(path)?;
    let info = r.streaminfo();
    let channels = info.channels as usize;
    let full = (1i64 << (info.bits_per_sample.max(1) - 1)) as f32;
    let v: Result<Vec<i32>, _> = r.samples().collect();
    Ok(Sound {
        rate: info.sample_rate,
        samples: mono(v?.into_iter().map(|s| s as f32 / full), channels),
    })
}

/// Writes mono samples as a 16-bit WAV file.
///
/// # Errors
/// When the file cannot be written.
pub fn write_wav(path: &Path, rate: u32, samples: &[f32]) -> Result<(), FileError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)?;
    }
    w.finalize()?;
    Ok(())
}

/// Writes 16-bit mono samples as a FLAC file (about a third of the WAV).
///
/// # Errors
/// When encoding or writing fails.
pub fn write_flac(path: &Path, rate: u32, samples: &[i16]) -> Result<(), FileError> {
    use flacenc::component::BitRepr as _;
    use flacenc::error::Verify as _;
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|(_, e)| FileError::Encode(format!("{e:?}")))?;
    let wide: Vec<i32> = samples.iter().map(|&s| i32::from(s)).collect();
    let source = flacenc::source::MemSource::from_samples(&wide, 1, 16, rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| FileError::Encode(format!("{e:?}")))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| FileError::Encode(format!("{e:?}")))?;
    std::fs::write(path, sink.as_slice())?;
    Ok(())
}

/// Turns a 16-bit mono WAV into a FLAC beside it and removes the WAV.
/// Returns the FLAC's path.
///
/// # Errors
/// When the WAV cannot be read or the FLAC cannot be written (the WAV stays).
pub fn wav_to_flac(wav: &Path) -> Result<std::path::PathBuf, FileError> {
    let mut r = hound::WavReader::open(wav)?;
    let rate = r.spec().sample_rate;
    let samples: Result<Vec<i16>, _> = r.samples::<i16>().collect();
    let flac = wav.with_extension("flac");
    write_flac(&flac, rate, &samples?)?;
    std::fs::remove_file(wav)?;
    Ok(flac)
}

/// Writes sound as it comes to a WAV file and turns it into FLAC at the end
/// (ADR 0021 §9: only when the person switched recording on).
pub struct Recorder {
    writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    path: std::path::PathBuf,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("path", &self.path)
            .finish()
    }
}

impl Recorder {
    /// Starts `<flac path>.wav`; the FLAC replaces it in [`Recorder::finish`].
    ///
    /// # Errors
    /// When the file cannot be created.
    pub fn create(flac: &Path, rate: u32) -> Result<Self, FileError> {
        if let Some(dir) = flac.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let path = flac.with_extension("wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        Ok(Recorder {
            writer: hound::WavWriter::create(&path, spec)?,
            path,
        })
    }

    /// Appends mono samples (full scale ±1).
    ///
    /// # Errors
    /// When writing fails.
    pub fn write(&mut self, samples: &[f32]) -> Result<(), FileError> {
        for &s in samples {
            self.writer
                .write_sample((s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)?;
        }
        Ok(())
    }

    /// Closes the WAV and turns it into FLAC; returns the file kept.
    ///
    /// # Errors
    /// When closing fails; a failed FLAC leaves the WAV and says so.
    pub fn finish(self) -> Result<std::path::PathBuf, FileError> {
        self.writer.finalize()?;
        wav_to_flac(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_file_reads_back() {
        let dir = std::env::temp_dir().join(format!("roast-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        let path = dir.join("t.wav");
        let s: Vec<f32> = (0..4800).map(|i| (i as f32 / 100.0).sin() * 0.5).collect();
        write_wav(&path, 48_000, &s).unwrap_or_else(|e| panic!("{e}"));
        let back = read(&path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back.rate, 48_000);
        assert_eq!(back.samples.len(), 4800);
        assert!((back.samples[100] - s[100]).abs() < 1e-3);
        assert!(read(&dir.join("t.mp3")).is_err());

        let flac = wav_to_flac(&path).unwrap_or_else(|e| panic!("{e}"));
        assert!(!path.exists(), "the WAV is gone");
        let back = read(&flac).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back.rate, 48_000);
        assert_eq!(back.samples.len(), 4800);
        assert!((back.samples[100] - s[100]).abs() < 1e-3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
