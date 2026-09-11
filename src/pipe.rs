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

use std::io::Read;
use std::path::{Path, PathBuf};

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

    /// Wait for one writer, and read what it writes to its end.
    ///
    /// # Errors
    /// Where the pipe could not be opened or read.
    pub fn accept_one(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        #[cfg(windows)]
        {
            let mut stream = self
                .inner
                .accept()
                .map_err(|e| classify("accepting a connection", &e))?;
            stream
                .read_to_end(&mut bytes)
                .map_err(|e| classify("reading the pipe", &e))?;
        }
        #[cfg(not(windows))]
        {
            let mut file =
                std::fs::File::open(&self.path).map_err(|e| classify("opening the pipe", &e))?;
            file.read_to_end(&mut bytes)
                .map_err(|e| classify("reading the pipe", &e))?;
        }
        Ok(bytes)
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
