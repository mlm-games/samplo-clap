use crate::dsp::{Adsr, FilterMode, ZdfSvf, db_to_linear, flush_denormals};
use crate::sample::Instrument;

pub struct Voice {
    pub active: bool,
    pub note: u8,
    pub channel: u8,
    pub note_id: Option<i32>,
    pub velocity: f32,

    // Sample playback
    pub region_idx: usize,
    pub position: f64,
    pub playback_rate: f64,

    // Envelope and filter
    pub env: Adsr,
    pub filter: ZdfSvf,

    pub releasing: bool,
    pub age: u64,
}

impl Voice {
    pub fn new(sr: f32) -> Self {
        Self {
            active: false,
            note: 0,
            channel: 0,
            note_id: None,
            velocity: 1.0,

            region_idx: 0,
            position: 0.0,
            playback_rate: 1.0,

            env: Adsr::new(sr),
            filter: ZdfSvf::new(sr),

            releasing: false,
            age: 0,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.env.set_sample_rate(sr);
        self.filter.set_sample_rate(sr);
    }

    pub fn start(
        &mut self,
        channel: u8,
        note: u8,
        velocity: f32,
        region_idx: usize,
        playback_rate: f64,
        age: u64,
    ) {
        self.active = true;
        self.channel = channel;
        self.note = note;
        self.note_id = None;
        self.velocity = velocity;

        self.region_idx = region_idx;
        self.position = 0.0;
        self.playback_rate = playback_rate;

        self.releasing = false;
        self.age = age;

        self.env.reset();
        self.env.note_on();
        self.filter.reset();
    }

    pub fn release(&mut self) {
        if self.active && !self.releasing {
            self.releasing = true;
            self.env.note_off();
        }
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.releasing = false;
        self.env.reset();
    }

    /// Render one stereo sample frame
    pub fn render(
        &mut self,
        instrument: &Instrument,
        filter_cutoff: f32,
        filter_q: f32,
        filter_mode: FilterMode,
        sample_rate: f32,
    ) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }

        // Get envelope
        let env = self.env.next();
        if self.env.is_idle() {
            self.active = false;
            return (0.0, 0.0);
        }

        // Get region
        let region = match instrument.regions.get(self.region_idx) {
            Some(r) => r,
            None => {
                self.active = false;
                return (0.0, 0.0);
            }
        };

        // Check if we've reached the end
        let end_frame = region.num_frames;
        if self.position >= end_frame as f64 {
            if region.loop_enabled {
                if let (Some(start), Some(end)) = (region.loop_start, region.loop_end) {
                    let loop_len = (end - start) as f64;
                    if loop_len > 0.0 {
                        self.position = start as f64 + ((self.position - start as f64) % loop_len);
                    }
                }
            } else {
                // One-shot ended
                self.active = false;
                return (0.0, 0.0);
            }
        }

        // Handle loop during sustain
        if region.loop_enabled && !self.releasing {
            if let (Some(start), Some(end)) = (region.loop_start, region.loop_end) {
                if self.position >= end as f64 {
                    let loop_len = (end - start) as f64;
                    if loop_len > 0.0 {
                        self.position = start as f64 + ((self.position - end as f64) % loop_len);
                    }
                }
            }
        }

        // Get sample
        let (mut l, mut r) = region.get_sample_stereo(self.position);

        // Apply region volume
        let region_gain = db_to_linear(region.volume_db);
        l *= region_gain;
        r *= region_gain;

        // Apply envelope and velocity
        let amp = env * self.velocity;
        l *= amp;
        r *= amp;

        // Apply filter
        self.filter.set(filter_cutoff, filter_q, filter_mode);
        l = self.filter.process(l);
        // For stereo, we'd ideally have two filters, but for simplicity:
        r = self.filter.process(r);

        // Advance position
        self.position += self.playback_rate;

        (flush_denormals(l), flush_denormals(r))
    }
}
