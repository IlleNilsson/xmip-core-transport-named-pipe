//! What a Location of this technology is configured with: its declaration,
//! which `Configured` reads, built with the crate's `declaration` feature
//! alone and the code behind the default (ADR-0064, amendments 2026-10-22
//! and 2026-11-26), for the library the surfaces load to carry (open
//! problem 48). It is the same on every operating system, the Windows pipe
//! and the Unix FIFO alike.

use xcore::settings::{Applies, Settings};

/// Every setting a Location takes beyond its address, the pipe: a bare
/// name, or a path as it is. A Receive Location makes it; a Send
/// Location's target names the pipe it opens.
pub const SETTINGS: &Settings = &Settings {
    technology: env!("CARGO_PKG_NAME"),
    settings: &[transport::configured::timeout(
        Applies::Receive,
        None,
        "How long a made pipe waits for a writer; unbounded when left out.",
    )],
};

/// The schemes a Location's address is entered with to choose this technology.
pub const SCHEMES: &[&str] = &["pipe"];
