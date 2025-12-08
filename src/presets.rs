use crate::params::{FilterMode, VitsParams, WaveType};

// later (when yadaw preset loading is implemented)
pub struct Preset<'a> {
    pub name: &'a str,
    pub set: fn(&mut VitsParams),
}

pub const FACTORY_PRESETS: &[Preset<'_>] = &[
    Preset {
        name: "Init",
        set: |p| *p = VitsParams::default(),
    },
    Preset {
        name: "Soft Saw Pad",
        set: |p| {
            *p = VitsParams::with_values(
                0.9,                 // gain
                32,                  // max_voices
                WaveType::Saw,       // wave
                0.5,                 // osc_mix
                8.0,                 // detune (cents)
                300.0,               // attack_ms
                1200.0,              // decay_ms
                0.8,                 // sustain
                2000.0,              // release_ms
                FilterMode::LowPass, // filter_mode
                1200.0,              // cutoff_hz
                0.15,                // resonance
                0.0,                 // mod_cutoff
                0.0,                 // mod_gain
            );
        },
    },
    Preset {
        name: "Pluck",
        set: |p| {
            *p = VitsParams::with_values(
                0.8,                 // gain
                32,                  // max_voices
                WaveType::Square,    // wave
                0.5,                 // osc_mix
                3.0,                 // detune (cents)
                2.0,                 // attack_ms
                180.0,               // decay_ms
                0.0,                 // sustain
                120.0,               // release_ms
                FilterMode::LowPass, // filter_mode
                2200.0,              // cutoff_hz
                0.25,                // resonance
                0.0,                 // mod_cutoff
                0.0,                 // mod_gain
            );
        },
    },
];
