use serde::Deserialize;
use std::sync::Arc;

/// A single audio sample region
pub struct Region {
    /// Sample data: mono or interleaved stereo, normalized to -1..1
    pub data: Arc<Vec<f32>>,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: usize,
    /// Original sample rate of the audio file
    pub sample_rate: f32,
    /// Total number of frames (samples per channel)
    pub num_frames: usize,

    // Mapping
    /// MIDI note at which sample plays at original pitch
    pub root_note: u8,
    /// Lowest MIDI note this region responds to
    pub lo_note: u8,
    /// Highest MIDI note this region responds to
    pub hi_note: u8,
    /// Lowest velocity (0-127)
    pub lo_vel: u8,
    /// Highest velocity (0-127)
    pub hi_vel: u8,

    // Loop points (in frames)
    pub loop_start: Option<usize>,
    pub loop_end: Option<usize>,
    pub loop_enabled: bool,

    // Round robin
    /// Group ID for round robin (regions with same group rotate)
    pub rr_group: u32,
    /// Sequence number within group (0, 1, 2, ...)
    pub rr_seq: u32,

    // Per-region adjustments
    pub tune_cents: f32,
    pub volume_db: f32,
    pub pan: f32,

    /// Original sample path (for debugging)
    pub sample_path: String,
}

impl Region {
    /// Check if this region matches a given note, velocity, and round robin sequence
    #[inline]
    pub fn matches(&self, note: u8, velocity: u8, rr_seq: Option<u32>) -> bool {
        let note_match = note >= self.lo_note && note <= self.hi_note;
        let vel_match = velocity >= self.lo_vel && velocity <= self.hi_vel;
        let rr_match = rr_seq.map(|seq| self.rr_seq == seq).unwrap_or(true);

        note_match && vel_match && rr_match
    }

    /// Check basic note/velocity match (ignoring round robin)
    #[inline]
    pub fn matches_base(&self, note: u8, velocity: u8) -> bool {
        note >= self.lo_note
            && note <= self.hi_note
            && velocity >= self.lo_vel
            && velocity <= self.hi_vel
    }

    /// Calculate playback rate for a given note at a target sample rate
    #[inline]
    pub fn playback_rate(&self, note: u8, target_sr: f32) -> f64 {
        let semitone_diff = (note as f32 - self.root_note as f32) + (self.tune_cents / 100.0);
        let pitch_ratio = 2.0f64.powf(semitone_diff as f64 / 12.0);
        let sr_ratio = self.sample_rate as f64 / target_sr as f64;
        pitch_ratio * sr_ratio
    }

    /// Get stereo samples with interpolation at a fractional position
    #[inline]
    pub fn get_sample_stereo(&self, pos: f64) -> (f32, f32) {
        let idx = pos as usize;
        let frac = (pos - idx as f64) as f32;

        if self.channels == 1 {
            let m = self.interpolate_mono(idx, frac);
            let (gl, gr) = pan_to_gains(self.pan);
            (m * gl, m * gr)
        } else {
            let l = self.interpolate_channel(idx, frac, 0);
            let r = self.interpolate_channel(idx, frac, 1);
            (l, r)
        }
    }

    #[inline]
    fn interpolate_mono(&self, idx: usize, frac: f32) -> f32 {
        let n = self.num_frames;
        if n == 0 {
            return 0.0;
        }

        let i0 = idx.saturating_sub(1).min(n - 1);
        let i1 = idx.min(n - 1);
        let i2 = (idx + 1).min(n - 1);
        let i3 = (idx + 2).min(n - 1);

        crate::dsp::hermite_interp(
            self.data[i0],
            self.data[i1],
            self.data[i2],
            self.data[i3],
            frac,
        )
    }

    #[inline]
    fn interpolate_channel(&self, idx: usize, frac: f32, ch: usize) -> f32 {
        let n = self.num_frames;
        if n == 0 {
            return 0.0;
        }

        let get = |frame: usize| -> f32 {
            let f = frame.min(n - 1);
            self.data[f * 2 + ch]
        };

        let i0 = idx.saturating_sub(1);
        let i1 = idx;
        let i2 = idx + 1;
        let i3 = idx + 2;

        crate::dsp::hermite_interp(get(i0), get(i1), get(i2), get(i3), frac)
    }
}

/// Round robin state tracker
#[derive(Default)]
pub struct RoundRobinState {
    /// Maps (note, rr_group) -> next sequence number
    state: std::collections::HashMap<(u8, u32), u32>,
}

impl RoundRobinState {
    pub fn new() -> Self {
        Self {
            state: std::collections::HashMap::new(),
        }
    }

    /// Get and advance the round robin counter for a note/group
    pub fn next(&mut self, note: u8, group: u32, max_seq: u32) -> u32 {
        let key = (note, group);
        let current = self.state.entry(key).or_insert(0);
        let seq = *current;
        *current = (seq + 1) % (max_seq + 1);
        seq
    }

    /// Reset all counters
    pub fn reset(&mut self) {
        self.state.clear();
    }
}

/// A complete instrument definition
pub struct Instrument {
    pub name: String,
    pub regions: Vec<Region>,
    /// Maps (note, velocity_layer, rr_group) -> max rr_seq for that combo
    rr_max: std::collections::HashMap<(u8, u8, u32), u32>,
}

impl Instrument {
    pub fn empty() -> Self {
        Self {
            name: String::from("Empty"),
            regions: Vec::new(),
            rr_max: std::collections::HashMap::new(),
        }
    }

    pub fn new(name: String, regions: Vec<Region>) -> Self {
        let mut inst = Self {
            name,
            regions,
            rr_max: std::collections::HashMap::new(),
        };
        inst.build_rr_map();
        inst
    }

    /// Build the round robin max sequence map
    fn build_rr_map(&mut self) {
        self.rr_max.clear();

        for region in &self.regions {
            // For each note this region covers
            for note in region.lo_note..=region.hi_note {
                // Simplified: use velocity midpoint as layer key
                let vel_layer = (region.lo_vel / 32).min(3);
                let key = (note, vel_layer, region.rr_group);

                let entry = self.rr_max.entry(key).or_insert(0);
                *entry = (*entry).max(region.rr_seq);
            }
        }
    }

    /// Get max round robin sequence for a note/group
    pub fn get_rr_max(&self, note: u8, velocity: u8, group: u32) -> u32 {
        let vel_layer = (velocity / 32).min(3);
        self.rr_max
            .get(&(note, vel_layer, group))
            .copied()
            .unwrap_or(0)
    }

    /// Find the best matching region for a note/velocity, with round robin
    pub fn find_region(
        &self,
        note: u8,
        velocity: u8,
        rr_state: &mut RoundRobinState,
    ) -> Option<usize> {
        // First pass: find all matching regions and determine groups
        let mut matches: Vec<(usize, u32, u32)> = Vec::new(); // (index, group, seq)

        for (i, region) in self.regions.iter().enumerate() {
            if region.matches_base(note, velocity) {
                matches.push((i, region.rr_group, region.rr_seq));
            }
        }

        if matches.is_empty() {
            return None;
        }

        // If only one match, return it
        if matches.len() == 1 {
            return Some(matches[0].0);
        }

        // Group by rr_group and select based on round robin
        // For simplicity, take the first group we encounter
        let group = matches[0].1;
        let max_seq = self.get_rr_max(note, velocity, group);
        let target_seq = rr_state.next(note, group, max_seq);

        // Find region with matching sequence, or fall back to first
        matches
            .iter()
            .find(|(_, g, s)| *g == group && *s == target_seq)
            .or_else(|| matches.first())
            .map(|(i, _, _)| *i)
    }

    /// Find all matching regions (for layering without round robin)
    pub fn find_all_regions(&self, note: u8, velocity: u8) -> Vec<usize> {
        self.regions
            .iter()
            .enumerate()
            .filter(|(_, r)| r.matches_base(note, velocity))
            .map(|(i, _)| i)
            .collect()
    }
}

/// JSON definition format
#[derive(Deserialize)]
pub struct InstrumentDef {
    pub name: String,
    #[serde(default)]
    pub regions: Vec<RegionDef>,
}

#[derive(Deserialize)]
pub struct RegionDef {
    pub sample: String,
    #[serde(default = "default_root")]
    pub root: u8,
    #[serde(default)]
    pub lo_note: Option<u8>,
    #[serde(default)]
    pub hi_note: Option<u8>,
    #[serde(default)]
    pub lo_vel: Option<u8>,
    #[serde(default)]
    pub hi_vel: Option<u8>,
    #[serde(default)]
    pub loop_start: Option<usize>,
    #[serde(default)]
    pub loop_end: Option<usize>,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default)]
    pub rr_group: u32,
    #[serde(default)]
    pub rr_seq: u32,
    #[serde(default)]
    pub tune_cents: f32,
    #[serde(default)]
    pub volume_db: f32,
    #[serde(default)]
    pub pan: f32,
}

fn default_root() -> u8 {
    60
}

#[inline]
fn pan_to_gains(pan: f32) -> (f32, f32) {
    let x = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5;
    let theta = x * core::f32::consts::FRAC_PI_2;
    (theta.cos(), theta.sin())
}
