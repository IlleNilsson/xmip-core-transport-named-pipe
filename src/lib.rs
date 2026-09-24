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
//! the receiver knows the Stream is whole. `pipe.rs` holds the object on
//! each system; this file holds the transport. The standard library makes
//! neither object, so the `interprocess` crate does, behind a safe surface.
//!
//! Proven where the operating system has the object, which is every system
//! the estate builds on: a Windows box proves the Windows pipe, a Unix box
//! the FIFO. A pipe is not an artefact anyone claims — one writer at a time
//! is what the object already is — so [`Transport::claims`] answers `None`.
//!
//! The origin URI is the pipe's name: `pipe://./orders` on Windows for
//! `\\.\pipe\orders`, `pipe:///run/xmip/orders` for a FIFO. A send target
//! is a pipe name, an operating-system path, or `pipe://` and either.

pub mod pipe;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

pub use pipe::Listener;
use transport::error::Result;
use transport::held::Held;
use transport::loopback::{FarEnd, Loopback};
use transport::{Arrived, Directions, Transport};

pub struct NamedPipeTransport {
    path: PathBuf,
    timeout: Option<Duration>,
}

impl NamedPipeTransport {
    /// The pipe called `name`: a bare name under `\\.\pipe\` on Windows and
    /// in the temporary directory on Unix, or a path as it is.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            path: pipe::path_of(name),
            timeout: None,
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

    /// Make the pipe, so a sender has something to open.
    ///
    /// # Errors
    /// Where the name is not permitted.
    pub fn bind(&self) -> Result<Listener> {
        Listener::create(&self.path)
    }

    /// Take one connection from an already-made pipe, to its end.
    ///
    /// # Errors
    /// Where the pipe could not be read.
    pub fn accept_one(&self, listener: &Listener) -> Result<Arrived> {
        let bytes = listener.accept_one(self.timeout)?;
        Ok(Arrived::new(self.origin(), bytes))
    }
}

/// The path a target names: `pipe://` and a name or path, or either bare.
#[must_use]
pub fn target_path(target: &str) -> PathBuf {
    pipe::path_of(target.strip_prefix("pipe://").unwrap_or(target))
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

    /// Make the pipe, and take one connection to its end.
    fn receive(&self) -> Result<Vec<Arrived>> {
        let listener = self.bind()?;
        Ok(vec![self.accept_one(&listener)?])
    }

    /// Open the pipe the target names, write the bytes, close.
    fn send(&self, target: &str, bytes: &[u8]) -> Result<()> {
        pipe::write_once(&target_path(target), bytes)
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
    /// A made pipe waiting for its one connection.
    fn far_end(&self) -> Result<Box<dyn FarEnd>> {
        let transport = Self::new(&fresh_name());
        let listener = transport.bind()?;
        let address = transport.path().display().to_string();
        Ok(Box::new(Held::new(address, move || {
            transport.accept_one(&listener)
        })))
    }

    /// On Windows a connection that writes nothing and closes before the
    /// reader is waiting is one the pipe never saw: the object drops it,
    /// and the reader waits on. A Stream of nothing therefore reaches a
    /// reader already in its accept — the FIFO carries it whichever end
    /// opened first — and a loopback, whose two ends start together,
    /// declares it rather than racing for it.
    fn refuses(&self, payload: &[u8]) -> Option<String> {
        if cfg!(windows) && payload.is_empty() {
            Some(String::from(
                "a Stream of nothing: a Windows pipe drops a connection that \
                 writes nothing and closes before the reader is waiting",
            ))
        } else {
            None
        }
    }

    fn send_to(&self, address: &str, payload: &[u8]) -> Result<()> {
        if let Some(why) = self.refuses(payload) {
            return Err(transport::error::protocol_error(why));
        }
        Self::new(address).send(address, payload)
    }

    /// A pipe is connected to by opening it, not by a TCP connect: open,
    /// write one byte, close. One byte rather than none, because a poke
    /// that writes nothing is, on Windows, a connection that never
    /// happened, and the far end would wait on; the byte is the far end's
    /// to discard, since the failed send is what the round reports.
    fn unblock(&self, address: &str) {
        drop(pipe::write_once(&target_path(address), &[0]));
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
    fn the_loopback_returns_the_edge_payloads_whole() {
        let pair = NamedPipeTransport::loopback();
        for (name, bytes) in edge_payloads() {
            if let Some(why) = pair.refuses(&bytes) {
                assert!(cfg!(windows) && name == "empty", "{name}: {why}");
                let error = pair.round(&bytes).expect_err("refused, and judged");
                assert!(error.message.contains("a Stream of nothing"), "{error}");
                continue;
            }
            let arrived = pair
                .round(&bytes)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(arrived.bytes, bytes, "{name}");
            assert!(arrived.origin_uri.starts_with("pipe://"), "{name}");
        }
        assert!(pair.ceiling().is_none());
        assert!(pair.refuses(b"\r\n\0").is_none());
        assert_eq!(pair.refuses(b"").is_some(), cfg!(windows));
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
        assert_eq!(pipe.path(), pipe::path_of("orders"));
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
        assert_eq!(target_path("pipe://orders"), pipe::path_of("orders"));
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
        let first = far_end.accept_one(&listener).expect("the long one");
        let second = far_end.accept_one(&listener).expect("the empty one");
        sender.join().expect("thread").expect("sending");
        assert_eq!(first.bytes, long);
        assert!(second.bytes.is_empty());
    }
}
