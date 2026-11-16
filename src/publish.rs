//! Atomic publishing of user-visible files (rule 29, spec §3): the bytes go to a
//! uniquely named temp file in the destination directory, flushed, then renamed
//! into place, so a failure mid-way never leaves a half-written PDF or template
//! source where the user expects the real file.
//!
//! Two producers share this seam: `cv build` (the PDF) and `cv templates
//! --extract` (the built-in folder's files).

use std::io::{self, Write};
use std::path::Path;

use crate::error::{Error, Result};

/// Write `bytes` to `path` atomically: temp in the same directory, write, flush,
/// persist. Every failure reports the same typed `Error::Write` naming the
/// destination.
pub(crate) fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    publish_with(path, &|sink| write_bytes(sink, bytes))
}

/// The same, with the byte-writing injected: the temp-create and persist steps run
/// for real while a test can make the write itself fail — a real disk only breaks
/// when it is full or failing, which no test can arrange, while the failure path
/// and the message the user sees must still be tested (rule 21, the seam pattern
/// of ADR-0036).
fn publish_with(path: &Path, write: &dyn Fn(&mut dyn Write) -> io::Result<()>) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let temp = tempfile::Builder::new()
        .prefix(".cv-")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(|source| write_failure(path, source))?;
    write(&mut temp.as_file()).map_err(|source| write_failure(path, source))?;
    temp.persist(path)
        .map_err(|e| write_failure(path, e.error))?;
    Ok(())
}

/// The bytes into a sink, then flush it: the rename that follows must not publish
/// a file whose tail is still buffered (spec §3). The sink is a parameter so the
/// write-failure and flush-failure arms have tests: real files get injected
/// broken sinks instead of a broken disk.
fn write_bytes(sink: &mut dyn Write, bytes: &[u8]) -> io::Result<()> {
    sink.write_all(bytes)?;
    sink.flush()
}

/// One write failure: the destination that was asked for, the OS reason.
fn write_failure(path: &Path, source: std::io::Error) -> Error {
    Error::Write {
        path: path.display().to_string(),
        message: source.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sink that can be told to break on the write, on the flush, or on neither:
    /// the three states of the disk the bytes are written to.
    struct Sink {
        break_write: bool,
        break_flush: bool,
        written: Vec<u8>,
    }

    impl Sink {
        /// A disk that takes the bytes.
        fn working() -> Self {
            Self {
                break_write: false,
                break_flush: false,
                written: Vec::new(),
            }
        }

        /// A disk with no room left: `write` itself refuses.
        fn full() -> Self {
            Self {
                break_write: true,
                break_flush: false,
                written: Vec::new(),
            }
        }

        /// A device that accepts the bytes and loses them on the way out: the failure
        /// only shows when the buffer is flushed.
        fn failing() -> Self {
            Self {
                break_write: false,
                break_flush: true,
                written: Vec::new(),
            }
        }
    }

    impl io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.break_write {
                return Err(io::Error::other("No space left on device"));
            }
            self.written.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.break_flush {
                return Err(io::Error::other("disk I/O error"));
            }
            Ok(())
        }
    }

    fn write_error(sink: &mut Sink) -> io::Error {
        write_bytes(sink, b"%PDF-1.7").expect_err("a broken disk must fail the write")
    }

    #[test]
    fn a_full_disk_names_the_destination_and_the_os_reason() {
        let message = write_error(&mut Sink::full()).to_string();
        assert!(
            message.contains("No space left on device"),
            "the OS reason must survive: {message}"
        );
    }

    /// The bytes can leave the buffer only to be lost: a flush failure is the same
    /// typed error, or the rename would publish a truncated PDF.
    #[test]
    fn a_failing_device_is_reported_at_the_flush() {
        let message = write_error(&mut Sink::failing()).to_string();
        assert!(message.contains("disk I/O error"), "{message}");
    }

    #[test]
    fn a_healthy_sink_receives_every_byte() {
        let mut sink = Sink::working();
        write_bytes(&mut sink, b"%PDF-1.7 bytes").expect("the write works");
        assert_eq!(sink.written, b"%PDF-1.7 bytes", "nothing lost or reordered");
    }

    #[test]
    fn a_failing_writer_aborts_publish_before_persisting() {
        // The publish-level `write(...)?` arm: with the write injected to fail, the
        // destination is never published, and the typed error names it (a failed
        // build must not leave a corrupt file — spec §3).
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("out.pdf");
        let err = publish_with(&path, &|_| Err(io::Error::other("No space left on device")))
            .expect_err("the failing writer must abort");
        let message = err.to_string();
        assert!(
            message.contains("could not write")
                && message.contains("out.pdf")
                && message.contains("No space left on device"),
            "the Write error names the destination and the reason: {message}"
        );
        assert!(!path.exists(), "nothing is published on a failed write");
    }

    #[test]
    fn publish_writes_the_destination_and_leaves_no_temp_behind() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("out.txt");
        publish(&path, b"payload").expect("publishes");
        assert_eq!(std::fs::read(&path).expect("read"), b"payload");
        let names: Vec<String> = std::fs::read_dir(root.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["out.txt"],
            "the temp file is renamed, never left: {names:?}"
        );
    }

    #[test]
    fn publish_replacing_an_existing_destination_keeps_only_the_new_bytes() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("out.txt");
        std::fs::write(&path, b"old").expect("seed");
        publish(&path, b"new").expect("overwrites via persist");
        assert_eq!(std::fs::read(&path).expect("read"), b"new");
    }

    #[test]
    fn a_persist_collision_names_the_destination_as_a_write_error() {
        // A directory blocking the rename target fails `persist`; the error names
        // the destination the user asked for.
        let root = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(root.path().join("blocked")).expect("block the target");
        let err = publish(&root.path().join("blocked"), b"x").expect_err("target is a directory");
        let message = err.to_string();
        assert!(
            message.contains("could not write") && message.contains("blocked"),
            "the Write error names the destination: {message}"
        );
    }
}
