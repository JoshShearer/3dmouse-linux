//! Six-axis signal pipeline (FR-200).
//!
//! Order (SRS §3 FR-200, documented in docs/architecture/axis-pipeline.md):
//! ```text
//! raw canonical i32 -> centre offset (software calibration, FR-207)
//!   -> normalise by model range, clamp to [-1, 1]            (FR-202)
//!   -> axis remap (out[i] = in[axis_map[i]]), invert           (FR-203)
//!   -> deadzone with rescale: sign(x)·max(0,|x|-d)/(1-d)        (FR-203)
//!   -> response curve                                           (FR-204)
//!   -> gain: per-axis × translation|rotation × master           (FR-203)
//!   -> first-order low-pass, tau = smoothing_time_ms            (FR-205)
//!   -> mode constraints: enable masks, dominant axis            (FR-206)
//!   -> clamp to [-MAX_OUT, MAX_OUT]
//! ```
//! The same pipeline output feeds every backend (FR-210).
#![allow(clippy::needless_range_loop)] // per-axis math reads clearer indexed

use serde::{Deserialize, Serialize};
use sm_core::AXES;

/// Upper bound for any normalised output (gain may exceed 1).
pub const MAX_OUT: f32 = 8.0;
/// libspnav clients expect integer motion of roughly ±350 at full
/// deflection with sensitivity 1.0 (spacenavd convention).
pub const SPNAV_SCALE: f32 = 350.0;
/// A gap longer than this resets time-dependent state (suspend/resume, §4.3).
pub const MAX_DT_US: u64 = 250_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Curve {
    /// y = x
    #[default]
    Linear,
    /// y = sign(x)·|x|^exponent, exponent in [0.2, 5]
    Power { exponent: f32 },
    /// Blend of linear and cubic: y = (1-k)·x + k·x³, k in [0, 1].
    /// Monotonic for k ≤ 1, y(±1) = ±1.
    SCurve { k: f32 },
}

impl Curve {
    pub fn eval(&self, x: f32) -> f32 {
        match *self {
            Curve::Linear => x,
            Curve::Power { exponent } => x.signum() * x.abs().powf(exponent),
            Curve::SCurve { k } => (1.0 - k) * x + k * x * x * x,
        }
    }
}

fn default_true() -> bool {
    true
}
fn ones() -> [f32; AXES] {
    [1.0; AXES]
}
fn one() -> f32 {
    1.0
}
fn identity_map() -> [u8; AXES] {
    [0, 1, 2, 3, 4, 5]
}
fn default_deadzone() -> [f32; AXES] {
    [0.02; AXES]
}

/// Navigation settings (the `navigation` table of a profile, SRS §5).
/// Units: gains are dimensionless multipliers; deadzones are fractions of
/// full deflection; smoothing is a time constant in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavConfig {
    #[serde(default = "one")]
    pub master_gain: f32,
    #[serde(default = "one")]
    pub translation_gain: f32,
    #[serde(default = "one")]
    pub rotation_gain: f32,
    #[serde(default = "identity_map")]
    pub axis_map: [u8; AXES],
    #[serde(default = "ones")]
    pub per_axis_gain: [f32; AXES],
    pub per_axis_invert: [bool; AXES],
    /// Per-axis lock (FR-206 axis lock); true = axis enabled.
    pub per_axis_enabled: [bool; AXES],
    #[serde(default = "default_deadzone")]
    pub deadzone: [f32; AXES],
    pub response_curve: Curve,
    pub smoothing_time_ms: f32,
    pub dominant_axis: bool,
    #[serde(default = "default_true")]
    pub translation_enabled: bool,
    #[serde(default = "default_true")]
    pub rotation_enabled: bool,
    /// Software neutral offset in raw canonical units (FR-207).
    pub center_offset: [i32; AXES],
}

impl Default for NavConfig {
    fn default() -> Self {
        NavConfig {
            master_gain: 1.0,
            translation_gain: 1.0,
            rotation_gain: 1.0,
            axis_map: identity_map(),
            per_axis_gain: ones(),
            per_axis_invert: [false; AXES],
            per_axis_enabled: [true; AXES],
            deadzone: default_deadzone(),
            response_curve: Curve::Linear,
            smoothing_time_ms: 0.0,
            dominant_axis: false,
            translation_enabled: true,
            rotation_enabled: true,
            center_offset: [0; AXES],
        }
    }
}

impl NavConfig {
    /// Validate ranges; returns human-readable problems.
    pub fn validate(&self) -> Result<(), String> {
        let fin = |v: f32| v.is_finite();
        let gain_ok = |v: f32| fin(v) && (0.0..=10.0).contains(&v);
        if !gain_ok(self.master_gain) || !gain_ok(self.translation_gain) || !gain_ok(self.rotation_gain) {
            return Err("gains must be finite and within [0, 10]".into());
        }
        if !self.per_axis_gain.iter().all(|g| gain_ok(*g)) {
            return Err("per_axis_gain must be within [0, 10]".into());
        }
        let mut seen = [false; AXES];
        for &m in &self.axis_map {
            if m as usize >= AXES || seen[m as usize] {
                return Err("axis_map must be a permutation of 0..5".into());
            }
            seen[m as usize] = true;
        }
        if !self.deadzone.iter().all(|d| fin(*d) && (0.0..=0.9).contains(d)) {
            return Err("deadzone must be within [0, 0.9]".into());
        }
        match self.response_curve {
            Curve::Linear => {}
            Curve::Power { exponent } if fin(exponent) && (0.2..=5.0).contains(&exponent) => {}
            Curve::SCurve { k } if fin(k) && (0.0..=1.0).contains(&k) => {}
            _ => return Err("response_curve parameter out of range".into()),
        }
        if !fin(self.smoothing_time_ms) || !(0.0..=1000.0).contains(&self.smoothing_time_ms) {
            return Err("smoothing_time_ms must be within [0, 1000]".into());
        }
        if !self.center_offset.iter().all(|c| c.abs() <= 10_000) {
            return Err("center_offset out of range".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Motion {
    /// Normalised output (1.0 = full deflection at unity gain).
    pub norm: [f32; AXES],
    /// libspnav-compatible integers.
    pub spnav: [i32; AXES],
}

impl Motion {
    pub fn is_zero(&self) -> bool {
        self.spnav.iter().all(|v| *v == 0) && self.norm.iter().all(|v| *v == 0.0)
    }
}

#[derive(Debug, Clone)]
pub struct Pipeline {
    cfg: NavConfig,
    axis_range: f32,
    state: [f32; AXES],
    last_t_us: Option<u64>,
}

impl Pipeline {
    pub fn new(cfg: NavConfig, axis_range: i32) -> Self {
        Pipeline { cfg, axis_range: axis_range.max(1) as f32, state: [0.0; AXES], last_t_us: None }
    }

    pub fn config(&self) -> &NavConfig {
        &self.cfg
    }

    /// Atomically replace the config (FR-404). Filter state is kept so a
    /// profile switch during motion does not jump; it decays naturally.
    pub fn set_config(&mut self, cfg: NavConfig) {
        self.cfg = cfg;
    }

    pub fn set_axis_range(&mut self, r: i32) {
        self.axis_range = r.max(1) as f32;
    }

    /// Clear filter state (disconnect, resume) – FR-104/FR-208.
    pub fn reset(&mut self) {
        self.state = [0.0; AXES];
        self.last_t_us = None;
    }

    pub fn is_settled(&self) -> bool {
        self.state.iter().all(|v| *v == 0.0)
    }

    /// Run one sample (raw canonical values) at monotonic time `t_us`.
    pub fn process(&mut self, raw: [i32; AXES], t_us: u64) -> Motion {
        let c = &self.cfg;
        let mut x = [0f32; AXES];
        for i in 0..AXES {
            let v = (raw[i] - c.center_offset[i]) as f32 / self.axis_range;
            x[i] = v.clamp(-1.0, 1.0);
        }
        let mut y = [0f32; AXES];
        for i in 0..AXES {
            let mut v = x[c.axis_map[i] as usize];
            if c.per_axis_invert[i] {
                v = -v;
            }
            let d = c.deadzone[i];
            let a = v.abs();
            v = if a <= d { 0.0 } else { v.signum() * (a - d) / (1.0 - d) };
            v = c.response_curve.eval(v);
            let group = if i < 3 { c.translation_gain } else { c.rotation_gain };
            v *= c.per_axis_gain[i] * group * c.master_gain;
            y[i] = v;
        }

        // low-pass
        let dt_us = match self.last_t_us {
            Some(p) if t_us >= p => t_us - p,
            _ => u64::MAX,
        };
        self.last_t_us = Some(t_us);
        let tau = c.smoothing_time_ms * 1000.0;
        if tau <= 0.0 || dt_us > MAX_DT_US {
            self.state = y;
        } else {
            let alpha = 1.0 - (-(dt_us as f32) / tau).exp();
            for i in 0..AXES {
                self.state[i] += alpha * (y[i] - self.state[i]);
                // no drift: snap tiny residues when the target is neutral
                if y[i] == 0.0 && self.state[i].abs() < 1e-3 {
                    self.state[i] = 0.0;
                }
            }
        }

        let mut out = self.state;
        for (i, o) in out.iter_mut().enumerate() {
            let group_on = if i < 3 { c.translation_enabled } else { c.rotation_enabled };
            if !group_on || !c.per_axis_enabled[i] {
                *o = 0.0;
            }
        }
        if c.dominant_axis {
            let (mut best, mut bv) = (0usize, 0f32);
            for (i, o) in out.iter().enumerate() {
                if o.abs() > bv {
                    bv = o.abs();
                    best = i;
                }
            }
            for (i, o) in out.iter_mut().enumerate() {
                if i != best {
                    *o = 0.0;
                }
            }
        }
        let mut m = Motion::default();
        for i in 0..AXES {
            let v = out[i].clamp(-MAX_OUT, MAX_OUT);
            m.norm[i] = v;
            m.spnav[i] = (v * SPNAV_SCALE).round() as i32;
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn p(cfg: NavConfig) -> Pipeline {
        Pipeline::new(cfg, 350)
    }

    #[test]
    fn identity_full_scale() {
        let mut pl = p(NavConfig { deadzone: [0.0; 6], ..Default::default() });
        let m = pl.process([350, -350, 175, 0, 0, 0], 0);
        assert_eq!(m.spnav, [350, -350, 175, 0, 0, 0]);
        assert!((m.norm[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn deadzone_rescales_continuously() {
        let mut pl = p(NavConfig { deadzone: [0.1; 6], ..Default::default() });
        assert_eq!(pl.process([35, 0, 0, 0, 0, 0], 0).norm[0], 0.0);
        let m = pl.process([350, 0, 0, 0, 0, 0], 1);
        assert!((m.norm[0] - 1.0).abs() < 1e-6);
        let m = pl.process([36, 0, 0, 0, 0, 0], 2);
        assert!(m.norm[0] > 0.0 && m.norm[0] < 0.01);
    }

    #[test]
    fn invert_remap_gain() {
        let cfg = NavConfig {
            deadzone: [0.0; 6],
            axis_map: [1, 0, 2, 3, 4, 5],
            per_axis_invert: [true, false, false, false, false, false],
            rotation_gain: 2.0,
            ..Default::default()
        };
        let mut pl = p(cfg);
        let m = pl.process([100, 200, 0, 35, 0, 0], 0);
        assert_eq!(m.spnav[0], -200);
        assert_eq!(m.spnav[1], 100);
        assert_eq!(m.spnav[3], 70);
    }

    #[test]
    fn curves() {
        assert_eq!(Curve::Power { exponent: 2.0 }.eval(-0.5), -0.25);
        let s = Curve::SCurve { k: 1.0 };
        assert_eq!(s.eval(1.0), 1.0);
        assert!((s.eval(0.5) - 0.125).abs() < 1e-6);
    }

    #[test]
    fn dominant_and_masks() {
        let mut pl = p(NavConfig { deadzone: [0.0; 6], dominant_axis: true, ..Default::default() });
        let m = pl.process([100, 300, 0, 200, 0, 0], 0);
        assert_eq!(m.spnav, [0, 300, 0, 0, 0, 0]);
        let mut pl = p(NavConfig { deadzone: [0.0; 6], rotation_enabled: false, ..Default::default() });
        assert_eq!(pl.process([1, 1, 1, 100, 100, 100], 0).spnav, [1, 1, 1, 0, 0, 0]);
    }

    #[test]
    fn smoothing_converges_and_never_drifts() {
        let mut pl = p(NavConfig { deadzone: [0.0; 6], smoothing_time_ms: 20.0, ..Default::default() });
        let mut t = 0;
        let m = pl.process([350, 0, 0, 0, 0, 0], t);
        assert_eq!(m.norm[0], 1.0); // first sample: no history
        for _ in 0..100 {
            t += 8_000;
            pl.process([0; 6], t);
        }
        assert!(pl.is_settled(), "filter must reach exact zero");
    }

    #[test]
    fn timestamp_jump_resets_filter() {
        let mut pl = p(NavConfig { deadzone: [0.0; 6], smoothing_time_ms: 500.0, ..Default::default() });
        pl.process([350, 0, 0, 0, 0, 0], 0);
        pl.process([350, 0, 0, 0, 0, 0], 8_000);
        // resume after suspend: new value is taken directly
        let m = pl.process([0; 6], 10_000_000);
        assert_eq!(m.norm[0], 0.0);
        // backwards time also resets instead of producing NaN
        let m = pl.process([175, 0, 0, 0, 0, 0], 5);
        assert!((m.norm[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn validation() {
        assert!(NavConfig::default().validate().is_ok());
        assert!(NavConfig { axis_map: [0, 0, 1, 2, 3, 4], ..Default::default() }.validate().is_err());
        assert!(NavConfig { master_gain: f32::NAN, ..Default::default() }.validate().is_err());
        assert!(NavConfig { response_curve: Curve::Power { exponent: 50.0 }, ..Default::default() }.validate().is_err());
    }

    fn arb_cfg() -> impl Strategy<Value = NavConfig> {
        (0.0f32..10.0, 0.0f32..10.0, 0.0f32..0.9, 0.0f32..200.0, any::<bool>(), 0.2f32..5.0).prop_map(|(g, r, dz, sm, dom, e)| NavConfig {
            master_gain: g,
            rotation_gain: r,
            deadzone: [dz; 6],
            smoothing_time_ms: sm,
            dominant_axis: dom,
            response_curve: Curve::Power { exponent: e },
            ..Default::default()
        })
    }

    proptest! {
        #[test]
        fn outputs_finite_bounded(cfg in arb_cfg(), samples in proptest::collection::vec((proptest::array::uniform6(-2000i32..2000), 0u64..300_000), 1..50)) {
            prop_assert!(cfg.validate().is_ok());
            let mut pl = p(cfg);
            let mut t = 0;
            for (raw, dt) in samples {
                t += dt;
                let m = pl.process(raw, t);
                for i in 0..6 {
                    prop_assert!(m.norm[i].is_finite());
                    prop_assert!(m.norm[i].abs() <= MAX_OUT);
                    prop_assert!(m.spnav[i].abs() <= (MAX_OUT * SPNAV_SCALE) as i32);
                }
            }
        }

        #[test]
        fn neutral_in_neutral_out(cfg in arb_cfg()) {
            let mut pl = p(cfg);
            prop_assert!(pl.process([0; 6], 0).is_zero());
        }

        #[test]
        fn sign_preserved_without_invert(v in -350i32..350) {
            let mut pl = p(NavConfig { deadzone: [0.0; 6], ..Default::default() });
            let m = pl.process([v, 0, 0, 0, 0, 0], 0);
            prop_assert_eq!(m.spnav[0].signum(), v.signum());
        }
    }
}
