#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Machine profiles: the shipped profiles load, and validation catches unsafe
//! or inconsistent ones.

use std::path::PathBuf;

use roast_core::Actuator;
use roast_machine::{FieldRole, MachineProfile, Parity, ProfileError, SafeValue};

fn machines_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../machines")
}

fn cubean_text() -> String {
    std::fs::read_to_string(machines_dir().join("skywalker-cubean-plus.toml")).unwrap()
}

fn cubean() -> MachineProfile {
    MachineProfile::from_toml_str(&cubean_text()).unwrap()
}

/// Validates and returns the fields of all issues.
fn issues(profile: &MachineProfile) -> Vec<String> {
    match profile.validate() {
        Ok(()) => Vec::new(),
        Err(e) => e.issues.into_iter().map(|i| i.field).collect(),
    }
}

#[test]
fn every_shipped_profile_loads() {
    let profiles = MachineProfile::load_dir(machines_dir()).unwrap();
    assert!(profiles.iter().any(|p| p.id == "skywalker-cubean-plus"));
}

#[test]
fn cubean_plus_profile_matches_the_research() {
    let p = cubean();
    let serial = p.serial().unwrap();
    assert_eq!(serial.port_hint, "/dev/cu.AKROASTER");
    assert_eq!(serial.baud_rate, 115_200);
    assert_eq!(
        (serial.data_bits, serial.parity, serial.stop_bits),
        (8, Parity::None, 1)
    );

    let tc4 = p.tc4().unwrap();
    assert_eq!(tc4.line_ending, "\n");
    let init: Vec<_> = tc4.init.iter().map(|s| s.send.as_str()).collect();
    assert_eq!(init, ["CHAN;2100", "UNITS;C", "FILT;95,95,95,95"]);
    // Verified on hardware: the machine never acknowledges setup commands.
    assert!(tc4.init.iter().all(|s| s.expect_prefix.is_none()));
    assert_eq!(tc4.read.command, "READ");
    assert_eq!(
        tc4.read.fields,
        [
            FieldRole::Ambient,
            FieldRole::Et,
            FieldRole::Bt,
            FieldRole::Actuator(Actuator::Burner),
            FieldRole::Actuator(Actuator::Air),
            FieldRole::Setpoint,
        ]
    );
    assert!(!tc4.et_present);

    let burner = p.actuator(Actuator::Burner).unwrap();
    assert_eq!(
        (burner.command.as_str(), burner.min, burner.max),
        ("OT1,{}", 0.0, 100.0)
    );
    assert!(burner.reported);
    let drum = p.actuator(Actuator::Drum).unwrap();
    assert_eq!(
        (drum.command.as_str(), drum.min, drum.max),
        ("OT3,{}", 80.0, 100.0)
    );
    assert!(!drum.reported);
    assert_eq!(p.actuator(Actuator::Filter).unwrap().command, "OT4,{}");
    let cooling = p.switch(Actuator::Cooling).unwrap();
    assert_eq!(
        (cooling.on.as_str(), cooling.off.as_str()),
        ("CLDN,ON", "CLDN,OF")
    );

    assert_eq!(p.safe_state[&Actuator::Burner], SafeValue::Value(0.0));
    assert_eq!(p.safe_state[&Actuator::Air], SafeValue::Value(100.0));
    assert_eq!(p.safe_state[&Actuator::Drum], SafeValue::Value(100.0));
    assert!((p.sampling_interval_s - 1.5).abs() < 1e-9);
    assert!((p.limits.max_bt_c - 240.0).abs() < 1e-9);
    assert!(p.link_loss.tested);
    assert!(p.capabilities.reports_actuators);
    assert_eq!(burner.render(42), "OT1,42");
}

#[test]
fn toml_round_trip() {
    let p = cubean();
    let text = p.to_toml_string().unwrap();
    assert!(
        text.contains("min = 80"),
        "whole numbers stay integers:\n{text}"
    );
    let back = MachineProfile::from_toml_str(&text).unwrap();
    assert_eq!(p, back);
}

#[test]
fn burner_safe_value_must_be_its_minimum() {
    let mut p = cubean();
    p.safe_state
        .insert(Actuator::Burner, SafeValue::Value(10.0));
    assert_eq!(issues(&p), ["safe_state.burner"]);
    p.safe_state.insert(Actuator::Burner, SafeValue::Keep);
    assert_eq!(issues(&p), ["safe_state.burner"]);
    p.safe_state.remove(&Actuator::Burner);
    assert_eq!(issues(&p), ["safe_state.burner"]);
}

#[test]
fn air_must_stay_on() {
    let mut p = cubean();
    p.safe_state.insert(Actuator::Air, SafeValue::Value(0.0));
    assert_eq!(issues(&p), ["safe_state.air"]);
    p.safe_state.remove(&Actuator::Air);
    assert_eq!(issues(&p), ["safe_state.air"]);
}

#[test]
fn drum_is_never_turned_off() {
    let mut p = cubean();
    // The Cubean+ drum cannot go below 80 %, so "keep" is safe.
    p.safe_state.insert(Actuator::Drum, SafeValue::Keep);
    assert!(issues(&p).is_empty());
    p.safe_state.remove(&Actuator::Drum);
    assert!(issues(&p).is_empty());

    // A drum that can be stopped needs an explicit running speed.
    let drum = p
        .actuators
        .iter_mut()
        .find(|a| a.actuator == Actuator::Drum)
        .unwrap();
    drum.min = 0.0;
    assert_eq!(issues(&p), ["safe_state.drum"]);
    p.safe_state.insert(Actuator::Drum, SafeValue::Value(0.0));
    assert_eq!(issues(&p), ["safe_state.drum"]);
    p.safe_state.insert(Actuator::Drum, SafeValue::Value(60.0));
    assert!(issues(&p).is_empty());
}

#[test]
fn safe_state_actuators_must_exist_and_fit() {
    let mut p = cubean();
    p.switches.clear();
    p.safe_state.insert(Actuator::Cooling, SafeValue::On);
    assert_eq!(issues(&p), ["safe_state.cooling"]);

    let mut p = cubean();
    p.safe_state
        .insert(Actuator::Cooling, SafeValue::Value(100.0));
    assert_eq!(issues(&p), ["safe_state.cooling"]);
    p.safe_state.insert(Actuator::Cooling, SafeValue::On);
    assert!(issues(&p).is_empty());

    let mut p = cubean();
    p.safe_state.insert(Actuator::Filter, SafeValue::On);
    assert_eq!(issues(&p), ["safe_state.filter"]);
    p.safe_state
        .insert(Actuator::Filter, SafeValue::Value(150.0));
    assert_eq!(issues(&p), ["safe_state.filter"]);
}

#[test]
fn read_layout_is_checked() {
    let mut p = cubean();
    let roast_machine::ProtocolConfig::Tc4(tc4) = &mut p.protocol;
    tc4.read.fields.push(FieldRole::Bt);
    tc4.read.fields.push(FieldRole::Setpoint);
    assert_eq!(issues(&p), ["protocol.read.fields", "protocol.read.fields"]);

    let mut p = cubean();
    let roast_machine::ProtocolConfig::Tc4(tc4) = &mut p.protocol;
    tc4.et_present = true;
    tc4.read.fields.retain(|f| *f != FieldRole::Et);
    assert_eq!(issues(&p), ["protocol.et_present"]);

    let mut p = cubean();
    let roast_machine::ProtocolConfig::Tc4(tc4) = &mut p.protocol;
    tc4.read.fields.push(FieldRole::Actuator(Actuator::Drum));
    assert_eq!(issues(&p), ["protocol.read.fields"]);
}

#[test]
fn reported_actuators_must_be_read() {
    let mut p = cubean();
    p.actuators
        .iter_mut()
        .find(|a| a.actuator == Actuator::Filter)
        .unwrap()
        .reported = true;
    assert_eq!(issues(&p), ["actuators.filter.reported"]);

    let mut p = cubean();
    p.capabilities.reports_actuators = false;
    assert_eq!(issues(&p), ["capabilities.reports_actuators"]);
}

#[test]
fn actuator_definitions_are_checked() {
    let mut p = cubean();
    let filter = p
        .actuators
        .iter_mut()
        .find(|a| a.actuator == Actuator::Filter)
        .unwrap();
    filter.command = "OT4".into();
    filter.min = 100.0;
    assert_eq!(issues(&p), ["actuators.filter.command", "actuators.filter"]);

    let mut p = cubean();
    let mut dup = p.actuators[0].clone();
    dup.reported = false;
    p.actuators.push(dup);
    assert!(issues(&p).contains(&"actuators.burner".to_string()));

    let mut p = cubean();
    p.switches[0].actuator = Actuator::Burner;
    assert!(issues(&p).contains(&"switches.burner".to_string()));
}

#[test]
fn numbers_are_in_sane_ranges() {
    let mut p = cubean();
    p.sampling_interval_s = 0.0;
    p.limits.max_bt_c = 400.0;
    p.limits.ai_burner_max_step = 50.0;
    p.limits.ai_air_min_interval_s = 0.0;
    let found = issues(&p);
    for field in [
        "sampling_interval_s",
        "limits.max_bt_c",
        "limits.ai_burner_max_step",
        "limits.ai_air_min_interval_s",
    ] {
        assert!(found.iter().any(|f| f == field), "{field} not in {found:?}");
    }

    let mut p = cubean();
    p.sampling_interval_s = 0.5;
    assert_eq!(issues(&p), ["transport.timeout_ms"]);
}

#[test]
fn identity_is_checked() {
    let mut p = cubean();
    p.id = "Cubean Plus".into();
    p.vendor = " ".into();
    p.link_loss.link_lost = String::new();
    assert_eq!(issues(&p), ["id", "vendor", "link_loss.link_lost"]);
}

#[test]
fn errors_name_the_field() {
    let text = cubean_text().replace("burner = 0", "burner = 5");
    let err = MachineProfile::from_toml_str(&text).unwrap_err();
    let message = err.to_string();
    assert!(matches!(err, ProfileError::Invalid { .. }));
    assert!(message.contains("safe_state.burner"), "{message}");
    assert!(message.contains("minimum 0"), "{message}");
}

#[test]
fn schema_errors_are_parse_errors() {
    for (from, to) in [
        ("\"actuator:burner\"", "\"actuator:heater\""),
        ("\"setpoint\"", "\"sv\""),
        ("max_bt_c = 240", "max_bt_c = 240\nmax_et_c = 300"),
        ("burner = 0", "burner = \"low\""),
        ("kind = \"tc4\"", "kind = \"modbus\""),
    ] {
        let text = cubean_text().replace(from, to);
        assert_ne!(text, cubean_text(), "replacement {from} did not apply");
        let err = MachineProfile::from_toml_str(&text).unwrap_err();
        assert!(
            matches!(err, ProfileError::Toml { .. }),
            "{from} → {to}: {err}"
        );
    }
}

#[test]
fn keep_on_and_off_parse() {
    let text = cubean_text().replace("drum = 100", "drum = \"keep\"\ncooling = \"off\"");
    let p = MachineProfile::from_toml_str(&text).unwrap();
    assert_eq!(p.safe_state[&Actuator::Drum], SafeValue::Keep);
    assert_eq!(p.safe_state[&Actuator::Cooling], SafeValue::Off);
}

#[test]
fn load_dir_checks_file_names_and_skips_other_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "not a profile").unwrap();
    std::fs::write(dir.path().join("skywalker-cubean-plus.toml"), cubean_text()).unwrap();
    let profiles = MachineProfile::load_dir(dir.path()).unwrap();
    assert_eq!(profiles.len(), 1);

    std::fs::write(dir.path().join("other.toml"), cubean_text()).unwrap();
    let err = MachineProfile::load_dir(dir.path()).unwrap_err();
    assert!(matches!(err, ProfileError::IdMismatch { .. }), "{err}");

    let err = MachineProfile::load(dir.path().join("missing.toml")).unwrap_err();
    assert!(matches!(err, ProfileError::Io { .. }));
}
