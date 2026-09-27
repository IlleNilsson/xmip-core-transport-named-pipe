//! The operating system's pipe: made, accepted and connected to, on each
//! system in that system's way.
//!
//! On Windows a named pipe is a kernel object under `\\.\pipe\`, created
//! by one process and connected to by another, and one instance carries
//! one connection. On Unix a named pipe is a FIFO: a special file on the
//! file system that a reader opens and a writer opens, and the kernel
//! joins the two. Both are one Stream per connection here — the writer
//! closes, the reader sees the end — and nothing above this file knows
//! which one it has.

use std::path::{Path, PathBuf};
use std::time::Duration;

use transport::error::{Result, classify};

/// The operating system's path for a pipe called `name`: as given where it
/// already is a path, `\\.\pipe\<name>` on Windows otherwise, and a file in
/// the temporary directory on Unix otherwise.
#[must_use]
pub fn path_of(name: &str) -> PathBuf {
    if name.contains('/') || name.contains(char::from(92)) {
        return PathBuf::from(name);
    }
    if cfg!(windows) {
        PathBuf::from(format!("\\\\.\\pipe\\{name}"))
    } else {
        std::env::temp_dir().join(name)
    }
}

/// The made end, waiting for a connection.
pub struct Listener {
    #[cfg(windows)]
    inner: interprocess::os::windows::named_pipe::PipeListener<
        interprocess::os::windows::named_pipe::pipe_mode::Bytes,
        interprocess::os::windows::named_pipe::pipe_mode::Bytes,
    >,
    path: PathBuf,
}

impl Listener {
    /// Make the pipe at `path`.
    ///
    /// # Errors
    /// Where the name is not permitted or, on Unix, its directory is
    /// missing. A FIFO a previous run left behind is reused.
    pub fn create(path: &Path) -> Result<Self> {
        #[cfg(windows)]
        {
            use interprocess::os::windows::named_pipe::{PipeListenerOptions, pipe_mode};
            let inner = PipeListenerOptions::new()
                .path(path)
                .create_duplex::<pipe_mode::Bytes>()
                .map_err(|e| classify("creating the pipe", &e))?;
            Ok(Self {
                inner,
                path: path.to_path_buf(),
            })
        }
        #[cfg(not(windows))]
        {
            if !path.exists() {
                interprocess::os::unix::fifo_file::create_fifo(path, 0o600)
                    .map_err(|e| classify("creating the pipe", &e))?;
            }
            Ok(Self {
                path: path.to_path_buf(),
            })
        }
    }

    /// Where the pipe is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Wait for one writer within `timeout`, and read what it writes to its
    /// end. `None` waits for as long as it takes, which is what a listening
    /// Receive Location does.
    ///
    /// # Errors
    /// Where no writer came within `timeout`, or the pipe could not be opened
    /// or read.
    pub fn accept_one(&self, timeout: Option<Duration>) -> Result<Vec<u8>> {
        #[cfg(windows)]
        let mut pipe = transport::socket::accept_within(self, timeout, "no writer came")?;
        #[cfg(not(windows))]
        let mut pipe = self.open_within(timeout)?;
        Ok(net::read::to_end(&mut pipe, net::MAX_BODY)?)
    }
}

/// The pipe's accept, bounded by `timeout` through the capability's one
/// bounded accept (`transport::socket::accept_within`).
///
/// It waited for a writer for as long as it took until 2026-09-21, and a
/// far end whose near end never came waited for good — the defect that
/// hung the Playground's gate six times over TCP, and the same line here.
/// A pipe instance is not a socket and has no readiness the capability's
/// wait can block on; its connection is signalled through overlapped I/O,
/// which the pipe library does not offer and this crate may not write
/// unsafely. So the wait here is a nap of a quarter of a millisecond
/// between tries, where the socket listeners wait on the operating
/// system.
#[cfg(windows)]
impl transport::socket::Acceptor for Listener {
    type Accepted = interprocess::os::windows::named_pipe::PipeStream<
        interprocess::os::windows::named_pipe::pipe_mode::Bytes,
        interprocess::os::windows::named_pipe::pipe_mode::Bytes,
    >;

    fn accept_now(&self) -> std::io::Result<Self::Accepted> {
        // bounded: the caller's, accept_within's deadline or its None arm
        self.inner.accept()
    }

    fn nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        self.inner.set_nonblocking(nonblocking)
    }

    fn settle(accepted: &Self::Accepted) -> std::io::Result<()> {
        accepted.set_nonblocking(false)
    }

    fn wait_ready(&self, within: Duration) -> std::io::Result<()> {
        std::thread::sleep(within.min(Duration::from_micros(250)));
        Ok(())
    }
}

#[cfg(not(windows))]
impl Listener {
    /// The FIFO opened for reading within `timeout`; `None` waits for a
    /// writer as long as it takes.
    ///
    /// A FIFO's open for reading blocks until a writer opens the other end,
    /// and it waited for good until 2026-09-21. It could not be bounded on
    /// the machine the Windows half was written on, and the owner's Linux
    /// guest is what made it possible to write and run this one.
    ///
    /// The open happens on a thread of its own and is waited on with a
    /// deadline. Opening with `O_NONBLOCK` instead would return at once and
    /// then read an end-of-file for a writer that has not arrived yet, which
    /// is indistinguishable from one that has come and gone. When the
    /// deadline passes, the waiting open is released by opening the other end
    /// for writing and closing it — the one thing a blocked FIFO open waits
    /// for — so no thread is left behind.
    fn open_within(&self, timeout: Option<Duration>) -> Result<std::fs::File> {
        use std::sync::mpsc::{RecvTimeoutError, channel};
        use transport::TransportError;

        let Some(timeout) = timeout else {
            return std::fs::File::open(&self.path).map_err(|e| classify("opening the pipe", &e));
        };

        let path = self.path.clone();
        let (handing, arrival) = channel();
        let opener = std::thread::spawn(move || {
            drop(handing.send(std::fs::File::open(&path)));
        });

        match arrival.recv_timeout(timeout) {
            Ok(file) => {
                drop(opener.join());
                file.map_err(|e| classify("opening the pipe", &e))
            }
            Err(RecvTimeoutError::Timeout) => {
                drop(std::fs::OpenOptions::new().write(true).open(&self.path));
                drop(opener.join());
                Err(TransportError::retryable(format!(
                    "no writer came within {} ms",
                    timeout.as_millis()
                ))
                .at("opening the pipe"))
            }
            Err(RecvTimeoutError::Disconnected) => {
                drop(opener.join());
                Err(TransportError::permanent("the pipe's opener stopped").at("opening the pipe"))
            }
        }
    }
}

#[cfg(not(windows))]
impl Drop for Listener {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

/// Connect to the pipe at `path`, write `bytes` and close, which is what
/// tells the reader the Stream is whole.
///
/// # Errors
/// Where nothing has made the pipe — permanent, as a missing file is — or
/// it could not be written.
pub fn write_once(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    #[cfg(windows)]
    let mut stream = {
        use interprocess::os::windows::named_pipe::{DuplexPipeStream, pipe_mode};
        DuplexPipeStream::<pipe_mode::Bytes>::connect_by_path(path)
            .map_err(|e| classify("connecting to the pipe", &e))?
    };
    #[cfg(not(windows))]
    let mut stream = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| classify("connecting to the pipe", &e))?;
    stream
        .write_all(bytes)
        .map_err(|e| classify("writing to the pipe", &e))?;
    stream
        .flush()
        .map_err(|e| classify("flushing the pipe", &e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pipe_nobody_writes_to_gives_up_within_its_timeout() {
        // The defect this asserts is the one that hung the Playground's gate:
        // a far end whose near end never came waited for good (2026-09-21).
        let path = path_of(&format!("xmip-unanswered-{}", std::process::id()));
        let listener = Listener::create(&path).expect("the pipe is made");
        let began = std::time::Instant::now();

        let refused = listener.accept_one(Some(Duration::from_millis(200)));
        let waited = began.elapsed();

        assert!(refused.is_err(), "nothing was written, so nothing arrived");
        assert!(
            waited < Duration::from_secs(2),
            "gave up after {waited:?}, which is not a timeout"
        );
        assert!(
            waited >= Duration::from_millis(150),
            "gave up after {waited:?}, before it had waited"
        );
    }

    #[test]
    fn a_bare_name_becomes_the_systems_path_and_a_path_stays() {
        let bare = path_of("orders");
        if cfg!(windows) {
            assert_eq!(bare, PathBuf::from("\\\\.\\pipe\\orders"));
        } else {
            assert_eq!(bare, std::env::temp_dir().join("orders"));
        }
        assert_eq!(
            path_of("/run/xmip/orders"),
            PathBuf::from("/run/xmip/orders")
        );
        assert_eq!(
            path_of("\\\\server\\pipe\\orders"),
            PathBuf::from("\\\\server\\pipe\\orders")
        );
    }

    #[test]
    fn a_pipe_nobody_made_cannot_be_written() {
        let path = path_of(&format!("xmip-nobody-{}", std::process::id()));
        let error = write_once(&path, b"x").expect_err("nothing there");
        assert!(!error.retryable, "{error}");
    }
}
