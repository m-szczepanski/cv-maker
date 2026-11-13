//! `cv watch` (plan 5.6, spec §3): rebuild on change of the YAML or its custom
//! template folder, printing each build's result line.
//!
//! The loop trusts content, not event paths (ADR-0029): backends coalesce and report
//! with different granularity (FSEvents bursts vs. precise inotify), and our own
//! atomic PDF writes land next to the YAML. So every event burst is followed by a
//! content snapshot of the watched inputs, and only a snapshot that actually differs
//! triggers a rebuild — which also means a missed or noisy event can never cause a
//! rebuild loop or a stale document between changes.
//!
//! The rebuild itself *is* `cv build`'s code path (result line, warnings, exit-code-
//! neutral overflow included). Errors never end the watch: the watched file is the
//! one the user is actively editing, and broken half-seconds are normal.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::cli::{BuildArgs, Cli, WatchArgs};
use crate::error::{Error, Result};
use crate::model::Document;

/// Quiet period that collapses one save's event burst into a single rebuild (the
/// `notify` debouncing note of spec §2, implemented here without a second crate —
/// `.AGENTS.md` rule 22).
const DEBOUNCE_QUIET: Duration = Duration::from_millis(100);

pub fn run(args: &WatchArgs, cli: &Cli) -> Result<()> {
    let yaml = absolute_input(&args.file)?;
    // One rebuild is exactly one `cv build` run with the watch defaults: the
    // resolved output path is `<stem>.pdf` next to the input (spec §3).
    let build_args = BuildArgs {
        file: args.file.clone(),
        output: None,
        no_fit: false,
    };

    let mut targets = watch_targets(&yaml);
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)
        .map_err(|e| Error::Internal(format!("could not start file watching: {e}")))?;
    watcher
        .watch(&targets.yaml_dir, RecursiveMode::NonRecursive)
        .map_err(|e| {
            Error::Internal(format!(
                "could not watch '{}': {e}",
                targets.yaml_dir.display()
            ))
        })?;
    let mut watched_template: Option<PathBuf> = None;
    sync_template_watch(&mut watcher, &targets, &mut watched_template);

    // The baseline snapshot is taken *before* each build: a save landing while a
    // build is running wakes the next burst and is caught up (inner loop), so the
    // PDF always converges to the newest content.
    let mut before = snapshot(&targets);
    build_once(&build_args, cli);

    if !cli.quiet {
        println!("watching {}, press Ctrl-C to stop", args.file.display());
    }

    loop {
        match wait_for_burst(&rx, DEBOUNCE_QUIET) {
            Burst::Wake => {}
            // The event channel died while the watcher is still owned — a broken
            // notify internal, not a user situation; reporting beats silence.
            Burst::Disconnected => {
                return Err(Error::Internal("file watcher stopped".to_owned()));
            }
        }

        // Catch-up: rebuild until the on-disk state stops changing. The final
        // content may differ from what the last build read (an edit landed
        // mid-build), so the loop re-checks instead of going back to sleep.
        loop {
            let after = snapshot(&targets);
            if !after.differs(&before) {
                break;
            }
            // A changed YAML can re-point design.template (built-in → folder or to
            // a different folder), so the reference is re-read after its rebuild.
            let yaml_changed = after.entry(&targets.yaml) != before.entry(&targets.yaml);

            build_once(&build_args, cli);
            before = after;
            if yaml_changed {
                targets = watch_targets(&yaml);
            }
            sync_template_watch(&mut watcher, &targets, &mut watched_template);
        }
    }
}

/// What a rebuild can be caused by: the CV file and, if `design.template` is a path
/// reference, its template folder (spec §6: `cv watch` also watches the custom
/// template). Re-read after every YAML change so the reference can move.
struct WatchTargets {
    yaml: PathBuf,
    yaml_dir: PathBuf,
    template_dir: Option<PathBuf>,
}

/// The CV file as an absolute path with its directory canonicalized (the file itself
/// may not exist yet: creating it must also trigger a rebuild).
fn absolute_input(file: &Path) -> Result<PathBuf> {
    let name = file
        .file_name()
        .ok_or_else(|| Error::Unsupported(format!("'{}' is not a file path", file.display())))?;
    let parent = match file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let dir = parent.canonicalize().map_err(|source| Error::Io {
        path: parent.display().to_string(),
        source,
    })?;
    Ok(dir.join(name))
}

fn watch_targets(yaml: &Path) -> WatchTargets {
    let template_dir = std::fs::read_to_string(yaml)
        .ok()
        .and_then(|text| Document::from_yaml_str(&text).ok())
        .filter(|document| crate::templates::is_path_like(&document.design.template))
        .map(|document| {
            yaml.parent()
                .unwrap_or(Path::new("."))
                .join(&document.design.template)
        });
    WatchTargets {
        yaml: yaml.to_path_buf(),
        yaml_dir: yaml.parent().unwrap_or(Path::new(".")).to_path_buf(),
        template_dir,
    }
}

/// Block for the first event, then drain the burst until `quiet` of silence. Only
/// arrival times matter, never event content (see module docs).
enum Burst {
    /// A burst was consumed; it is time to compare snapshots.
    Wake,
    Disconnected,
}

fn wait_for_burst(rx: &Receiver<notify::Result<notify::Event>>, quiet: Duration) -> Burst {
    if rx.recv().is_err() {
        return Burst::Disconnected;
    }
    loop {
        match rx.recv_timeout(quiet) {
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => return Burst::Wake,
            Err(RecvTimeoutError::Disconnected) => return Burst::Disconnected,
        }
    }
}

/// Move the folder watch to the current custom template, if any. A folder that does
/// not exist yet cannot be registered; it is picked up by the next sync (every burst
/// calls this), and its creation itself bursts through the watched YAML directory.
fn sync_template_watch(
    watcher: &mut RecommendedWatcher,
    targets: &WatchTargets,
    watched: &mut Option<PathBuf>,
) {
    let desired = targets
        .template_dir
        .as_ref()
        .filter(|dir| dir.is_dir())
        .cloned();
    if desired == *watched {
        return;
    }
    if let Some(old) = watched.take() {
        // We only unwatch paths we registered; failure still means it is gone from
        // our books, and the next sync re-derives the truth from `desired`.
        let _ = watcher.unwatch(&old);
    }
    if let Some(dir) = desired {
        match watcher.watch(&dir, RecursiveMode::Recursive) {
            Ok(()) => *watched = Some(dir),
            Err(e) => eprintln!(
                "warning: could not watch template folder '{}': {e}; changes there are picked up on the next YAML save",
                dir.display()
            ),
        }
    }
}

/// Content snapshot of the watched inputs: file → hash of its bytes, missing files
/// as `None`. A folder that does not exist becomes a single `dir → None` entry, so
/// its appearance is a change.
struct Snapshot(BTreeMap<PathBuf, Option<u64>>);

impl Snapshot {
    fn entry(&self, path: &Path) -> Option<&Option<u64>> {
        self.0.get(path)
    }

    fn differs(&self, other: &Self) -> bool {
        self.0 != other.0
    }
}

fn snapshot(targets: &WatchTargets) -> Snapshot {
    let mut files = BTreeMap::new();
    record(&mut files, &targets.yaml);
    if let Some(dir) = &targets.template_dir {
        record_tree(&mut files, dir);
    }
    Snapshot(files)
}

fn record(files: &mut BTreeMap<PathBuf, Option<u64>>, path: &Path) {
    files.insert(path.to_path_buf(), hash_file(path));
}

fn record_tree(files: &mut BTreeMap<PathBuf, Option<u64>>, dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        // Missing (or unreadable): one absent marker; content cannot be walked.
        files.insert(dir.to_path_buf(), None);
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            record_tree(files, &path);
        } else {
            record(files, &path);
        }
    }
}

/// Hash of the file's bytes, `None` when it cannot be read (missing counts as a
/// state, so deletion is detected too). Only compared within one process run, so
/// the fixed-seed default hasher is fine.
fn hash_file(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

/// One rebuild: build's own code path; its result line (or diagnostics) is the
/// watch's per-build output (spec §3). Errors print and the loop continues.
fn build_once(args: &BuildArgs, cli: &Cli) {
    if let Err(error) = crate::commands::build::run(args, cli) {
        eprint!(
            "{}",
            crate::error::report(&error, std::io::stderr().is_terminal())
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_yaml(dir: &Path, yaml: &str) -> PathBuf {
        std::fs::write(dir.join("cv.yaml"), yaml).expect("write yaml");
        dir.join("cv.yaml")
    }

    fn events_channel() -> (
        mpsc::Sender<notify::Result<notify::Event>>,
        Receiver<notify::Result<notify::Event>>,
    ) {
        mpsc::channel()
    }

    #[test]
    fn wait_for_burst_counts_one_save_burst_as_a_single_wake() {
        // Zero quiet period, sender kept alive: the drain consumes all three queued
        // events and returns on the first timeout — deterministic, no wall-clock
        // waiting (`.AGENTS.md` rule 18).
        let (tx, rx) = events_channel();
        for _ in 0..3 {
            tx.send(Ok(notify::Event::new(notify::EventKind::Any)))
                .expect("send");
        }
        assert!(matches!(wait_for_burst(&rx, Duration::ZERO), Burst::Wake));
        // The burst was fully drained: nothing left for the next wake.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn wait_for_burst_reports_a_disconnected_channel() {
        let (tx, rx) = events_channel();
        drop(tx);
        assert!(matches!(
            wait_for_burst(&rx, Duration::ZERO),
            Burst::Disconnected
        ));
    }

    #[test]
    fn watch_targets_follows_the_template_reference_kind() {
        let dir = tempfile::tempdir().expect("temp dir");
        let built_in = temp_yaml(
            dir.path(),
            "schema: 1\ncv:\n  name: X\ndesign:\n  template: compact\n",
        );
        let targets = watch_targets(&built_in);
        assert_eq!(targets.template_dir, None, "built-ins live in the binary");

        let custom = temp_yaml(
            dir.path(),
            "schema: 1\ncv:\n  name: X\ndesign:\n  template: ./tpl\n",
        );
        let targets = watch_targets(&custom);
        assert_eq!(
            targets.template_dir,
            Some(dir.path().join("tpl")),
            "relative references resolve against the YAML directory"
        );

        let broken = temp_yaml(dir.path(), "not even yaml: [");
        let targets = watch_targets(&broken);
        assert_eq!(
            targets.template_dir, None,
            "an unparseable file has no knowable template; the build reports it"
        );
    }

    #[test]
    fn the_snapshot_changes_only_for_watched_content() {
        let dir = tempfile::tempdir().expect("temp dir");
        let yaml = temp_yaml(dir.path(), "schema: 1\ncv:\n  name: X\n");
        std::fs::create_dir(dir.path().join("tpl")).expect("tpl dir");
        std::fs::write(dir.path().join("tpl/template.typ"), "old").expect("write");
        let targets = WatchTargets {
            yaml,
            yaml_dir: dir.path().to_path_buf(),
            template_dir: Some(dir.path().join("tpl")),
        };

        let initial = snapshot(&targets);
        assert!(!initial.differs(&snapshot(&targets)), "unchanged inputs");

        std::fs::write(dir.path().join("other.txt"), "noise").expect("write noise");
        assert!(
            !initial.differs(&snapshot(&targets)),
            "files outside the watch set must not count"
        );

        std::fs::write(dir.path().join("tpl/template.typ"), "new").expect("edit template");
        assert!(
            initial.differs(&snapshot(&targets)),
            "a template edit counts"
        );
    }

    #[test]
    fn a_missing_folder_then_its_creation_is_a_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let targets = WatchTargets {
            yaml: temp_yaml(dir.path(), "schema: 1\ncv:\n  name: X\n"),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: Some(dir.path().join("tpl")),
        };
        let before = snapshot(&targets);
        std::fs::create_dir(dir.path().join("tpl")).expect("create tpl");
        std::fs::write(dir.path().join("tpl/template.typ"), "x").expect("write");
        assert!(
            before.differs(&snapshot(&targets)),
            "the folder appearing must trigger a rebuild"
        );
    }

    #[test]
    fn a_deleted_yaml_is_a_change_too() {
        let dir = tempfile::tempdir().expect("temp dir");
        let targets = WatchTargets {
            yaml: temp_yaml(dir.path(), "schema: 1\ncv:\n  name: X\n"),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: None,
        };
        let before = snapshot(&targets);
        std::fs::remove_file(&targets.yaml).expect("delete yaml");
        assert!(
            before.differs(&snapshot(&targets)),
            "deletion must wake the loop (the build then reports the missing file)"
        );
    }

    #[test]
    fn absolute_input_resolves_relative_names_against_the_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("cv.yaml");
        let resolved = absolute_input(&missing).expect("dir exists, file may not");
        // tempdir paths can be symlinks (/var -> /private/var); identity is the
        // canonical directory plus the given file name.
        assert_eq!(
            resolved,
            dir.path()
                .canonicalize()
                .expect("canonical")
                .join("cv.yaml")
        );
    }

    #[test]
    fn absolute_input_names_the_missing_directory_on_error() {
        let err = absolute_input(Path::new("no-such-dir-xyz/cv.yaml")).expect_err("must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("no-such-dir-xyz"),
            "the error must name the directory: {msg}"
        );
    }

    #[test]
    fn absolute_input_rejects_references_without_a_file_name() {
        let err = absolute_input(Path::new(".")).expect_err("'.' has no file name");
        assert!(err.to_string().contains("not a file path"), "got: {err}");
    }
}
