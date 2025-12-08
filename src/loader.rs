use crate::sample::{Instrument, InstrumentDef, Region, RegionDef};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Supported audio formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Flac,
    Unknown,
}

impl AudioFormat {
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("wav") | Some("WAV") => AudioFormat::Wav,
            Some("flac") | Some("FLAC") => AudioFormat::Flac,
            _ => AudioFormat::Unknown,
        }
    }
}

/// Loaded audio data
pub struct AudioData {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
    pub num_frames: usize,
}

/// Load an audio file (WAV or FLAC) using Symphonia
pub fn load_audio(path: &Path) -> Result<AudioData, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;

    let mss = MediaSourceStream::new(
        Box::new(ReadOnlySource::new(BufReader::new(file))),
        Default::default(),
    );

    // Provide format hint based on extension
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();
    let decoder_opts = DecoderOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| format!("Failed to probe {}: {}", path.display(), e))?;

    let mut format = probed.format;

    // Get the default track
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| format!("No audio track in {}", path.display()))?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let channels = codec_params.channels.map(|c| c.count()).unwrap_or(1);

    let sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| "Unknown sample rate".to_string())?;

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &decoder_opts)
        .map_err(|e| format!("Failed to create decoder: {}", e))?;

    let mut samples: Vec<f32> = Vec::new();

    // Decode all packets
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => return Err(format!("Decode error: {}", e)),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("Decode error: {}", e)),
        };

        append_samples(&decoded, &mut samples, channels);
    }

    let num_frames = samples.len() / channels;

    Ok(AudioData {
        samples,
        channels,
        sample_rate,
        num_frames,
    })
}

fn append_samples(buffer: &AudioBufferRef, out: &mut Vec<f32>, channels: usize) {
    match buffer {
        AudioBufferRef::F32(buf) => {
            for frame in 0..buf.frames() {
                for ch in 0..channels.min(buf.spec().channels.count()) {
                    out.push(buf.chan(ch)[frame]);
                }
            }
        }
        AudioBufferRef::S16(buf) => {
            const SCALE: f32 = 1.0 / 32768.0;
            for frame in 0..buf.frames() {
                for ch in 0..channels.min(buf.spec().channels.count()) {
                    out.push(buf.chan(ch)[frame] as f32 * SCALE);
                }
            }
        }
        AudioBufferRef::S24(buf) => {
            const SCALE: f32 = 1.0 / 8388608.0;
            for frame in 0..buf.frames() {
                for ch in 0..channels.min(buf.spec().channels.count()) {
                    out.push(buf.chan(ch)[frame].0 as f32 * SCALE);
                }
            }
        }
        AudioBufferRef::S32(buf) => {
            const SCALE: f32 = 1.0 / 2147483648.0;
            for frame in 0..buf.frames() {
                for ch in 0..channels.min(buf.spec().channels.count()) {
                    out.push(buf.chan(ch)[frame] as f32 * SCALE);
                }
            }
        }
        AudioBufferRef::U8(buf) => {
            const SCALE: f32 = 1.0 / 128.0;
            for frame in 0..buf.frames() {
                for ch in 0..channels.min(buf.spec().channels.count()) {
                    out.push((buf.chan(ch)[frame] as f32 - 128.0) * SCALE);
                }
            }
        }
        _ => {
            // Other formats - skip silently
        }
    }
}

/// Load an instrument from a JSON definition file
pub fn load_instrument_json(def_path: &Path) -> Result<Instrument, String> {
    let json_str = std::fs::read_to_string(def_path)
        .map_err(|e| format!("Failed to read {}: {}", def_path.display(), e))?;

    let def: InstrumentDef =
        serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    let base_dir = def_path.parent().unwrap_or(Path::new("."));
    let mut regions = Vec::with_capacity(def.regions.len());

    for region_def in &def.regions {
        let sample_path = base_dir.join(&region_def.sample);
        match load_region(&sample_path, region_def) {
            Ok(region) => regions.push(region),
            Err(e) => {
                eprintln!("Warning: {}", e);
            }
        }
    }

    Ok(Instrument::new(def.name, regions))
}

fn load_region(sample_path: &Path, def: &RegionDef) -> Result<Region, String> {
    let audio = load_audio(sample_path)?;

    Ok(Region {
        data: Arc::new(audio.samples),
        channels: audio.channels,
        sample_rate: audio.sample_rate as f32,
        num_frames: audio.num_frames,

        root_note: def.root,
        lo_note: def.lo_note.unwrap_or(0),
        hi_note: def.hi_note.unwrap_or(127),
        lo_vel: def.lo_vel.unwrap_or(0),
        hi_vel: def.hi_vel.unwrap_or(127),

        loop_start: def.loop_start,
        loop_end: def.loop_end,
        loop_enabled: def.loop_enabled,

        rr_group: def.rr_group,
        rr_seq: def.rr_seq,

        tune_cents: def.tune_cents,
        volume_db: def.volume_db,
        pan: def.pan,

        sample_path: sample_path.to_string_lossy().to_string(),
    })
}

/// Scan a directory for instrument files (.json or .sfz)
pub fn scan_instruments(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext == "json" || ext == "sfz" {
                    found.push(path);
                }
            }
        }
    }

    found.sort();
    found
}

/// Create a test instrument with a sine wave
pub fn create_test_instrument(sample_rate: f32) -> Instrument {
    use core::f32::consts::PI;

    let duration_secs = 1.0;
    let num_frames = (sample_rate * duration_secs) as usize;
    let freq = 440.0;

    let mut data = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let t = i as f32 / sample_rate;
        let sample = (2.0 * PI * freq * t).sin() * 0.8;
        data.push(sample);
    }

    let region = Region {
        data: Arc::new(data),
        channels: 1,
        sample_rate,
        num_frames,

        root_note: 69,
        lo_note: 0,
        hi_note: 127,
        lo_vel: 0,
        hi_vel: 127,

        loop_start: Some((sample_rate * 0.1) as usize),
        loop_end: Some((sample_rate * 0.9) as usize),
        loop_enabled: true,

        rr_group: 0,
        rr_seq: 0,

        tune_cents: 0.0,
        volume_db: 0.0,
        pan: 0.0,

        sample_path: String::from("<generated>"),
    };

    Instrument::new(String::from("Test Sine"), vec![region])
}
