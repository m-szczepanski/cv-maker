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

/// Watch one CV file until the process is stopped (Ctrl-C): build immediately,
/// then on every change of the watched content (spec §3).
///
/// Coverage note (`cargo llvm-cov`, rule 21): this function runs inside the
/// `cv watch` subprocess that the e2e tests in `tests/watch.rs` must SIGKILL
/// (the loop has no external stop signal by design, ADR-0029), so its profile
/// never flushes and only these glue lines read as uncovered. Every decision
/// the loop makes — wake/drain, snapshot equality, catch-up convergence,
/// target re-derivation, watch sync — is unit-tested below; the e2e tests
/// assert the end-to-end behaviour the glue produces.
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

        catch_up(
            &mut targets,
            &mut before,
            &mut || build_once(&build_args, cli),
            &mut |t| sync_template_watch(&mut watcher, t, &mut watched_template),
        );
    }
}

/// One wake's work: rebuild until the on-disk snapshot stops changing. A save
/// landing *during* a build is noticed by the next comparison, so the loop
/// converges on the newest content instead of leaving a stale PDF; every
/// iteration requires a real content change, so it terminates. Split from `run`
/// so the convergence behaviour is unit-testable without a live watcher.
fn catch_up(
    targets: &mut WatchTargets,
    before: &mut Snapshot,
    build: &mut dyn FnMut(),
    resync: &mut dyn FnMut(&WatchTargets),
) {
    loop {
        let after = snapshot(targets);
        if !after.differs(before) {
            return;
        }
        // A changed YAML can re-point design.template (built-in → folder or to a
        // different folder), so the reference is re-read after its rebuild.
        let yaml_changed = after.entry(&targets.yaml) != before.entry(&targets.yaml);

        build();
        *before = after;
        if yaml_changed {
            *targets = watch_targets(&targets.yaml);
        }
        resync(targets);
    }
}

/// What a rebuild can be caused by: the CV file and, if `design.template` is a path
/// reference, its template folder (spec §6: `cv watch` also watches the custom
/// template). Re-read after every YAML change so the reference can move.
#[derive(Clone)]
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
            // Watch failing on a folder that passed `is_dir` is a race (removed
            // in between) or an OS watch-limit fault; the build already states
            // everything about the folder, and the next sync retries.
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
    use clap::Parser;

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
    fn wait_for_burst_notices_a_sender_lost_mid_drain() {
        let (tx, rx) = events_channel();
        tx.send(Ok(notify::Event::new(notify::EventKind::Any)))
            .expect("send");
        drop(tx);
        // The first event arrives, then the sender dies during the drain: still
        // a clean Disconnected, never a hang.
        assert!(matches!(
            wait_for_burst(&rx, Duration::ZERO),
            Burst::Disconnected
        ));
    }

    #[test]
    fn catch_up_builds_once_for_one_change_and_then_converges() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut targets = WatchTargets {
            yaml: temp_yaml(dir.path(), "schema: 1\ncv:\n  name: A\n"),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: None,
        };
        let mut before = snapshot(&targets);
        let mut builds = 0;

        // Quiet disk: no build.
        catch_up(&mut targets, &mut before, &mut || builds += 1, &mut |_| {});
        assert_eq!(builds, 0, "nothing changed, nothing rebuilt");

        std::fs::write(&targets.yaml, "schema: 1\ncv:\n  name: B\n").expect("edit");
        catch_up(&mut targets, &mut before, &mut || builds += 1, &mut |_| {});
        assert_eq!(builds, 1, "one change, one rebuild");
        catch_up(&mut targets, &mut before, &mut || builds += 1, &mut |_| {});
        assert_eq!(builds, 1, "caught up: no further rebuild without a change");
    }

    #[test]
    fn catch_up_rebuilds_a_save_that_landed_during_the_build() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut targets = WatchTargets {
            yaml: temp_yaml(dir.path(), "schema: 1\ncv:\n  name: A\n"),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: None,
        };
        let mut before = snapshot(&targets);
        std::fs::write(&targets.yaml, "schema: 1\ncv:\n  name: B\n").expect("first edit");

        let yaml = targets.yaml.clone();
        let mut builds = 0;
        // The "build" itself takes time: mid-build, a second save lands. The
        // catch-up loop must notice it and rebuild again, then stop.
        catch_up(
            &mut targets,
            &mut before,
            &mut || {
                builds += 1;
                if builds == 1 {
                    std::fs::write(&yaml, "schema: 1\ncv:\n  name: C\n").expect("mid-build edit");
                }
            },
            &mut |_| {},
        );
        assert_eq!(builds, 2, "the mid-build save is caught up");
        let latest = snapshot(&targets);
        assert!(
            !latest.differs(&before),
            "before tracks the content the final build could have consumed"
        );
    }

    #[test]
    fn catch_up_repoints_targets_after_a_yaml_reference_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("tpl")).expect("tpl dir");
        let mut targets = WatchTargets {
            yaml: temp_yaml(
                dir.path(),
                "schema: 1\ncv:\n  name: X\ndesign:\n  template: compact\n",
            ),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: None,
        };
        let mut before = snapshot(&targets);
        std::fs::write(
            &targets.yaml,
            "schema: 1\ncv:\n  name: X\ndesign:\n  template: ./tpl\n",
        )
        .expect("re-point the template");

        let mut seen = None;
        catch_up(&mut targets, &mut before, &mut || {}, &mut |t| {
            seen = t.template_dir.clone()
        });
        assert_eq!(
            targets.template_dir,
            Some(dir.path().join("tpl")),
            "the YAML change re-derived the watch targets"
        );
        assert_eq!(
            seen,
            Some(dir.path().join("tpl")),
            "the resync saw the new targets (watch registration follows them)"
        );
    }

    #[test]
    fn sync_template_watch_registers_existing_folders_and_moves_with_them() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (tx, _rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx).expect("watcher starts");
        let mut watched = None;

        // No custom template: nothing to register.
        let built_in = WatchTargets {
            yaml: dir.path().join("cv.yaml"),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: None,
        };
        sync_template_watch(&mut watcher, &built_in, &mut watched);
        assert_eq!(watched, None);

        // Missing folder: not registered (its appearance wakes the loop later).
        let mut custom = WatchTargets {
            template_dir: Some(dir.path().join("tpl")),
            ..built_in.clone()
        };
        sync_template_watch(&mut watcher, &custom, &mut watched);
        assert_eq!(
            watched, None,
            "a folder that does not exist yet stays unwatched"
        );

        std::fs::create_dir(dir.path().join("tpl")).expect("create tpl");
        sync_template_watch(&mut watcher, &custom, &mut watched);
        assert_eq!(watched.as_deref(), Some(dir.path().join("tpl").as_ref()));

        // A second sync with the same state is a no-op (no churn).
        sync_template_watch(&mut watcher, &custom, &mut watched);
        assert_eq!(watched.as_deref(), Some(dir.path().join("tpl").as_ref()));

        // Moving the reference moves the registration; deleting it unregisters.
        std::fs::create_dir(dir.path().join("tpl2")).expect("create tpl2");
        custom.template_dir = Some(dir.path().join("tpl2"));
        sync_template_watch(&mut watcher, &custom, &mut watched);
        assert_eq!(watched.as_deref(), Some(dir.path().join("tpl2").as_ref()));

        custom.template_dir = None;
        sync_template_watch(&mut watcher, &custom, &mut watched);
        assert_eq!(watched, None);
    }

    #[test]
    fn nested_template_subdirectories_are_part_of_the_snapshot() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("tpl/lib")).expect("nested dir");
        std::fs::write(dir.path().join("tpl/lib/inline.typ"), "one").expect("write");
        let targets = WatchTargets {
            yaml: temp_yaml(dir.path(), "schema: 1\ncv:\n  name: X\n"),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: Some(dir.path().join("tpl")),
        };
        let before = snapshot(&targets);
        std::fs::write(dir.path().join("tpl/lib/inline.typ"), "two").expect("edit nested");
        assert!(
            before.differs(&snapshot(&targets)),
            "an edit deep in the folder must wake the rebuild"
        );
    }

    #[test]
    fn build_once_writes_the_pdf_and_survives_a_broken_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let yaml = temp_yaml(
            dir.path(),
            "schema: 1\ncv:\n  name: Watch Inner\ndesign:\n  template: compact\n",
        );
        let cli = Cli::try_parse_from(["cv", "--quiet", "build"]).expect("cli parses");
        let build_args = BuildArgs {
            file: yaml.clone(),
            output: None,
            no_fit: false,
        };
        build_once(&build_args, &cli);
        assert!(
            dir.path().join("cv.pdf").exists(),
            "a valid file builds through the watch path"
        );

        // A broken file reports (stderr, swallowed here) and does not panic:
        // exactly the property the watch loop relies on.
        std::fs::write(&yaml, "schema: 1\ncv:\n  name: [unclosed\n").expect("break");
        build_once(&build_args, &cli);
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
    fn absolute_input_falls_back_to_the_working_directory_for_a_bare_name() {
        // `cv watch` with the default `cv.yaml` has no parent component; "."
        // (the process working directory) is the anchor.
        let resolved = absolute_input(Path::new("cv.yaml")).expect("cwd always exists");
        assert_eq!(resolved.file_name().expect("has a name"), "cv.yaml");
        assert!(resolved.is_absolute());
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
