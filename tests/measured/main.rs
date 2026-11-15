//! The module's timing program (`doc/governance/rust-style.md` section
//! 5c): the named pipe held to a time. A plain `cargo test` leaves it out;
//! the landing runs it after every other test of the module, alone, one
//! test at a time (`cargo test --test measured --all-features --
//! --test-threads=1`), so what is measured is Xmip's work and not its
//! neighbors':
//!
//! - a pipe nobody writes to gives up soon after its timeout, the fastest
//!   of five (`xcore::latency::at_once`).

use std::time::Duration;

use transport::Configured;
use xcore::latency::at_once;
use xmip_core_transport_named_pipe::NamedPipeTransport;

fn unique(name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!("xmip-pipe-measured-{name}-{}-{nanos}", std::process::id())
}

#[test]
fn a_pipe_nobody_writes_to_gives_up_soon_after_its_timeout() {
    // The defect this asserts is the one that hung the Essentials' gate: a
    // far end whose near end never came waited for good (2026-09-21).
    let far_end =
        NamedPipeTransport::new(&unique("unanswered")).timing_out_after(Duration::from_millis(200));
    let listener = far_end.bind().expect("the pipe is made");
    // A wait that outlives its timeout does so in every one of five, a
    // give-up a busy machine preempted in some only. It is a timeout, not a
    // wake, so no plain wake is measured beside it. Until 2026-11-15 one
    // give-up was timed once.
    let (refused, waited, _) = at_once("a pipe nobody writes to, given up", || {
        far_end.accept_one(&listener).map(drop)
    });
    assert!(refused.is_err(), "nothing was written, so nothing arrived");
    assert!(
        waited < Duration::from_millis(400),
        "gave up after {waited:?}, the fastest of five, which is not a timeout"
    );
}
