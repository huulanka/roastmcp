//! Facts about the `.alog` layout shared by the reader and the writer. They
//! were learned from real files written by Artisan 4.0.2, 4.2.0 and 5.0.0; see
//! the crate documentation.

use roast_core::{Actuator, EventKind};

/// The event kinds stored in `timeindex`, in slot order. Slot 0 (CHARGE) uses
/// `-1` for "not set", every other slot uses `0`.
pub(crate) const TIMEINDEX_KINDS: [EventKind; 8] = [
    EventKind::Charge,
    EventKind::DryEnd,
    EventKind::FirstCrackStart,
    EventKind::FirstCrackEnd,
    EventKind::SecondCrackStart,
    EventKind::SecondCrackEnd,
    EventKind::Drop,
    EventKind::CoolEnd,
];

/// Artisan's default special event types. Types 0 to 3 index into this list;
/// type 4 is an untyped event.
pub(crate) const DEFAULT_ETYPES: [&str; 5] = ["Air", "Drum", "Damper", "Burner", "--"];

/// `specialeventstype` of an untyped event.
pub(crate) const UNTYPED_EVENT: i64 = 4;

/// Prefix that marks a [`roast_core::Note`] as coming from (and going back to)
/// Artisan's `cuppingnotes` rather than `roastingnotes`.
pub const CUPPING_NOTE_PREFIX: &str = "Cupping notes: ";

/// Prefixes of `computed` keys for the events, as Artisan writes them, with
/// the matching event kind.
pub(crate) const COMPUTED_EVENT_PREFIXES: [(&str, EventKind); 6] = [
    ("DRY", EventKind::DryEnd),
    ("FCs", EventKind::FirstCrackStart),
    ("FCe", EventKind::FirstCrackEnd),
    ("SCs", EventKind::SecondCrackStart),
    ("DROP", EventKind::Drop),
    ("TP", EventKind::TurningPoint),
];

/// The actuator role a special event type or extra channel name stands for.
/// Matching is case-insensitive on the role's name (`Air`, `Drum`, `Burner`,
/// `Cooling`, `Filter`); anything else, such as `Damper`, is not an actuator.
pub(crate) fn actuator_for_name(name: &str) -> Option<Actuator> {
    let name = name.trim();
    Actuator::ALL
        .into_iter()
        .find(|a| a.as_str().eq_ignore_ascii_case(name))
}

/// The `etypes` index Artisan's default event types use for an actuator.
pub(crate) fn etype_index(actuator: Actuator) -> Option<i64> {
    match actuator {
        Actuator::Air => Some(0),
        Actuator::Drum => Some(1),
        Actuator::Burner => Some(3),
        Actuator::Cooling | Actuator::Filter => None,
    }
}

/// Decodes a `specialeventsvalue` into percent: `(v - 1) * 10`. Real files
/// also use `0.0` for 0 % (labelled `0%` or `Q0`), so the result is clamped at
/// zero. Rounded to six decimals to drop floating-point noise.
pub(crate) fn event_value_to_percent(v: f64) -> f64 {
    round6(((v - 1.0) * 10.0).max(0.0))
}

/// Encodes percent as a `specialeventsvalue`, the inverse of
/// [`event_value_to_percent`].
pub(crate) fn percent_to_event_value(pct: f64) -> f64 {
    pct / 10.0 + 1.0
}

pub(crate) fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// Formats a percentage compactly: `80%`, `21.5%`.
pub(crate) fn format_percent(pct: f64) -> String {
    let rounded = (pct * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}%")
    } else {
        format!("{rounded:.1}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_values() {
        assert_eq!(event_value_to_percent(3.5), 25.0);
        assert_eq!(event_value_to_percent(9.0), 80.0);
        assert_eq!(event_value_to_percent(11.0), 100.0);
        assert_eq!(event_value_to_percent(1.0), 0.0);
        assert_eq!(event_value_to_percent(0.0), 0.0);
        for pct in [0.0, 21.666667, 25.0, 85.0, 100.0] {
            assert_eq!(event_value_to_percent(percent_to_event_value(pct)), pct);
        }
    }

    #[test]
    fn names() {
        assert_eq!(actuator_for_name("Burner"), Some(Actuator::Burner));
        assert_eq!(actuator_for_name(" air "), Some(Actuator::Air));
        assert_eq!(actuator_for_name("DRUM"), Some(Actuator::Drum));
        assert_eq!(actuator_for_name("Damper"), None);
        assert_eq!(actuator_for_name("--"), None);
        assert_eq!(format_percent(80.0), "80%");
        assert_eq!(format_percent(21.666), "21.7%");
    }
}
