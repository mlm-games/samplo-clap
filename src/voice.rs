use crate::dsp::{Adsr, FilterMode, ZdfSvf, flush_denormals};
use crate::sample::{Instrument, LoopMode};

pub struct Voice {
    pub active: bool,
    pub note: u8,
    pub channel: u8,
    pub note_id: Option<i32>,
    pub velocity: f32,

    pub region_idx: usize,
    pub position: f64,
    pub playback_rate: f64,

    pub env: Adsr,
    pub filter_l: ZdfSvf,
    pub filter_r: ZdfSvf,

    pub releasing: bool,
    pub age: u64,

    last_cutoff: f32,
    last_q: f32,
    last_filter_mode: FilterMode,

    last_a_ms: f32,
    last_d_ms: f32,
    last_s: f32,
    last_r_ms: f32,
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
            filter_l: ZdfSvf::new(sr),
            filter_r: ZdfSvf::new(sr),

            releasing: false,
            age: 0,

            last_cutoff: -1.0,
            last_q: -1.0,
            last_filter_mode: FilterMode::Off,

            last_a_ms: -1.0,
            last_d_ms: -1.0,
            last_s: -1.0,
            last_r_ms: -1.0,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.env.set_sample_rate(sr);
        self.filter_l.set_sample_rate(sr);
        self.filter_r.set_sample_rate(sr);
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
        self.filter_l.reset();
        self.filter_r.reset();
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

    pub fn render(
        &mut self,
        instrument: &Instrument,
        filter_cutoff: f32,
        filter_q: f32,
        filter_mode: FilterMode,
    ) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }

        let env = self.env.next();
        if self.env.is_idle() {
            self.active = false;
            return (0.0, 0.0);
        }

        let region = match instrument.regions.get(self.region_idx) {
            Some(r) => r,
            None => {
                self.active = false;
                return (0.0, 0.0);
            }
        };

        let end_frame = region.num_frames;
        if self.position >= end_frame as f64 {
            match region.loop_mode {
                LoopMode::Continuous => {
                    if let (Some(start), Some(end)) = (region.loop_start, region.loop_end) {
                        let loop_len = (end - start) as f64;
                        if loop_len > 0.0 {
                            self.position =
                                start as f64 + ((self.position - start as f64) % loop_len);
                        }
                    }
                }
                LoopMode::Sustain if !self.releasing => {
                    if let (Some(start), Some(end)) = (region.loop_start, region.loop_end) {
                        let loop_len = (end - start) as f64;
                        if loop_len > 0.0 {
                            self.position =
                                start as f64 + ((self.position - start as f64) % loop_len);
                        }
                    }
                }
                _ => {
                    self.active = false;
                    return (0.0, 0.0);
                }
            }
        }

        // Sustain loop: wrap during sustain, play through end on release
        if region.loop_mode == LoopMode::Sustain && !self.releasing {
            if let (Some(start), Some(end)) = (region.loop_start, region.loop_end) {
                if self.position >= end as f64 {
                    let loop_len = (end - start) as f64;
                    if loop_len > 0.0 {
                        self.position = start as f64 + ((self.position - end as f64) % loop_len);
                    }
                }
            }
        }

        let (mut l, mut r) = region.get_sample_stereo(self.position);

        let region_gain = region.volume_lin;
        l *= region_gain;
        r *= region_gain;

        let amp = env * self.velocity;
        l *= amp;
        r *= amp;

        // Cache filter params: only recompute when they change
        if filter_cutoff != self.last_cutoff
            || filter_q != self.last_q
            || filter_mode != self.last_filter_mode
        {
            self.filter_l.set(filter_cutoff, filter_q, filter_mode);
            self.filter_r.set(filter_cutoff, filter_q, filter_mode);
            self.last_cutoff = filter_cutoff;
            self.last_q = filter_q;
            self.last_filter_mode = filter_mode;
        }
        l = self.filter_l.process(l);
        r = self.filter_r.process(r);

        self.position += self.playback_rate;

        (flush_denormals(l), flush_denormals(r))
    }

    /// Call from process() to update ADSR only on change
    pub fn set_env_ms(&mut self, a_ms: f32, d_ms: f32, s: f32, r_ms: f32) {
        if a_ms == self.last_a_ms
            && d_ms == self.last_d_ms
            && s == self.last_s
            && r_ms == self.last_r_ms
        {
            return;
        }
        self.env.set_ms(a_ms, d_ms, s, r_ms);
        self.last_a_ms = a_ms;
        self.last_d_ms = d_ms;
        self.last_s = s;
        self.last_r_ms = r_ms;
    }
}
