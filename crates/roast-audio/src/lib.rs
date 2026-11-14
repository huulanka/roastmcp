//! Microphone capture and first-crack detection (ADR 0021).
//!
//! The pipeline, pure and testable without a microphone:
//! samples → [`spectrum::Analyzer`] (every 10 ms the spectral flux and level
//! of the crack band 0.4–4 kHz and the high band 6–16 kHz, and a spectrogram
//! column) → [`pops::PopFinder`] per band (a short rise far above the band's
//! recent noise that falls back quickly) → [`crack::CrackLogic`] (the rate of
//! pops plus common sense: first crack only after CHARGE and dry end and
//! inside the temperature window). [`listener::Listener`] ties the first two
//! together; [`synth::Synth`] makes roaster sound with pops for tests and the
//! simulator; [`offline::analyze`] plays a recorded file through it all.

pub mod crack;
pub mod file;
pub mod listener;
pub mod offline;
pub mod pops;
pub mod spectrum;
pub mod synth;

pub use crack::{CrackLogic, HeardCrack, RoastContext};
pub use listener::{COLUMNS_PER_S, Heard, Listener};
pub use pops::{Band, Pop, Sensitivity};
pub use spectrum::COLUMN_BANDS;
pub use synth::Synth;
