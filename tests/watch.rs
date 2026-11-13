//! M5 watch e2e tests (plan 5.6/5.7, spec §3): `cv watch` builds at start and
//! rebuilds on change of the YAML **or its custom template folder**.
//!
//! The tests spawn the real binary and read its stdout/stderr line by line through
//! helper threads and channels with a bounded deadline: waiting is driven by the
//! process's own output (no sleeps, no ordering assumptions — `.AGENTS.md` rule 18;
//! the deadline only converts a hang into a failure with a message).

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// Generous upper bound for a whole save→event→rebuild→line round trip on a slow
/// machine; normal-case latency is a fraction of this.
const DEADLINE: Duration = Duration::from_secs(60);

fn unique_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cv-watch-{}-{}", name, std::process::id()));
    fs::remove_dir_all(&base).ok();
    fs::create_dir_all(&base).expect("create test dir");
    base
}

struct Watcher {
    child: Child,
    stdout: Receiver<String>,
    stderr: Receiver<String>,
}

impl Watcher {
    fn spawn(dir: &Path, args: &[&str]) -> Watcher {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cv"))
            .args(args)
            .current_dir(dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("cv watch starts");
        let stdout = pump(child.stdout.take().expect("piped stdout"));
        let stderr = pump(child.stderr.take().expect("piped stderr"));
        Watcher {
            child,
            stdout,
            stderr,
        }
    }

    /// The next stdout line containing `needle` within [`DEADLINE`].
    fn wait_stdout(&self, needle: &str) -> String {
        next(&self.stdout, needle, "stdout")
    }

    /// The next stderr line containing `needle` within [`DEADLINE`].
    fn wait_stderr(&self, needle: &str) -> String {
        next(&self.stderr, needle, "stderr")
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn pump<R: std::io::Read + Send + 'static>(reader: R) -> Receiver<String> {
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            if tx.send(line.expect("utf-8 output line")).is_err() {
                break;
            }
        }
    });
    lines
}

fn next(rx: &Receiver<String>, needle: &str, stream: &str) -> String {
    let mut skipped: Vec<String> = Vec::new();
    let start = Instant::now();
    loop {
        let remaining = DEADLINE.saturating_sub(start.elapsed());
        match rx.recv_timeout(remaining) {
            Ok(line) if line.contains(needle) => {
                for skipped in skipped.iter() {
                    eprintln!("watch {stream} (skipped): {skipped}");
                }
                return line;
            }
            Ok(line) => skipped.push(line),
            Err(_) => panic!(
                "no {stream} line containing {needle:?} within {DEADLINE:?}; skipped: {skipped:?}"
            ),
        }
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create copy dir");
    for entry in fs::read_dir(from).expect("read dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

fn init_in(dir: &Path) {
    let status = Command::new(env!("CARGO_BIN_EXE_cv"))
        .args(["init"])
        .current_dir(dir)
        .status()
        .expect("cv init runs");
    assert!(status.success(), "cv init failed");
}

/// Plan 5.7: a YAML edit triggers a rebuild; the PDF carries the new content.
#[test]
fn watch_rebuilds_when_the_yaml_changes() {
    let dir = unique_dir("yaml");
    init_in(&dir);

    let mut watcher = Watcher::spawn(&dir, &["watch"]);
    let first = watcher.wait_stdout("cv.pdf  1 page");
    assert!(first.contains("fit:"), "initial build line: {first}");
    watcher.wait_stdout("watching");

    // Rename the person on disk; the rebuilt PDF must show it.
    let yaml_path = dir.join("cv.yaml");
    let yaml = fs::read_to_string(&yaml_path)
        .expect("read yaml")
        .replace("Jane Doe", "Jon Doe");
    fs::write(&yaml_path, yaml).expect("write yaml");

    let second = watcher.wait_stdout("cv.pdf  1 page");
    assert!(
        !second.contains("watching"),
        "a build line follows the edit: {second}"
    );
    let text = pdf_extract::extract_text_by_pages(dir.join("cv.pdf"))
        .expect("extract")
        .concat();
    assert!(text.contains("Jon Doe"), "rebuild picked up the edit");

    watcher.kill();
}

/// Plan 5.7 (spec §6): editing the *custom template folder* triggers a rebuild.
#[test]
fn watch_rebuilds_when_the_custom_template_folder_changes() {
    let dir = unique_dir("template");
    init_in(&dir);
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/compact"),
        &dir.join("my-template"),
    );
    let yaml_path = dir.join("cv.yaml");
    let yaml = fs::read_to_string(&yaml_path)
        .expect("read yaml")
        .replace("template: compact", "template: ./my-template");
    fs::write(&yaml_path, yaml).expect("point at the folder");

    let mut watcher = Watcher::spawn(&dir, &["watch"]);
    watcher.wait_stdout("cv.pdf  1 page");
    watcher.wait_stdout("watching");

    fs::write(dir.join("my-template/template.typ"), {
        let mut source =
            fs::read_to_string(dir.join("my-template/template.typ")).expect("read template");
        source.push_str("\n#block[watched-template-marker]\n");
        source
    })
    .expect("edit template");

    watcher.wait_stdout("cv.pdf  1 page");
    let text = pdf_extract::extract_text_by_pages(dir.join("cv.pdf"))
        .expect("extract")
        .concat();
    assert!(
        text.contains("watched-template-marker"),
        "the template edit must reach the PDF"
    );

    watcher.kill();
}

/// A broken save is reported and does not kill the watch: the fix afterwards
/// rebuilds (plan 5.6 — a broken half-second while editing is normal).
#[test]
fn watch_survives_a_broken_build_and_reports_it() {
    let dir = unique_dir("broken");
    init_in(&dir);

    let mut watcher = Watcher::spawn(&dir, &["watch"]);
    watcher.wait_stdout("cv.pdf  1 page");
    watcher.wait_stdout("watching");

    fs::write(dir.join("cv.yaml"), "schema: 1\ncv:\n  name: [unclosed\n").expect("break the yaml");
    // The watch prints build diagnostics itself (they never reach `main`): the
    // miette report header and source name must appear on stderr.
    watcher.wait_stderr("×");
    watcher.wait_stderr("cv.yaml");

    fs::write(
        dir.join("cv.yaml"),
        "schema: 1\ncv:\n  name: Fixed After Break\ndesign:\n  template: compact\n",
    )
    .expect("fix the yaml");
    watcher.wait_stdout("cv.pdf  1 page");
    let text = pdf_extract::extract_text_by_pages(dir.join("cv.pdf"))
        .expect("extract")
        .concat();
    assert!(
        text.contains("Fixed After Break"),
        "the post-break rebuild must show the fixed content"
    );
    watcher.kill();
}

/// `cv watch` on a missing *directory* is a clean startup error (spec §13.9).
#[test]
fn watch_names_the_missing_directory_and_exits() {
    let dir = unique_dir("missing-dir");
    let output = Command::new(env!("CARGO_BIN_EXE_cv"))
        .args(["watch", "no-such-dir/cv.yaml"])
        .current_dir(&dir)
        .output()
        .expect("binary runs");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no-such-dir"),
        "error names the directory: {stderr}"
    );
}

/// `--quiet` watch: the progress banner and per-build stdout result lines are
/// suppressed (ADR-0012), while build *errors* still reach stderr — essentials
/// are never suppressed. Proven positively: a broken save under `--quiet` still
/// reports on stderr.
#[test]
fn quiet_watch_still_reports_errors() {
    let dir = unique_dir("quiet");
    init_in(&dir);

    let mut watcher = Watcher::spawn(&dir, &["--quiet", "watch"]);
    // No "watching" banner under --quiet; drive an error instead and confirm it
    // surfaces on stderr regardless of quiet.
    fs::write(dir.join("cv.yaml"), "schema: 1\ncv:\n  name: [unclosed\n").expect("break the yaml");
    watcher.wait_stderr("×");
    watcher.wait_stderr("cv.yaml");
    watcher.kill();
}
