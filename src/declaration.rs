//! This technology's declaration: the transport guide's "The declaration",
//! `module/core/capability/transport/doc/adding-a-transport.md`. It is the
//! same on every operating system, the Windows pipe and the Unix FIFO alike.

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

/// This technology's schemes, as [`transport::Configured::SCHEMES`] defines
/// them.
pub const SCHEMES: &[&str] = &["pipe"];
