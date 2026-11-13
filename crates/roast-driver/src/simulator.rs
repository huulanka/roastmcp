//! A simulated drum roaster for trying RoastMCP without hardware.
//!
//! A two-node heat model (drum and beans) with a lagging probe, moisture
//! evaporation and a mild exotherm around first crack. Its constants are
//! fitted to the Skywalker Cubean+ (`docs/research/simulator-calibration.md`):
//! to four real roasts of about 200 g from CHARGE to DROP (the bean probe
//! within 4 °C RMS) and to the empty drum (holds 200 °C at 38 % burner and 150 °C at
//! 21 %, air 20 %; rises about 25 °C/min at 90 %). So a whole roast can be
//! practised, and automatic control tried, with the real machine's sluggishness.
//! It is still a simulator, not a prediction; the digital twin (roast-twin)
//! is the place for that.

use std::time::Instant;

use roast_core::{Actuator, ActuatorCommand, ActuatorValues, EventKind};
use roast_machine::MachineProfile;

use crate::{DriverError, Reading, RoasterDriver};

const AMBIENT_C: f64 = 22.0;
const HEATER_W: f64 = 1700.0;
/// Heat capacity of the drum and the air in it.
const DRUM_J_PER_K: f64 = 3431.0;
/// Losses grow faster than the temperature: `LOSS_W * ΔT^LOSS_EXPONENT`.
const LOSS_W: f64 = 0.047_66;
const LOSS_EXPONENT: f64 = 1.8;
/// How much the air flow adds to the losses (at 100 % air: this fraction more).
const AIR_LOSS: f64 = 1.04;
/// Heat flow from the drum to the beans, and how much the air flow adds to it.
const BEAN_TRANSFER_W_PER_K: f64 = 6.967;
const AIR_TRANSFER: f64 = 1.602;
const BATCH_KG: f64 = 0.2;
/// Effective heat capacity of the batch as the probe sees it warm up (well
/// above the beans' own: it stands for everything that warms with them).
const BEAN_J_PER_KG_K: f64 = 6493.0;
const LATENT_J_PER_KG: f64 = 2.26e6;
const EVAPORATION_W_PER_K: f64 = 2.429;
const EXOTHERM_W: f64 = 13.67;
/// The probe sits in the beans: this share of its reading is theirs.
const PROBE_BEAN_SHARE: f64 = 0.9396;
const PROBE_TAU_S: f64 = 48.04;
const STEP_S: f64 = 0.1;

/// The profile the simulator presents: the Cubean+ actuator set without a
/// port, named as a simulator.
#[must_use]
pub fn simulator_profile(base: &MachineProfile) -> MachineProfile {
    let mut p = base.clone();
    p.id = "simulator".into();
    p.name = format!("Simulator ({})", base.name);
    p.vendor = "RoastMCP".into();
    p.model = "Simulator".into();
    p.link_loss.tested = true;
    p.link_loss.link_lost = "Simulator: no real machine is affected.".into();
    p.link_loss.software_killed = "Simulator: no real machine is affected.".into();
    p
}

/// The simulated machine.
#[derive(Debug, Clone)]
pub struct SimulatedMachine {
    speed: f64,
    last: Option<Instant>,
    drum_c: f64,
    bean_c: f64,
    probe_c: f64,
    moisture_kg: f64,
    beans: bool,
    actuators: ActuatorValues,
    noise: u64,
}

impl SimulatedMachine {
    /// A cold machine; `speed` > 1 runs the physics faster than real time.
    #[must_use]
    pub fn new(speed: f64) -> Self {
        SimulatedMachine {
            speed: if speed.is_finite() && speed > 0.0 {
                speed
            } else {
                1.0
            },
            last: None,
            drum_c: AMBIENT_C,
            bean_c: AMBIENT_C,
            probe_c: AMBIENT_C,
            moisture_kg: 0.0,
            beans: false,
            actuators: ActuatorValues::from([
                (Actuator::Burner, 0.0),
                (Actuator::Air, 30.0),
                (Actuator::Drum, 100.0),
                (Actuator::Cooling, 0.0),
                (Actuator::Filter, 0.0),
            ]),
            noise: 0x2545_f491_4f6c_dd1d,
        }
    }

    /// Advances the model by `seconds` of machine time.
    pub fn advance(&mut self, seconds: f64) {
        let steps = (seconds / STEP_S).round().max(0.0) as u64;
        for _ in 0..steps {
            self.step(STEP_S);
        }
    }

    /// Current probe temperature without noise, °C.
    #[must_use]
    pub fn probe(&self) -> f64 {
        self.probe_c
    }

    /// Sets an actuator directly (tests and the twin).
    pub fn set(&mut self, actuator: Actuator, value: f64) {
        self.actuators.insert(actuator, value);
    }

    fn get(&self, a: Actuator) -> f64 {
        self.actuators.get(&a).copied().unwrap_or(0.0)
    }

    fn step(&mut self, dt: f64) {
        let air = self.get(Actuator::Air) / 100.0;
        let heater = HEATER_W * self.get(Actuator::Burner) / 100.0;
        let loss = LOSS_W
            * (1.0 + AIR_LOSS * air)
            * (self.drum_c - AMBIENT_C).max(0.0).powf(LOSS_EXPONENT);
        let to_beans = if self.beans {
            BEAN_TRANSFER_W_PER_K * (1.0 + AIR_TRANSFER * air) * (self.drum_c - self.bean_c)
        } else {
            0.0
        };
        self.drum_c += (heater - loss - to_beans) * dt / DRUM_J_PER_K;

        if self.beans {
            let evap = if self.bean_c > 95.0 && self.moisture_kg > 0.0 {
                (self.bean_c - 95.0).min(60.0) * EVAPORATION_W_PER_K
            } else {
                0.0
            };
            let exo = if (190.0..225.0).contains(&self.bean_c) {
                EXOTHERM_W
            } else {
                0.0
            };
            let cooling = if self.get(Actuator::Cooling) > 0.0 {
                30.0 * (self.bean_c - AMBIENT_C)
            } else {
                0.0
            };
            self.moisture_kg = (self.moisture_kg - evap * dt / LATENT_J_PER_KG).max(0.0);
            let heat_cap = BATCH_KG * BEAN_J_PER_KG_K;
            self.bean_c += (to_beans - evap + exo - cooling) * dt / heat_cap;
        }

        let target = if self.beans {
            PROBE_BEAN_SHARE * self.bean_c + (1.0 - PROBE_BEAN_SHARE) * self.drum_c
        } else {
            self.drum_c
        };
        self.probe_c += (target - self.probe_c) * dt / PROBE_TAU_S;
    }

    fn jitter(&mut self) -> f64 {
        // xorshift64: deterministic, no dependency.
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 7;
        self.noise ^= self.noise << 17;
        #[allow(clippy::cast_precision_loss)]
        let unit = (self.noise % 1000) as f64 / 1000.0;
        (unit - 0.5) * 0.2
    }
}

impl RoasterDriver for SimulatedMachine {
    fn connect(&mut self) -> Result<Vec<String>, DriverError> {
        self.last = Some(Instant::now());
        Ok(Vec::new())
    }

    fn read(&mut self) -> Result<Reading, DriverError> {
        let now = Instant::now();
        if let Some(last) = self.last {
            self.advance((now - last).as_secs_f64() * self.speed);
        }
        self.last = Some(now);
        let bt = self.probe_c + self.jitter();
        Ok(Reading {
            ambient_c: Some(AMBIENT_C),
            bt: Some((bt * 10.0).round() / 10.0),
            et: None,
            actuators: ActuatorValues::from([
                (Actuator::Burner, self.get(Actuator::Burner)),
                (Actuator::Air, self.get(Actuator::Air)),
            ]),
        })
    }

    fn apply(&mut self, cmd: &ActuatorCommand) -> Result<(), DriverError> {
        self.actuators.insert(cmd.actuator, cmd.value);
        Ok(())
    }

    fn on_event(&mut self, kind: EventKind) {
        match kind {
            EventKind::Charge if !self.beans => {
                self.beans = true;
                self.bean_c = AMBIENT_C;
                self.moisture_kg = BATCH_KG * 0.10;
            }
            EventKind::Drop => self.beans = false,
            _ => {}
        }
    }

    fn close(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Heats the empty drum and lets it settle near 200 °C at air 20 %.
    fn preheated() -> SimulatedMachine {
        let mut m = SimulatedMachine::new(1.0);
        m.set(Actuator::Air, 20.0);
        m.set(Actuator::Burner, 90.0);
        while m.probe() < 192.0 {
            m.advance(1.0);
        }
        m.set(Actuator::Burner, 38.0);
        m.advance(600.0);
        m
    }

    #[test]
    fn the_empty_drum_heats_and_holds_like_the_cubean() {
        let mut m = SimulatedMachine::new(1.0);
        m.set(Actuator::Air, 20.0);
        m.set(Actuator::Burner, 90.0);
        m.advance(180.0);
        let (from, at) = (m.probe(), 180.0);
        m.advance(60.0);
        let rise = m.probe() - from;
        assert!(
            (18.0..30.0).contains(&rise),
            "{rise} °C/min from {from} °C after {at} s"
        );

        // 38 % holds about 200 °C.
        let m = preheated();
        assert!((194.0..206.0).contains(&m.probe()), "holds {}", m.probe());
    }

    #[test]
    fn a_roast_with_a_real_recipe_has_the_real_milestones() {
        let mut m = preheated();
        m.on_event(EventKind::Charge);
        // The recipe of a real 200 g roast (seconds after CHARGE, burner, air).
        let recipe = [
            (0.0, 29.0, 20.0),
            (50.0, 85.0, 20.0),
            (150.0, 90.0, 20.0),
            (260.0, 85.0, 20.0),
            (290.0, 85.0, 30.0),
            (320.0, 80.0, 30.0),
            (410.0, 75.0, 30.0),
            (445.0, 70.0, 30.0),
            (458.0, 70.0, 35.0),
            (485.0, 60.0, 35.0),
            (508.0, 50.0, 35.0),
            (538.0, 45.0, 35.0),
            (551.0, 40.0, 35.0),
            (566.0, 40.0, 40.0),
        ];
        let probes: Vec<f64> = (0..=660)
            .map(|t| {
                for &(at, burner, air) in &recipe {
                    if (at - f64::from(t)).abs() < 0.5 {
                        m.set(Actuator::Burner, burner);
                        m.set(Actuator::Air, air);
                    }
                }
                m.advance(1.0);
                m.probe()
            })
            .collect();
        let (tp_i, tp_c) = probes
            .iter()
            .copied()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        let after = |c: f64| {
            probes
                .iter()
                .skip(tp_i)
                .position(|&p| p >= c)
                .map(|i| (i + tp_i) as f64)
        };
        // The real roast: TP 94 °C at 1:13, 160 °C at 5:21, 191 °C at 8:28,
        // 203 °C at 10:30.
        assert!((60.0..100.0).contains(&(tp_i as f64)), "TP at {tp_i} s");
        assert!((88.0..106.0).contains(&tp_c), "TP at {tp_c} °C");
        let dry = after(160.0).unwrap();
        assert!((280.0..360.0).contains(&dry), "160 °C at {dry} s");
        let fc = after(191.0).unwrap();
        assert!((470.0..550.0).contains(&fc), "191 °C at {fc} s");
        let end = after(203.0).unwrap();
        assert!((580.0..670.0).contains(&end), "203 °C at {end} s");
    }
}
