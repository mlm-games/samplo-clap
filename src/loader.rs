use crate::sample::{Instrument, InstrumentDef, Region, RegionDef};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::sync::OnceLock;
use symphonia::core::audio::{Audio, GenericAudioBufferRef};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;

fn get_codecs() -> &'static CodecRegistry {
    static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_audio_decoder::<symphonia_adapter_mousiki::OpusDecoder>();
        registry
    })
}

/// Loaded audio data
pub struct AudioData {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
    pub num_frames: usize,
}

/// Load an audio file using Symphonia
pub fn load_audio(path: &Path) -> Result<AudioData, String> {
    let file = File::open(path).map_err(|e| format!("Cannot open '{}': {}", path.display(), e))?;

    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);

    let mss = MediaSourceStream::new(
        Box::new(ReadOnlySource::new(BufReader::new(file))),
        Default::default(),
    );

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();
    let decoder_opts = AudioDecoderOptions::default();

    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, format_opts, metadata_opts)
        .map_err(|e| {
            format!(
                "Cannot identify format of '{}' ({} bytes): {}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                file_size,
                e
            )
        })?;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.as_ref().is_some_and(|p| p.is_audio()))
        .ok_or_else(|| format!("No audio track in '{}'", path.display()))?;

    let track_id = track.id;

    let codec_params = track
        .codec_params
        .clone()
        .ok_or_else(|| format!("No codec parameters in '{}'", path.display()))?;

    let audio_params = codec_params.audio().unwrap();
    let channels = audio_params
        .channels
        .as_ref()
        .map(|c| c.count())
        .unwrap_or(1);
    let sample_rate = audio_params
        .sample_rate
        .ok_or_else(|| format!("Unknown sample rate in '{}'", path.display()))?;

    let mut decoder = get_codecs()
        .make_audio_decoder(&audio_params, &decoder_opts)
        .map_err(|e| {
            format!(
                "No decoder for '{}' (codec {:?}): {}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                audio_params.codec,
                e
            )
        })?;

    let mut samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(symphonia::core::errors::Error::ResetRequired) => {
                continue;
            }
            Err(_) => break,
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(e) => {
                nih_plug::nih_log!("Decode error in '{}': {:?}", path.display(), e);
                continue;
            }
        };

        append_samples(&decoded, &mut samples, channels);
    }

    if samples.is_empty() {
        return Err(format!(
            "No audio data decoded from '{}' ({} bytes)",
            path.file_name().unwrap_or_default().to_string_lossy(),
            file_size
        ));
    }

    let num_frames = samples.len() / channels;

    Ok(AudioData {
        samples,
        channels,
        sample_rate,
        num_frames,
    })
}

fn append_samples(buffer: &GenericAudioBufferRef, out: &mut Vec<f32>, channels: usize) {
    match *buffer {
        GenericAudioBufferRef::F32(ref buf) => {
            let n_ch = channels.min(buf.spec().channels().count());
            let frames = buf.frames();
            out.reserve(frames * n_ch);
            let planes: Vec<&[f32]> = (0..n_ch).map(|ch| buf.plane(ch).unwrap()).collect();
            for frame in 0..frames {
                for ch in 0..n_ch {
                    out.push(planes[ch][frame]);
                }
            }
        }
        GenericAudioBufferRef::S16(ref buf) => {
            const SCALE: f32 = 1.0 / 32768.0;
            let n_ch = channels.min(buf.spec().channels().count());
            let frames = buf.frames();
            out.reserve(frames * n_ch);
            let planes: Vec<&[i16]> = (0..n_ch).map(|ch| buf.plane(ch).unwrap()).collect();
            for frame in 0..frames {
                for ch in 0..n_ch {
                    out.push(planes[ch][frame] as f32 * SCALE);
                }
            }
        }
        GenericAudioBufferRef::S24(ref buf) => {
            const SCALE: f32 = 1.0 / 8388608.0;
            let n_ch = channels.min(buf.spec().channels().count());
            let frames = buf.frames();
            out.reserve(frames * n_ch);
            for frame in 0..frames {
                for ch in 0..n_ch {
                    out.push(buf.plane(ch).unwrap()[frame].0 as f32 * SCALE);
                }
            }
        }
        GenericAudioBufferRef::S32(ref buf) => {
            const SCALE: f32 = 1.0 / 2147483648.0;
            let n_ch = channels.min(buf.spec().channels().count());
            let frames = buf.frames();
            out.reserve(frames * n_ch);
            let planes: Vec<&[i32]> = (0..n_ch).map(|ch| buf.plane(ch).unwrap()).collect();
            for frame in 0..frames {
                for ch in 0..n_ch {
                    out.push(planes[ch][frame] as f32 * SCALE);
                }
            }
        }
        GenericAudioBufferRef::U8(ref buf) => {
            const SCALE: f32 = 1.0 / 128.0;
            let n_ch = channels.min(buf.spec().channels().count());
            let frames = buf.frames();
            out.reserve(frames * n_ch);
            let planes: Vec<&[u8]> = (0..n_ch).map(|ch| buf.plane(ch).unwrap()).collect();
            for frame in 0..frames {
                for ch in 0..n_ch {
                    out.push((planes[ch][frame] as f32 - 128.0) * SCALE);
                }
            }
        }
        _ => {}
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
                nih_plug::nih_log!("Warning: {}", e);
            }
        }
    }

    Ok(Instrument::new(def.name, regions))
}

fn load_region(sample_path: &Path, def: &RegionDef) -> Result<Region, String> {
    let audio = load_audio(sample_path)?;

    use crate::sample::LoopMode;

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
        loop_mode: if def.loop_enabled {
            LoopMode::Continuous
        } else {
            LoopMode::NoLoop
        },

        rr_group: def.rr_group,
        rr_seq: def.rr_seq,

        tune_cents: def.tune_cents,
        volume_db: def.volume_db,
        volume_lin: crate::dsp::db_to_linear(def.volume_db),
        pan: def.pan,

        #[cfg(debug_assertions)]
        sample_path: sample_path.to_string_lossy().to_string(),
    })
}

/// Scan a directory for instrument files (.json or .sfz)
/// Setting the max depth to two to cover git repos (like https://github.com/sfzinstruments 's instruments)
pub fn scan_instruments(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    scan_recursive(dir, 0, max_depth, &mut found);
    found.sort();
    found
}

fn scan_recursive(dir: &Path, current_depth: usize, max_depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        let Ok(ft) = entry.file_type() else {
            continue;
        };

        // Skips hidden directories
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }

        if ft.is_file() && is_instrument_file(&path) {
            found.push(path);
        } else if ft.is_dir() && current_depth < max_depth {
            // Skip common non-instrument directories
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !matches!(name.to_lowercase().as_str(), "samples" | "waves" | "audio") {
                scan_recursive(&path, current_depth + 1, max_depth, found);
            }
        }
    }
}

#[inline]
fn is_instrument_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => {
            let ext_lower = ext.to_lowercase();
            ext_lower == "sfz" || ext_lower == "json"
        }
        None => false,
    }
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

    use crate::sample::LoopMode;

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
        loop_mode: LoopMode::Continuous,

        rr_group: 0,
        rr_seq: 0,

        tune_cents: 0.0,
        volume_db: 0.0,
        volume_lin: crate::dsp::db_to_linear(0.0),
        pan: 0.0,

        #[cfg(debug_assertions)]
        sample_path: String::from("<generated>"),
    };

    Instrument::new(String::from("Test Sine"), vec![region])
}
