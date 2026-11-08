//! Reads and writes Artisan `.alog` roast logs ([ADR 0008]).
//!
//! An `.alog` file is a Python literal (a dict) written by Artisan. [`py`]
//! parses and prints that literal format; [`read_alog`] maps it to a
//! [`roast_core::Roast`] and [`write_alog`] maps a roast back, so roasts made
//! with RoastMCP open in Artisan and upload to services that accept `.alog`.
//!
//! [ADR 0008]: https://github.com/huulanka/roastmcp/blob/main/docs/adr/0008-open-roast-files-and-alog.md

mod error;
mod format;
pub mod py;
mod read;
mod text;
mod write;

pub use error::AlogError;
pub use py::{ParseError, PyValue, parse};
pub use read::{from_py_value, read_alog, read_alog_file};
pub use write::{to_py_value, write_alog, write_alog_with_offset};
