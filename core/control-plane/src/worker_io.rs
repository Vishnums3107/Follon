//! Bounded, deadline-enforced frame transport for a strategy worker process.
//!
//! A worker is untrusted code behind a pipe. Reading its output with an
//! unbounded `read_line` lets it exhaust memory with one endless line, waiting
//! for its answer with no deadline lets it hang a replay forever, and writing to
//! it with a blocking `write_all` hangs the same way once it stops reading.
//!
//! Each direction therefore runs on its own thread. The reader applies its bound
//! before a frame is allocated, and every round trip waits at most one deadline
//! for its answer. Time never enters a result: an expired deadline ends the
//! replay with an error, and it is the only thing a deadline does (delivery
//! state E7.2).

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use crate::EngineError;

/// Bounds on the frames exchanged with one strategy worker.
///
/// A frame is one newline-terminated JSON line. The defaults are generous, for
/// a hung or hostile worker and not for a slow strategy: the frame limit admits
/// the largest state and metric set the service contract allows in any
/// realistic form, and a callback has a full minute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StrategyWorkerLimits {
    /// Longest accepted frame in bytes, newline included. A longer frame is
    /// refused before it is buffered in full.
    pub max_frame_bytes: usize,
    /// Longest wait for one worker answer, covering the write of the request.
    pub frame_deadline: Duration,
}

impl StrategyWorkerLimits {
    /// Frames of 16 MiB and a 60 second round trip.
    pub const DEFAULT: Self = Self {
        max_frame_bytes: 16 * 1024 * 1024,
        frame_deadline: Duration::from_secs(60),
    };

    /// The smallest frame limit: a ready frame and an error frame fit well
    /// inside it, so a smaller value could only refuse a well-formed worker.
    pub const MIN_FRAME_BYTES: usize = 4 * 1024;

    /// The largest frame limit. A limit is a bound on what an untrusted worker may make
    /// this process hold, so one that no machine could honour is a bound in name only.
    pub const MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;

    /// Refuses limits that would make a well-formed worker fail, or that bound nothing.
    pub fn validate(&self) -> Result<(), EngineError> {
        if !(Self::MIN_FRAME_BYTES..=Self::MAX_FRAME_BYTES).contains(&self.max_frame_bytes) {
            return Err(EngineError(format!(
                "strategy worker frame limit must be between {} and {} bytes",
                Self::MIN_FRAME_BYTES,
                Self::MAX_FRAME_BYTES
            )));
        }
        if self.frame_deadline.is_zero() {
            return Err(EngineError(
                "strategy worker frame deadline must not be zero".to_owned(),
            ));
        }
        Ok(())
    }
}

impl Default for StrategyWorkerLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why no frame arrived.
#[derive(Debug)]
pub(crate) enum FrameFault {
    /// The worker closed its output between frames.
    Closed,
    /// The worker closed its output in the middle of a frame.
    Truncated,
    /// A frame passed the limit before its newline.
    Oversized,
    /// The output could not be read.
    Unreadable,
    /// No frame arrived within the deadline.
    TimedOut,
}

impl FrameFault {
    pub(crate) fn describe(&self, limits: &StrategyWorkerLimits) -> EngineError {
        EngineError(match self {
            Self::Closed => "strategy worker closed stdout before returning a response".to_owned(),
            Self::Truncated => "strategy worker closed stdout in the middle of a frame".to_owned(),
            Self::Oversized => format!(
                "strategy worker frame exceeds the {} byte frame limit",
                limits.max_frame_bytes
            ),
            Self::Unreadable => "strategy worker output could not be read".to_owned(),
            Self::TimedOut => format!(
                "strategy worker did not answer within the {} second frame deadline",
                limits.frame_deadline.as_secs_f64()
            ),
        })
    }
}

/// The two pipe threads of one worker and the limits they enforce.
pub(crate) struct WorkerTransport {
    outgoing: Option<Sender<Vec<u8>>>,
    incoming: Receiver<Result<Vec<u8>, FrameFault>>,
    limits: StrategyWorkerLimits,
}

impl WorkerTransport {
    pub(crate) fn start(
        stdin: ChildStdin,
        stdout: ChildStdout,
        limits: StrategyWorkerLimits,
    ) -> Self {
        Self {
            outgoing: Some(spawn_writer(stdin)),
            incoming: spawn_reader(stdout, limits.max_frame_bytes),
            limits,
        }
    }

    pub(crate) fn limits(&self) -> &StrategyWorkerLimits {
        &self.limits
    }

    /// Queues one frame for the worker. It never blocks: the writer thread does
    /// the blocking write, and the deadline in [`Self::receive`] covers it.
    pub(crate) fn send(&self, frame: Vec<u8>) -> Result<(), EngineError> {
        self.outgoing
            .as_ref()
            .and_then(|outgoing| outgoing.send(frame).ok())
            .ok_or_else(|| EngineError("strategy worker stdin is closed".to_owned()))
    }

    /// Waits at most one deadline for the next frame, without its newline.
    pub(crate) fn receive(&self) -> Result<Vec<u8>, FrameFault> {
        match self.incoming.recv_timeout(self.limits.frame_deadline) {
            Ok(outcome) => outcome,
            Err(RecvTimeoutError::Timeout) => Err(FrameFault::TimedOut),
            Err(RecvTimeoutError::Disconnected) => Err(FrameFault::Closed),
        }
    }

    /// Closes the worker's input, so a worker that is reading sees end of file.
    pub(crate) fn close_input(&mut self) {
        self.outgoing.take();
    }
}

fn spawn_writer(mut stdin: ChildStdin) -> Sender<Vec<u8>> {
    let (sender, frames) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        for mut frame in frames {
            frame.push(b'\n');
            if stdin
                .write_all(&frame)
                .and_then(|()| stdin.flush())
                .is_err()
            {
                break;
            }
        }
        // Dropping `stdin` here is what closes the worker's input.
    });
    sender
}

fn spawn_reader(
    stdout: ChildStdout,
    max_frame_bytes: usize,
) -> Receiver<Result<Vec<u8>, FrameFault>> {
    // A rendezvous channel of one frame: a worker that talks unprompted fills
    // its pipe and blocks, instead of piling frames up in this process.
    let (sender, frames) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let outcome = read_frame(&mut reader, max_frame_bytes);
            let finished = outcome.is_err();
            if sender.send(outcome).is_err() || finished {
                break;
            }
        }
    });
    frames
}

/// Reads one newline-terminated frame, without its newline.
///
/// At most `max_frame_bytes + 1` bytes are ever buffered, so a worker that
/// never sends a newline cannot grow this process's memory. The limit counts
/// the newline: a frame of exactly `max_frame_bytes` is accepted, one byte more
/// is not.
fn read_frame(reader: &mut impl BufRead, max_frame_bytes: usize) -> Result<Vec<u8>, FrameFault> {
    let mut bytes = Vec::new();
    match reader
        .by_ref()
        .take((max_frame_bytes as u64).saturating_add(1))
        .read_until(b'\n', &mut bytes)
    {
        Ok(0) => Err(FrameFault::Closed),
        Ok(_) if bytes.ends_with(b"\n") && bytes.len() <= max_frame_bytes => {
            bytes.pop();
            Ok(bytes)
        }
        Ok(_) if bytes.len() > max_frame_bytes => Err(FrameFault::Oversized),
        Ok(_) => Err(FrameFault::Truncated),
        Err(_) => Err(FrameFault::Unreadable),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, BufReader, Cursor, Read};

    use super::*;

    fn read(bytes: &[u8], max_frame_bytes: usize) -> Result<Vec<u8>, FrameFault> {
        read_frame(
            &mut BufReader::new(Cursor::new(bytes.to_vec())),
            max_frame_bytes,
        )
    }

    #[test]
    fn a_frame_is_returned_without_its_newline() {
        assert_eq!(read(b"{\"a\":1}\n", 64).unwrap(), b"{\"a\":1}");
        assert_eq!(read(b"\n", 64).unwrap(), b"");
    }

    #[test]
    fn the_limit_counts_the_newline() {
        let mut exact = vec![b'x'; 63];
        exact.push(b'\n');
        assert_eq!(read(&exact, 64).unwrap().len(), 63);
        let mut over = vec![b'x'; 64];
        over.push(b'\n');
        assert!(matches!(read(&over, 64), Err(FrameFault::Oversized)));
    }

    /// The reader adds one to the limit to see a frame that is a byte too long, and a limit
    /// at the end of the range must not overflow doing it.
    #[test]
    fn a_limit_at_the_end_of_the_range_still_reads_a_frame() {
        assert_eq!(read(b"{}\n", usize::MAX).unwrap(), b"{}");
    }

    #[test]
    fn frames_are_read_one_at_a_time() {
        let mut reader = BufReader::new(Cursor::new(b"one\ntwo\n".to_vec()));
        assert_eq!(read_frame(&mut reader, 64).unwrap(), b"one");
        assert_eq!(read_frame(&mut reader, 64).unwrap(), b"two");
        assert!(matches!(
            read_frame(&mut reader, 64),
            Err(FrameFault::Closed)
        ));
    }

    #[test]
    fn a_frame_without_a_newline_is_truncated_or_oversized() {
        assert!(matches!(read(b"abc", 64), Err(FrameFault::Truncated)));
        assert!(matches!(read(&[b'x'; 65], 64), Err(FrameFault::Oversized)));
        assert!(matches!(read(&[b'x'; 64], 64), Err(FrameFault::Truncated)));
        assert!(matches!(read(b"", 64), Err(FrameFault::Closed)));
    }

    /// Yields `x` forever and fails once it has handed out more than `budget`
    /// bytes, so a reader with no bound errors here instead of exhausting
    /// memory or hanging.
    struct Endless {
        handed_out: usize,
        budget: usize,
    }

    impl Read for Endless {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.handed_out > self.budget {
                return Err(io::Error::other("the reader never stopped"));
            }
            buffer.fill(b'x');
            self.handed_out += buffer.len();
            Ok(buffer.len())
        }
    }

    #[test]
    fn a_frame_that_never_ends_is_refused_after_buffering_only_the_limit() {
        let limit = 4 * 1024;
        let endless = Endless {
            handed_out: 0,
            budget: 16 * limit,
        };
        let mut reader = BufReader::new(endless);
        assert!(matches!(
            read_frame(&mut reader, limit),
            Err(FrameFault::Oversized)
        ));
        // The buffer's own read-ahead is the only excess.
        assert!(reader.get_ref().handed_out <= limit + 8 * 1024 + 1);
    }

    /// Once the worker has gone, the writer thread's first failed write ends it
    /// and every later frame is refused instead of queued for nobody.
    #[test]
    fn sending_to_a_worker_that_has_gone_is_refused() {
        // The test binary itself, asked to list its tests, is a child that
        // exits at once and needs no interpreter. Semgrep's `current-exe` rule
        // warns against trusting the path for a security decision, and this
        // test only wants a process that ends.
        // nosemgrep: rust.lang.security.current-exe.current-exe
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("the test binary starts");
        let transport = WorkerTransport::start(
            child.stdin.take().unwrap(),
            child.stdout.take().unwrap(),
            StrategyWorkerLimits::DEFAULT,
        );
        child.wait().unwrap();
        let refusal = (0..200)
            .find_map(|_| {
                let outcome = transport.send(vec![b'x'; 1024]);
                thread::sleep(Duration::from_millis(10));
                outcome.err()
            })
            .expect("the writer noticed that the worker had gone");
        assert_eq!(refusal.0, "strategy worker stdin is closed");
    }

    #[test]
    fn a_read_failure_is_reported_as_unreadable() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("broken pipe"))
            }
        }
        assert!(matches!(
            read_frame(&mut BufReader::new(Failing), 64),
            Err(FrameFault::Unreadable)
        ));
    }
}
