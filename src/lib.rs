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

pub use pipe::Listener;
use transport::error::Result;
use transport::{Arrived, Directions, Transport};

pub struct NamedPipeTransport {
    path: PathBuf,
}

impl NamedPipeTransport {
    /// The pipe called `name`: a bare name under `\\.\pipe\` on Windows and
    /// in the temporary directory on Unix, or a path as it is.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            path: pipe::path_of(name),
        }
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
        let bytes = listener.accept_one()?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn unique(name: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        format!("xmip-pipe-{name}-{}-{nanos}", std::process::id())
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
        let name = unique("stream");
        let far_end = NamedPipeTransport::new(&name);
        let listener = far_end.bind().expect("making the pipe");
        let target = format!("pipe://{name}");
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            NamedPipeTransport::new("elsewhere").send(&target, b"over a pipe\0\xff")
        });
        let arrived = far_end.accept_one(&listener).expect("accepting");
        sender.join().expect("thread").expect("sending");
        assert_eq!(arrived.bytes, b"over a pipe\0\xff");
        assert_eq!(arrived.origin_uri, far_end.origin());
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
