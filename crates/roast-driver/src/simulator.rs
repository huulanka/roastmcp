//! A simulated drum roaster for trying RoastMCP without hardware.
//!
//! A deliberately simple two-node heat model (drum and beans) with moisture
//! evaporation, a mild exotherm around first crack and a lagging probe. It is
//! tuned to give plausible curves for a 200 g batch, not to predict a real
//! machine; the digital twin (roast-twin) is the place for that.

use std::time::Instant;

use roast_core::{Actuator, ActuatorCommand, ActuatorValues, EventKind};
use roast_machine::MachineProfile;

use crate::{DriverError, Reading, RoasterDriver};

const AMBIENT_C: f64 = 22.0;
const HEATER_W: f64 = 1700.0;
const DRUM_J_PER_K: f64 = 1500.0;
const LOSS_W_PER_K: f64 = 4.0;
const AIR_LOSS_W_PER_K: f64 = 6.0;
const BEAN_TRANSFER_W_PER_K: f64 = 2.6;
const BATCH_KG: f64 = 0.2;
const BEAN_J_PER_KG_K: f64 = 1600.0;
const LATENT_J_PER_KG: f64 = 2.26e6;
const PROBE_TAU_S: f64 = 25.0;
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
        let heater = HEATER_W * self.get(Actuator::Burner) / 100.0;
        let loss_k = LOSS_W_PER_K + AIR_LOSS_W_PER_K * self.get(Actuator::Air) / 100.0;
        let loss = loss_k * (self.drum_c - AMBIENT_C);
        let to_beans = if self.beans {
            BEAN_TRANSFER_W_PER_K * (self.drum_c - self.bean_c)
        } else {
            0.0
        };
        self.drum_c += (heater - loss - to_beans) * dt / DRUM_J_PER_K;

        if self.beans {
            let evap = if self.bean_c > 95.0 && self.moisture_kg > 0.0 {
                (self.bean_c - 95.0).min(60.0) * 1.5
            } else {
                0.0
            };
            let exo = if (190.0..225.0).contains(&self.bean_c) {
                40.0
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
            0.85 * self.bean_c + 0.15 * self.drum_c
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

    #[test]
    fn a_typical_roast_has_plausible_milestones() {
        let mut m = SimulatedMachine::new(1.0);
        m.set(Actuator::Burner, 60.0);
        m.set(Actuator::Air, 30.0);
        m.advance(900.0);
        let charge_temp = m.probe();
        assert!(
            (180.0..230.0).contains(&charge_temp),
            "preheat {charge_temp}"
        );

        m.on_event(EventKind::Charge);
        m.set(Actuator::Burner, 80.0);
        let probes: Vec<f64> = (0..=900)
            .map(|_| {
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
        let tp = (tp_i as f64, tp_c);
        let after = |c: f64| {
            probes
                .iter()
                .skip(tp_i)
                .position(|&p| p >= c)
                .map(|i| (i + tp_i) as f64)
        };
        let dry_end = after(150.0);
        let fc = after(196.0);
        assert!((40.0..120.0).contains(&tp.0), "TP at {:?}", tp);
        assert!((70.0..120.0).contains(&tp.1), "TP at {:?}", tp);
        let dry_end = dry_end.unwrap();
        assert!((150.0..360.0).contains(&dry_end), "DE at {dry_end}");
        let fc = fc.unwrap();
        assert!((420.0..660.0).contains(&fc), "FC at {fc}");
    }
}
