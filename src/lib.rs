#![forbid(unsafe_code)]

//! Streams that arrive over a named pipe. One connection is one Stream.
//!
//! A named pipe is the oldest way two processes on one box hand each
//! other bytes without a port: the receiver makes the pipe, the sender
//! opens it by name, writes and closes, and the kernel carries the bytes
//! with no network stack between them. On Windows it is the object under
//! `\\.\pipe\`, which the `BizTalk` and MSMQ era used between services on
//! one machine; on Unix it is a FIFO, a special file `mkfifo` makes. A
//! Receive Location makes its pipe and takes one connection to its end; a
//! Send Location opens the pipe, writes the Stream and closes, which is how
//! the receiver knows the Stream is whole. The object on each system is
//! `net::pipe`'s, the estate's one pipe, which the runtime's local endpoint
//! makes its pipe through too: one client is one connection, and an accept
//! returns only once a client has connected. Until 2026-10-21 this crate
//! made its pipe through the `interprocess` crate.
//!
//! Proven where the operating system has the object, which is every system
//! the estate builds on: a Windows box proves the Windows pipe, a Unix box
//! the FIFO. A pipe is not an artifact anyone claims — one writer at a time
//! is what the object already is — so [`Transport::claims`] answers `None`.
//!
//! The origin URI is the pipe's name: `pipe://./orders` on Windows for
//! `\\.\pipe\orders`, `pipe:///run/xmip/orders` for a FIFO. A send target
//! is a pipe name, an operating-system path, or `pipe://` and either.
//!
//! **Acceptance is at-most-once here** ([`AT_MOST_ONCE`]): the writer's
//! close ends the Stream and a pipe carries no reply, so the writer is gone
//! before the receive cycle ends. The body is the connection, read to its
//! end as the runtime asks.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use context::property::NAMED_PIPE_CLIENT_PROCESS_ID;
use net::Target;
pub use net::pipe::Listener;
use transport::ArrivalIdentity;
use transport::error::{Result, classify};
use transport::held::Held;
use transport::kept::Kept;
use transport::loopback::{FarEnd, Loopback};
use transport::{Arrived, Configured, Directions, Transport};
use xcore::settings::{Applies, Kind, Presence, Read, Setting, Settings};

/// Why an arrival on a named pipe cannot be acknowledged after the receive
/// cycle.
pub const AT_MOST_ONCE: &str = "a named pipe has no reply: the writer's close ends the Stream, \
                                and the writer is gone before the receive cycle ends";

pub struct NamedPipeTransport {
    path: PathBuf,
    timeout: Option<Duration>,
    /// The pipe the first receive makes, and every receive takes from: a
    /// writer that opens it between two receives waits for the next.
    receiving: Kept<Listener>,
}

impl NamedPipeTransport {
    /// The pipe called `name`: a bare name under `\\.\pipe\` on Windows and
    /// in the temporary directory on Unix, or a path as it is.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            path: net::pipe::path_of(name),
            timeout: None,
            receiving: Kept::new(),
        }
    }

    /// The same pipe, waiting at most `timeout` for a writer. Unset, a
    /// listening pipe waits for as long as it takes.
    #[must_use]
    pub const fn timing_out_after(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Where the pipe is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The origin every connection to this pipe shares.
    #[must_use]
    pub fn origin(&self) -> String {
        origin_of(&self.path)
    }

    /// Make the pipe, so a sender has something to open: its owner's, the
    /// system's and the administrators' on Windows, its owner's on Unix. A
    /// FIFO a previous run left behind is used.
    ///
    /// # Errors
    /// Where the name is not permitted or, on Unix, its directory is
    /// missing.
    pub fn bind(&self) -> Result<Listener> {
        Listener::bind(&self.path, &[]).map_err(|e| classify("creating the pipe", &e))
    }

    /// The pipe made, and its path: what a Receive Location keeps.
    fn made(&self) -> Result<(Listener, String)> {
        Ok((self.bind()?, self.path.display().to_string()))
    }

    /// Take one connection from an already-made pipe, once a writer has
    /// connected, within the timeout where one is set. Its body is the
    /// connection, read to its end as the runtime asks — the writer's close
    /// is the end — and the writer's process id travels with it where the
    /// system names it: Windows does, of a pipe's client; a FIFO has no
    /// peer. Acceptance is at-most-once ([`AT_MOST_ONCE`]).
    ///
    /// # Errors
    /// Where no writer came within the timeout, or the pipe could not be
    /// opened.
    pub fn accept_one(&self, listener: &Listener) -> Result<Arrived> {
        let pipe = listener
            .accept(self.timeout)
            .map_err(|e| classify("waiting for a writer", &e))?;
        let client = pipe.peer_process();
        let arrived = Arrived::new(
            self.origin(),
            pipe,
            transport::Acknowledgement::at_most_once(AT_MOST_ONCE),
        );
        Ok(match client {
            Some(client) => arrived.observing(NAMED_PIPE_CLIENT_PROCESS_ID, client.to_string()),
            None => arrived,
        })
    }
}

/// Connect to the pipe at `path` — once an instance is free on Windows,
/// once its reader is there on Unix — write `outgoing`'s body as it is
/// read, never whole in memory, and close, which is what tells the reader
/// the Stream is whole.
///
/// # Errors
/// Where nothing has made the pipe — permanent, as a missing file is — or
/// it could not be written.
pub fn write_once(path: &Path, outgoing: &transport::Outgoing<'_>) -> Result<()> {
    use std::io::Write;
    let mut stream =
        net::pipe::connect(path, None).map_err(|e| classify("connecting to the pipe", &e))?;
    outgoing.write_to(&mut stream)?;
    stream
        .flush()
        .map_err(|e| classify("flushing the pipe", &e))
}

/// The path a target names: `pipe://` and a name or path, or either bare.
#[must_use]
pub fn target_path(target: &str) -> PathBuf {
    let named = Target::under(<NamedPipeTransport as Configured>::SCHEMES, target)
        .map_or(target, |named| named.after_scheme());
    net::pipe::path_of(named)
}

/// `pipe://` and the pipe's name, forward slashes throughout.
#[must_use]
pub fn origin_of(path: &Path) -> String {
    let text = path.display().to_string().replace(char::from(92), "/");
    let text = text
        .strip_prefix("//./pipe/")
        .map_or(text.as_str(), |name| name);
    if text.starts_with('/') {
        format!("pipe://{text}")
    } else {
        format!("pipe://./{text}")
    }
}

impl Transport for NamedPipeTransport {
    fn name(&self) -> &'static str {
        "named-pipe"
    }

    fn directions(&self) -> Directions {
        Directions::BOTH
    }

    fn arrivals(&self) -> transport::Arrivals {
        transport::Arrivals::Unordered("each pipe connection is a Stream of its own")
    }

    fn waiting(&self) -> transport::Waiting {
        transport::Waiting::Waits("the pipe waits for a writer to connect")
    }

    /// Take one connection, on the pipe the first receive made and kept,
    /// read to its end by the runtime. Acceptance is at-most-once here: a
    /// pipe has no reply to defer ([`AT_MOST_ONCE`]).
    fn receive_up_to(&self, _most: usize) -> Result<Vec<Arrived>> {
        let listener = self.receiving.bound(|| self.made())?;
        Ok(vec![self.accept_one(listener)?])
    }

    fn sending(&self) -> transport::Sending {
        transport::Sending::Streamed("the pipe carries the body as it is read")
    }

    /// Open the pipe the target names, write the bytes, close.
    fn send_from(&self, target: &str, outgoing: transport::Outgoing<'_>) -> Result<()> {
        write_once(&target_path(target), &outgoing)
    }
}

impl Configured for NamedPipeTransport {
    /// The address is the pipe: a bare name, or a path as it is. A Receive
    /// Location makes it; a Send Location's target names the pipe it opens.
    const SETTINGS: &'static Settings = &Settings {
        technology: env!("CARGO_PKG_NAME"),
        settings: &[Setting {
            name: "timeout",
            kind: Kind::Duration,
            presence: Presence::Optional,
            meaning: "How long a made pipe waits for a writer; unbounded when left out.",
            applies: Applies::Receive,
        }],
    };

    /// The schemes a Location's address is entered with to choose this technology.
    const SCHEMES: &'static [&'static str] = &["pipe"];

    fn configured(address: &str, settings: &Read) -> Result<Self> {
        let transport = Self::new(address);
        Ok(match settings.optional_duration("timeout") {
            Some(timeout) => transport.timing_out_after(timeout),
            None => transport,
        })
    }
}

impl NamedPipeTransport {
    /// Both ends on this machine: a pipe of this process's own. Each far
    /// end makes a fresh one, so rounds driven at once from several threads
    /// do not read each other's Stream; the address is the pipe's path.
    #[must_use]
    pub fn loopback() -> Self {
        Self::new(&fresh_name()).timing_out_after(transport::LOOPBACK_TIMEOUT)
    }
}

/// A pipe name no other far end of this process has: the pipe is the
/// address, so two rounds at once need two pipes.
fn fresh_name() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(1);
    format!(
        "xmip-loopback-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

impl Loopback for NamedPipeTransport {
    fn arrival_identity(&self) -> ArrivalIdentity {
        if cfg!(windows) {
            ArrivalIdentity::Named(&[NAMED_PIPE_CLIENT_PROCESS_ID])
        } else {
            ArrivalIdentity::Unnamed("a FIFO has no peer: the kernel names no writer")
        }
    }

    /// A made pipe waiting for its one connection.
    fn far_end(&self) -> Result<Box<dyn FarEnd>> {
        let transport = Self::new(&fresh_name());
        let listener = transport.bind()?;
        let address = transport.path().display().to_string();
        Ok(Box::new(Held::new(address, move || {
            transport.accept_one(&listener)?.taken()
        })))
    }

    fn send_to(&self, address: &str, payload: &[u8]) -> Result<()> {
        Self::new(address).send(address, payload)
    }

    /// A pipe is connected to by opening it, not by a TCP connect: open
    /// and close, a Stream of nothing the far end discards, since the
    /// failed send is what the round reports. Until 2026-10-21 a Windows
    /// pipe dropped a connection that wrote nothing and closed before its
    /// reader was waiting, so the poke wrote one byte and a loopback
    /// refused an empty Stream; a pipe made in `net::pipe` keeps it.
    fn unblock(&self, address: &str) {
        drop(write_once(
            &target_path(address),
            &transport::Outgoing::held(&[]),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use transport::payload::edge_payloads;

    fn unique(name: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        format!("xmip-pipe-{name}-{}-{nanos}", std::process::id())
    }

    #[test]
    fn every_receive_takes_from_the_pipe_the_first_made() {
        let receiver = NamedPipeTransport::loopback();
        receiver.receiving.bound(|| receiver.made()).expect("made");
        let address = receiver.receiving.address().expect("address").to_string();
        // Each writer opens the pipe before its receive begins: the pipe
        // made by the first receive is still there. One writer at a time,
        // because a FIFO joins writers that overlap into one Stream.
        for round in 0..5u8 {
            let at = address.clone();
            let writer =
                std::thread::spawn(move || NamedPipeTransport::new(&at).send(&at, &[round]));
            let mut arrived = receiver.receive().expect("received");
            let arrived = arrived.remove(0);
            assert!(!arrived.defers(), "a pipe is at-most-once");
            assert_eq!(arrived.taken().expect("taken").bytes, [round]);
            writer.join().expect("writer").expect("written");
        }
    }

    #[test]
    fn named_pipe_declares_its_settings_and_reads_through_them() {
        use xcore::settings::Given;
        assert_eq!(
            NamedPipeTransport::SETTINGS.problems(),
            Vec::<String>::new()
        );
        let given = [("timeout".to_string(), Given::Text("250ms".to_string()))];
        let built = NamedPipeTransport::open("orders", Applies::Receive, &given).expect("built");
        assert_eq!(built.timeout, Some(Duration::from_millis(250)));
        assert_eq!(built.path(), net::pipe::path_of("orders"));
        let Err(refused) = NamedPipeTransport::open("orders", Applies::Send, &given) else {
            panic!("timeout is a receive setting");
        };
        assert!(
            refused.message.contains("\"timeout\""),
            "{}",
            refused.message
        );
    }

    #[test]
    fn the_loopback_returns_the_edge_payloads_whole() {
        let pair = NamedPipeTransport::loopback();
        for (name, bytes) in edge_payloads() {
            assert_eq!(pair.refuses(&bytes), None, "{name}");
            let arrived = pair
                .round(&bytes)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(arrived.bytes, bytes, "{name}");
            assert!(arrived.origin_uri.starts_with("pipe://"), "{name}");
        }
        assert!(pair.ceiling().is_none());
    }

    #[test]
    fn a_pipe_nobody_writes_to_gives_up_within_its_timeout() {
        // The defect this asserts is the one that hung the Playground's gate:
        // a far end whose near end never came waited for good (2026-09-21).
        let far_end = NamedPipeTransport::new(&unique("unanswered"))
            .timing_out_after(Duration::from_millis(200));
        let listener = far_end.bind().expect("the pipe is made");
        let began = std::time::Instant::now();
        let refused = far_end.accept_one(&listener).map(drop);
        let waited = began.elapsed();
        let error = refused.expect_err("nothing was written, so nothing arrived");
        assert!(error.retryable, "{error}");
        assert!(
            waited < Duration::from_millis(400),
            "gave up after {waited:?}, which is not a timeout"
        );
        assert!(
            waited >= Duration::from_millis(195),
            "gave up after {waited:?}, before it had waited"
        );
    }

    #[test]
    fn a_pipe_nobody_made_cannot_be_written() {
        let path = net::pipe::path_of(&unique("nobody"));
        let error = write_once(&path, &transport::Outgoing::held(b"x")).expect_err("nothing there");
        assert!(!error.retryable, "{error}");
    }

    #[test]
    fn one_writer_is_one_stream_and_no_second() {
        let name = unique("one");
        let far_end = NamedPipeTransport::new(&name).timing_out_after(Duration::from_millis(50));
        let listener = far_end.bind().expect("making the pipe");
        let writer =
            std::thread::spawn(move || NamedPipeTransport::new(&name).send(&name, b"once"));
        // Waits of 50 ms, until the writer, which starts beside it, comes.
        let first = (0..100)
            .find_map(|_| far_end.accept_one(&listener).ok())
            .expect("the writer came")
            .taken()
            .expect("read");
        writer.join().expect("writer").expect("written");
        assert_eq!(first.bytes, b"once");
        let second = far_end.accept_one(&listener).map(drop);
        assert!(second.is_err(), "one writer gave a second Stream");
    }

    #[test]
    fn each_far_end_is_its_own_pipe_and_a_long_stream_arrives_whole() {
        let pair = NamedPipeTransport::loopback();
        let first = pair.far_end().expect("the first pipe");
        let second = pair.far_end().expect("the second pipe");
        assert_ne!(first.address(), second.address());
        let long: Vec<u8> = (0..200_000u32)
            .map(|n| u8::try_from(n % 251).unwrap_or(0))
            .collect();
        let arrived = pair.round(&long).expect("the long one");
        assert_eq!(arrived.bytes, long);
        assert!(arrived.origin_uri.contains("xmip-loopback-"));
    }

    #[test]
    fn the_transport_names_itself_and_goes_both_ways() {
        let pipe = NamedPipeTransport::new("orders");
        assert_eq!(pipe.name(), "named-pipe");
        assert_eq!(pipe.directions(), Directions::BOTH);
        assert!(
            pipe.claims().is_none(),
            "one writer at a time is the object"
        );
        assert_eq!(pipe.path(), net::pipe::path_of("orders"));
    }

    #[test]
    fn an_origin_names_the_pipe_on_either_system() {
        assert_eq!(
            origin_of(Path::new("\\\\.\\pipe\\orders")),
            "pipe://./orders"
        );
        assert_eq!(
            origin_of(Path::new("/run/xmip/orders")),
            "pipe:///run/xmip/orders"
        );
        assert_eq!(
            target_path("pipe:///run/xmip/orders"),
            PathBuf::from("/run/xmip/orders")
        );
        assert_eq!(target_path("pipe://orders"), net::pipe::path_of("orders"));
    }

    #[test]
    fn a_connection_is_one_stream_read_to_its_end() {
        let pair = NamedPipeTransport::loopback();
        let arrived = pair.round(b"over a pipe\0\xff").expect("round");
        assert_eq!(arrived.bytes, b"over a pipe\0\xff");
        let far = pair.far_end().expect("a pipe of its own");
        let origin = origin_of(&target_path(far.address()));
        assert!(origin.starts_with("pipe://"), "{origin}");
        assert_ne!(origin, arrived.origin_uri, "each far end is its own pipe");
        assert!(arrived.origin_uri.starts_with("pipe://"));
        assert!(arrived.origin_uri.contains("xmip-loopback-"));
    }

    #[test]
    fn a_long_stream_and_an_empty_one_arrive_whole() {
        let name = unique("long");
        let far_end = NamedPipeTransport::new(&name);
        let listener = far_end.bind().expect("making the pipe");
        let long: Vec<u8> = (0..200_000u32)
            .map(|n| u8::try_from(n % 251).unwrap_or(0))
            .collect();
        let sent = long.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            let pipe = NamedPipeTransport::new("elsewhere");
            pipe.send(&name, &sent)?;
            std::thread::sleep(Duration::from_millis(20));
            pipe.send(&name, b"")
        });
        // Each read to its end before the next is accepted: the writer of the
        // long one waits on the reader, as a Receive Location's does.
        let first = far_end.accept_one(&listener).expect("the long one");
        let first = first.taken().expect("read");
        let second = far_end.accept_one(&listener).expect("the empty one");
        let second = second.taken().expect("read");
        sender.join().expect("thread").expect("sending");
        assert_eq!(first.bytes, long);
        assert_eq!(second.bytes, [] as [u8; 0]);
    }
}
