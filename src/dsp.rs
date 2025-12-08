use core::f32::consts::PI;

#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

#[inline]
pub fn flush_denormals(x: f32) -> f32 {
    if x.abs() < 1e-24 { 0.0 } else { x }
}

#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// Linear interpolation between two samples
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Hermite 4-point interpolation for higher quality pitch shifting
#[inline]
pub fn hermite_interp(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + c0
}

// Zero-delay TPT state variable filter
pub struct ZdfSvf {
    sr: f32,
    ic1eq: f32,
    ic2eq: f32,
    g: f32,
    r: f32,
    mode: FilterMode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    Off,
    LP,
    HP,
    BP,
}

impl ZdfSvf {
    pub fn new(sr: f32) -> Self {
        Self {
            sr: sr.max(1.0),
            ic1eq: 0.0,
            ic2eq: 0.0,
            g: 0.0,
            r: 1.0,
            mode: FilterMode::Off,
        }
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr.max(1.0);
    }

    #[inline]
    pub fn set(&mut self, cutoff_hz: f32, q: f32, mode: FilterMode) {
        let f = (cutoff_hz / self.sr).clamp(1e-5, 0.49);
        self.g = (PI * f).tan();
        self.r = (1.0 / q.max(0.05)).clamp(0.02, 10.0);
        self.mode = mode;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        if self.mode == FilterMode::Off {
            return x;
        }
        let h = 1.0 / (1.0 + self.g * (self.g + self.r));
        let v1 = h * (self.ic1eq + self.g * (x - self.ic2eq));
        let v2 = self.ic2eq + self.g * v1;
        self.ic1eq = flush_denormals(2.0 * v1 - self.ic1eq);
        self.ic2eq = flush_denormals(2.0 * v2 - self.ic2eq);
        match self.mode {
            FilterMode::LP => v2,
            FilterMode::BP => v1,
            FilterMode::HP => x - self.r * v1 - v2,
            FilterMode::Off => x,
        }
    }
}

/// ADSR envelope generator
pub struct Adsr {
    sr: f32,
    a_samples: f32,
    d_samples: f32,
    s_level: f32,
    r_samples: f32,
    level: f32,
    state: AdsrState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AdsrState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

impl Adsr {
    pub fn new(sr: f32) -> Self {
        Self {
            sr: sr.max(1.0),
            a_samples: 0.0,
            d_samples: 0.0,
            s_level: 1.0,
            r_samples: 0.0,
            level: 0.0,
            state: AdsrState::Idle,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr.max(1.0);
    }

    pub fn set_ms(&mut self, a_ms: f32, d_ms: f32, s: f32, r_ms: f32) {
        self.a_samples = (a_ms.max(0.0) / 1000.0) * self.sr;
        self.d_samples = (d_ms.max(0.0) / 1000.0) * self.sr;
        self.s_level = s.clamp(0.0, 1.0);
        self.r_samples = (r_ms.max(0.0) / 1000.0) * self.sr;
    }

    pub fn note_on(&mut self) {
        self.state = AdsrState::Attack;
        // Don't reset level - allows legato-style re-triggering
    }

    pub fn note_off(&mut self) {
        if self.state != AdsrState::Idle {
            self.state = AdsrState::Release;
        }
    }

    pub fn reset(&mut self) {
        self.level = 0.0;
        self.state = AdsrState::Idle;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.state {
            AdsrState::Idle => {
                self.level = 0.0;
            }
            AdsrState::Attack => {
                let inc = if self.a_samples <= 1.0 {
                    1.0
                } else {
                    1.0 / self.a_samples
                };
                self.level += inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.state = AdsrState::Decay;
                }
            }
            AdsrState::Decay => {
                let dec = if self.d_samples <= 1.0 {
                    1.0
                } else {
                    1.0 / self.d_samples
                };
                self.level -= dec;
                if self.level <= self.s_level {
                    self.level = self.s_level;
                    self.state = AdsrState::Sustain;
                }
            }
            AdsrState::Sustain => {
                // Hold at sustain level
            }
            AdsrState::Release => {
                let rel = if self.r_samples <= 1.0 {
                    1.0
                } else {
                    1.0 / self.r_samples
                };
                self.level -= rel;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.state = AdsrState::Idle;
                }
            }
        }
        self.level.clamp(0.0, 1.0)
    }

    #[inline]
    pub fn is_idle(&self) -> bool {
        self.state == AdsrState::Idle
    }
}
