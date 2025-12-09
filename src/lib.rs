mod dsp;
mod loader;
mod params;
mod sample;
mod sfz;
mod voice;

use nih_plug::prelude::*;
use params::SamploParams;
use sample::{Instrument, RoundRobinState};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use voice::Voice;

const MAX_VOICES: usize = 64;

pub struct Samplo {
    params: Arc<SamploParams>,
    sample_rate: f32,
    voices: Vec<Voice>,
    instrument: Instrument,
    rr_state: RoundRobinState,
    frame_counter: u64,

    current_instrument_idx: usize,
}

impl Default for Samplo {
    fn default() -> Self {
        let sr = 44100.0;
        Self {
            params: Arc::new(SamploParams::default()),
            sample_rate: sr,
            voices: (0..MAX_VOICES).map(|_| Voice::new(sr)).collect(),
            instrument: Instrument::empty(),
            rr_state: RoundRobinState::new(),
            frame_counter: 0,
            current_instrument_idx: 0,
        }
    }
}

impl Plugin for Samplo {
    const NAME: &'static str = "Samplo";
    const VENDOR: &'static str = "mlm-games";
    const URL: &'static str = "https://github.com/mlm-games/samplo-clap";
    const EMAIL: &'static str = "me@example.com";
    const VERSION: &'static str = "1.1.0";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        aux_input_ports: &[],
        aux_output_ports: &[],
        names: PortNames::const_default(),
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = BackgroundTask;

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _io: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _ctx: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;

        for voice in &mut self.voices {
            voice.set_sample_rate(self.sample_rate);
        }

        self.scan_and_register_instruments();

        if self.instrument.regions.is_empty() {
            self.instrument = loader::create_test_instrument(self.sample_rate);
            nih_log!("Loaded test sine instrument");
        }

        true
    }

    fn reset(&mut self) {
        self.frame_counter = 0;
        self.rr_state.reset();
        for voice in &mut self.voices {
            *voice = Voice::new(self.sample_rate);
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer<'_>,
        _aux: &mut AuxiliaryBuffers<'_>,
        ctx: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.frame_counter = self.frame_counter.wrapping_add(1);

        let params = self.params.clone();

        let inst_idx = params.instrument_index.value().max(0) as usize;
        if inst_idx != self.current_instrument_idx {
            self.current_instrument_idx = inst_idx;

            let list = instruments().lock().unwrap();
            if let Some(slot) = list.get(self.current_instrument_idx) {
                match self.load_instrument_from_path(&slot.path) {
                    Ok(()) => {
                        self.rr_state.reset();
                        nih_log!("Samplo: loaded instrument '{}'", slot.name);
                    }
                    Err(e) => {
                        nih_log!("Samplo: failed to load instrument {:?}: {}", slot.path, e);
                    }
                }
            }
        }

        let desired_voices = params.max_voices.value() as usize;
        self.resize_voice_pool(desired_voices);

        let attack = params.attack_ms.value();
        let decay = params.decay_ms.value();
        let sustain = params.sustain.value();
        let release = params.release_ms.value();
        let cutoff = params.cutoff_hz.value();
        let res = params.resonance.value();
        let filter_mode = params.filter_mode.value().to_dsp();
        let gain = params.gain.value();
        let pan = params.pan.value();
        let tune = params.tune_cents.value();
        let vel_sens = params.velocity_sens.value();

        let mut next_event = ctx.next_event();

        for (sample_idx, mut frame) in buffer.iter_samples().enumerate() {
            while let Some(ev) = next_event {
                if ev.timing() != sample_idx as u32 {
                    break;
                }

                match ev {
                    NoteEvent::NoteOn {
                        channel,
                        note,
                        velocity,
                        voice_id,
                        ..
                    } => {
                        self.note_on(channel, note, velocity, voice_id, tune, vel_sens);
                    }
                    NoteEvent::NoteOff {
                        channel,
                        note,
                        voice_id,
                        ..
                    } => {
                        self.note_off(channel, note, voice_id);
                    }
                    _ => {}
                }

                next_event = ctx.next_event();
            }

            let mut out_l = 0.0f32;
            let mut out_r = 0.0f32;

            for voice in &mut self.voices {
                if !voice.active {
                    continue;
                }

                voice.env.set_ms(attack, decay, sustain, release);

                let (l, r) =
                    voice.render(&self.instrument, cutoff, res, filter_mode, self.sample_rate);

                out_l += l;
                out_r += r;

                if !voice.active {
                    ctx.send_event(NoteEvent::VoiceTerminated {
                        timing: sample_idx as u32,
                        voice_id: voice.note_id,
                        channel: voice.channel,
                        note: voice.note,
                    });
                }
            }

            let (pan_l, pan_r) = pan_to_gains(pan);
            out_l *= gain * pan_l;
            out_r *= gain * pan_r;

            out_l = dsp::fast_tanh(out_l);
            out_r = dsp::fast_tanh(out_r);

            let mut ch = frame.iter_mut();
            if let Some(s) = ch.next() {
                *s = out_l;
            }
            if let Some(s) = ch.next() {
                *s = out_r;
            }
        }

        ProcessStatus::Normal
    }

    fn task_executor(&mut self) -> TaskExecutor<Self> {
        Box::new(|task| match task {
            BackgroundTask::LoadInstrument(path) => {
                nih_log!("Loading instrument: {:?}", path);
            }
        })
    }
}

pub enum BackgroundTask {
    LoadInstrument(PathBuf),
}

impl Samplo {
    fn resize_voice_pool(&mut self, target: usize) {
        let target = target.min(MAX_VOICES);
        if target > self.voices.len() {
            self.voices
                .extend((self.voices.len()..target).map(|_| Voice::new(self.sample_rate)));
        } else if target < self.voices.len() {
            self.voices.truncate(target);
        }
    }

    fn alloc_voice(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return i;
        }

        let mut oldest_idx = 0;
        let mut oldest_age = u64::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if v.age < oldest_age {
                oldest_age = v.age;
                oldest_idx = i;
            }
        }
        oldest_idx
    }

    fn note_on(
        &mut self,
        channel: u8,
        note: u8,
        velocity: f32,
        voice_id: Option<i32>,
        tune_cents: f32,
        vel_sens: f32,
    ) {
        let midi_vel = (velocity * 127.0) as u8;

        // Find matching region with round robin
        let region_idx = match self
            .instrument
            .find_region(note, midi_vel, &mut self.rr_state)
        {
            Some(idx) => idx,
            None => return,
        };

        let region = &self.instrument.regions[region_idx];

        let tune_ratio = 2.0f64.powf(tune_cents as f64 / 1200.0);
        let playback_rate = region.playback_rate(note, self.sample_rate) * tune_ratio;

        let vel_amount = 1.0 - vel_sens + vel_sens * velocity;

        let slot = self.alloc_voice();
        let voice = &mut self.voices[slot];
        voice.start(
            channel,
            note,
            vel_amount,
            region_idx,
            playback_rate,
            self.frame_counter,
        );
        voice.note_id = voice_id;
    }

    fn note_off(&mut self, channel: u8, note: u8, voice_id: Option<i32>) {
        for voice in &mut self.voices {
            if voice.active
                && voice.channel == channel
                && voice.note == note
                && (voice_id.is_none() || voice.note_id == voice_id)
            {
                voice.release();
            }
        }
    }

    /// Load an instrument from a path (JSON or SFZ)
    pub fn load_instrument_from_path(&mut self, path: &std::path::Path) -> Result<(), String> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let instrument = match ext {
            "json" => loader::load_instrument_json(path)?,
            "sfz" => sfz::load_sfz(path)?,
            _ => return Err(format!("Unknown format: {}", ext)),
        };

        self.instrument = instrument;
        self.rr_state.reset();
        nih_log!("Loaded instrument: {}", self.instrument.name);

        Ok(())
    }

    /// Scan for instrument definition files and populate the global instrument list.
    fn scan_and_register_instruments(&mut self) {
        use crate::loader::scan_instruments;

        let paths_to_try = [
            PathBuf::from("./instruments"),
            PathBuf::from("/storage/emulated/0/Samplo/instruments"),
            dirs::document_dir()
                .map(|d| d.join("Samplo/instruments"))
                .unwrap_or_default(),
        ];

        let mut slots = Vec::new();

        for dir in &paths_to_try {
            if !dir.exists() {
                continue;
            }

            nih_log!("Samplo: searching instruments in {:?}", dir);
            for path in scan_instruments(dir, 2) {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("<unnamed>")
                    .to_string();
                slots.push(InstrumentSlot { name, path });
            }
        }

        if !slots.is_empty() {
            let mut g = instruments().lock().unwrap();
            *g = slots;
            nih_log!("Samplo: found {} instruments", g.len());
        } else {
            nih_log!("Samplo: no instruments found, using test sine");
        }
    }
}

#[inline]
fn pan_to_gains(pan: f32) -> (f32, f32) {
    let x = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5;
    let theta = x * core::f32::consts::FRAC_PI_2;
    (theta.cos(), theta.sin())
}

impl ClapPlugin for Samplo {
    const CLAP_ID: &'static str = "dev.mlm-games.samplo";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Minimal sample player with SFZ support for Android and desktop");
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Sampler,
        ClapFeature::Stereo,
    ];

    fn remote_controls(&self, _context: &mut impl RemoteControlsContext) {}

    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
}

nih_export_clap!(Samplo);

/// One available instrument on disk
#[derive(Clone)]
pub struct InstrumentSlot {
    pub name: String,  // Display name (e.g. file stem)
    pub path: PathBuf, // Full path to .sfz/.json
}

/// Global list of discovered instruments, shared across plugin instances.
static GLOBAL_INSTRUMENTS: OnceLock<Mutex<Vec<InstrumentSlot>>> = OnceLock::new();

fn instruments() -> &'static Mutex<Vec<InstrumentSlot>> {
    GLOBAL_INSTRUMENTS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Helper used by the param's `value_to_string`:
/// map an index to a human‑readable instrument name.
pub fn instrument_name_for_index(idx: i32) -> String {
    let list = instruments().lock().unwrap();
    if list.is_empty() {
        return "None".to_string();
    }
    let clamped = idx.clamp(0, list.len().saturating_sub(1) as i32) as usize;
    list.get(clamped)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "None".to_string())
}
