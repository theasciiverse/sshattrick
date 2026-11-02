//! Where a player's screen goes: an SSH channel (upstream's only case) or,
//! added by Asciiverse, the terminal ssHattrick is running in, so the
//! person at this computer plays without connecting to anything.

use frittura_ssh_core::SshWriterProxy;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Notify;

pub enum Out {
    Ssh(SshWriterProxy),
    Local(LocalOut),
}

/// Bytes for this terminal, buffered like the SSH writer and written out
/// on `send`. Closing it tells the local front end the player has left.
pub struct LocalOut {
    buf: Vec<u8>,
    closed: bool,
    left: Arc<Notify>,
}

impl LocalOut {
    pub fn new(left: Arc<Notify>) -> LocalOut {
        LocalOut { buf: Vec::new(), closed: false, left }
    }

    fn flush_to_terminal(&mut self) -> std::io::Result<usize> {
        let n = self.buf.len();
        if n > 0 {
            let mut out = std::io::stdout().lock();
            out.write_all(&self.buf)?;
            out.flush()?;
            self.buf.clear();
        }
        Ok(n)
    }
}

impl std::fmt::Debug for Out {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Out::Ssh(w) => w.fmt(f),
            Out::Local(l) => f.debug_struct("LocalOut").field("buffered", &l.buf.len()).field("closed", &l.closed).finish(),
        }
    }
}

impl Out {
    pub async fn send(&mut self) -> std::io::Result<usize> {
        match self {
            Out::Ssh(w) => w.send().await,
            Out::Local(l) if l.closed => Ok(0),
            Out::Local(l) => l.flush_to_terminal(),
        }
    }

    pub async fn send_and_close(&mut self) {
        match self {
            Out::Ssh(w) => w.send_and_close().await,
            Out::Local(l) => {
                if l.closed {
                    return;
                }
                let _ = l.flush_to_terminal();
                l.closed = true;
                l.left.notify_one();
            }
        }
    }
}

impl Write for Out {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Out::Ssh(w) => w.write(buf),
            Out::Local(l) => {
                l.buf.extend_from_slice(buf);
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Out::Ssh(w) => w.flush(),
            // drawn on send, like the SSH writer
            Out::Local(_) => Ok(()),
        }
    }
}
