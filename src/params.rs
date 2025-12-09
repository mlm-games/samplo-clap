use nih_plug::prelude::*;

#[derive(Params)]
pub struct SamploParams {
    // Amplitude envelope
    #[id = "att"]
    pub attack_ms: FloatParam,
    #[id = "dec"]
    pub decay_ms: FloatParam,
    #[id = "sus"]
    pub sustain: FloatParam,
    #[id = "rel"]
    pub release_ms: FloatParam,

    // Filter
    #[id = "f_mode"]
    pub filter_mode: EnumParam<FilterModeParam>,
    #[id = "f_cut"]
    pub cutoff_hz: FloatParam,
    #[id = "f_res"]
    pub resonance: FloatParam,

    // Output
    #[id = "gain"]
    pub gain: FloatParam,
    #[id = "pan"]
    pub pan: FloatParam,

    // Playback
    #[id = "tune"]
    pub tune_cents: FloatParam,
    #[id = "voices"]
    pub max_voices: IntParam,
    #[id = "vel_sens"]
    pub velocity_sens: FloatParam,

    /// Instrument selection (idx into scanned instrument list)
    #[id = "inst"]
    pub instrument_index: IntParam,
}

#[derive(PartialEq, Eq, Clone, Copy, Enum)]
pub enum FilterModeParam {
    Off,
    LowPass,
    HighPass,
    BandPass,
}

impl Default for SamploParams {
    fn default() -> Self {
        Self {
            attack_ms: FloatParam::new(
                "Attack",
                5.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 2000.0,
                    factor: 0.3,
                },
            )
            .with_unit(" ms"),

            decay_ms: FloatParam::new(
                "Decay",
                100.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 4000.0,
                    factor: 0.3,
                },
            )
            .with_unit(" ms"),

            sustain: FloatParam::new("Sustain", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 }),

            release_ms: FloatParam::new(
                "Release",
                200.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 8000.0,
                    factor: 0.3,
                },
            )
            .with_unit(" ms"),

            filter_mode: EnumParam::new("Filter", FilterModeParam::Off),

            cutoff_hz: FloatParam::new(
                "Cutoff",
                8000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20000.0,
                    factor: 0.2,
                },
            )
            .with_unit(" Hz"),

            resonance: FloatParam::new("Resonance", 0.5, FloatRange::Linear { min: 0.1, max: 4.0 }),

            gain: FloatParam::new("Gain", 0.8, FloatRange::Linear { min: 0.0, max: 2.0 })
                .with_unit("×"),

            pan: FloatParam::new(
                "Pan",
                0.0,
                FloatRange::Linear {
                    min: -1.0,
                    max: 1.0,
                },
            ),

            tune_cents: FloatParam::new(
                "Tune",
                0.0,
                FloatRange::Linear {
                    min: -100.0,
                    max: 100.0,
                },
            )
            .with_unit(" cents"),

            max_voices: IntParam::new("Voices", 32, IntRange::Linear { min: 1, max: 64 }),

            velocity_sens: FloatParam::new(
                "Vel Sens",
                0.7,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),

            instrument_index: {
                use std::sync::Arc;
                IntParam::new("Instrument", 0, IntRange::Linear { min: 0, max: 127 })
                    .with_value_to_string(Arc::new(|idx| crate::instrument_name_for_index(idx)))
            },
        }
    }
}

impl FilterModeParam {
    pub fn to_dsp(&self) -> crate::dsp::FilterMode {
        match self {
            FilterModeParam::Off => crate::dsp::FilterMode::Off,
            FilterModeParam::LowPass => crate::dsp::FilterMode::LP,
            FilterModeParam::HighPass => crate::dsp::FilterMode::HP,
            FilterModeParam::BandPass => crate::dsp::FilterMode::BP,
        }
    }
}
