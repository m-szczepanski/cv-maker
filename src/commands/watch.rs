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
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::cli::{BuildArgs, Cli, WatchArgs};
use crate::error::{Error, Result};
use crate::model::Document;

/// Quiet period that collapses one save's event burst into a single rebuild (the
/// `notify` debouncing note of spec §2, implemented here without a second crate —
/// `.AGENTS.md` rule 22).
const DEBOUNCE_QUIET: Duration = Duration::from_millis(100);

/// The two watcher operations the loop performs, as a seam: a real `notify` watcher
/// refuses a folder that exists only on OS watch slots, which no test can arrange,
/// while the messages that explain it to the user must have tests (rule 16).
trait WatchFolders {
    fn watch_folder(&mut self, dir: &Path, mode: RecursiveMode) -> notify::Result<()>;
    fn unwatch_folder(&mut self, dir: &Path) -> notify::Result<()>;
}

impl WatchFolders for RecommendedWatcher {
    fn watch_folder(&mut self, dir: &Path, mode: RecursiveMode) -> notify::Result<()> {
        self.watch(dir, mode)
    }

    fn unwatch_folder(&mut self, dir: &Path) -> notify::Result<()> {
        self.unwatch(dir)
    }
}

type WatcherHandle = Box<dyn WatchFolders>;

/// The OS boundary of `cv watch`: turn an event sender into a watcher. `run` uses the
/// platform backend; a test hands in one that refuses or one that drops its sender, so
/// the refusal messages and the loop's exit are covered without a killed subprocess.
type WatcherFactory = fn(Sender<notify::Result<notify::Event>>) -> notify::Result<WatcherHandle>;

fn platform_watcher(tx: Sender<notify::Result<notify::Event>>) -> notify::Result<WatcherHandle> {
    notify::recommended_watcher(tx).map(|watcher| Box::new(watcher) as WatcherHandle)
}

/// Watch one CV file until the process is stopped (Ctrl-C): build immediately, then on
/// every change of the watched content (spec §3).
pub fn run(args: &WatchArgs, cli: &Cli) -> Result<()> {
    watch(args, cli, platform_watcher)
}

/// Start a session on the given watcher backend and run its loop.
fn watch(args: &WatchArgs, cli: &Cli, make_watcher: WatcherFactory) -> Result<()> {
    let mut session = Watch::start(args, cli, make_watcher)?;
    session.run()
}

/// The live watch session: the OS watcher and its event channel, what to rebuild, the
/// snapshot the last build consumed, and the template folder currently registered.
struct Watch<'a> {
    watcher: WatcherHandle,
    rx: Receiver<notify::Result<notify::Event>>,
    build_args: BuildArgs,
    targets: WatchTargets,
    before: Snapshot,
    watched_template: Option<PathBuf>,
    cli: &'a Cli,
}

impl<'a> Watch<'a> {
    /// A session over an existing event channel, baseline snapshot included: the
    /// snapshot is taken *before* the first build, so a save landing while a build
    /// runs is caught up (spec §3, ADR-0029).
    fn new(
        watcher: WatcherHandle,
        rx: Receiver<notify::Result<notify::Event>>,
        build_args: BuildArgs,
        targets: WatchTargets,
        watched_template: Option<PathBuf>,
        cli: &'a Cli,
    ) -> Self {
        let before = snapshot(&targets);
        Self {
            watcher,
            rx,
            build_args,
            targets,
            before,
            watched_template,
            cli,
        }
    }

    /// Register the watcher, build once, and print the banner (spec §3: `cv watch`
    /// starts from a current PDF).
    fn start(args: &WatchArgs, cli: &'a Cli, make_watcher: WatcherFactory) -> Result<Self> {
        let yaml = absolute_input(&args.file)?;
        // One rebuild is exactly one `cv build` run with the watch defaults: the
        // resolved output path is `<stem>.pdf` next to the input (spec §3).
        let build_args = BuildArgs {
            file: args.file.clone(),
            output: None,
            no_fit: false,
        };
        let targets = watch_targets(&yaml);
        let (tx, rx) = mpsc::channel();
        let mut watcher = make_watcher(tx)
            .map_err(|e| Error::Internal(format!("could not start file watching: {e}")))?;
        watch_folder(
            watcher.as_mut(),
            &targets.yaml_dir,
            RecursiveMode::NonRecursive,
        )?;
        let mut watched_template = None;
        sync_template_watch(watcher.as_mut(), &targets, &mut watched_template);

        let session = Self::new(watcher, rx, build_args, targets, watched_template, cli);
        build_once(&session.build_args, cli);
        if !cli.quiet {
            super::out_line(&format!(
                "watching {}, press Ctrl-C to stop",
                args.file.display()
            ));
        }
        Ok(session)
    }

    /// The watch loop: one turn per burst until the event channel dies. Ctrl-C ends
    /// the process; the disconnect is the only in-process exit (ADR-0029) — the
    /// channel died while the watcher is still owned, a broken notify internal rather
    /// than a user situation, so reporting beats silence.
    fn run(&mut self) -> Result<()> {
        loop {
            self.turn(DEBOUNCE_QUIET)?;
        }
    }

    /// Drain one burst and rebuild until the on-disk snapshot stops changing.
    /// `quiet` is the debounce window; tests pass zero to skip the wait.
    fn turn(&mut self, quiet: Duration) -> Result<()> {
        match wait_for_burst(&self.rx, quiet) {
            Burst::Wake => {}
            Burst::Disconnected => {
                return Err(Error::Internal("file watcher stopped".to_owned()));
            }
        }
        let cli = self.cli;
        let Watch {
            targets,
            before,
            watcher,
            watched_template,
            build_args,
            ..
        } = self;
        catch_up(
            targets,
            before,
            &mut || build_once(build_args, cli),
            &mut |t| sync_template_watch(watcher.as_mut(), t, watched_template),
        );
        Ok(())
    }
}

/// Point a watcher at a folder, or name the folder that could not be watched.
fn watch_folder(watcher: &mut dyn WatchFolders, dir: &Path, mode: RecursiveMode) -> Result<()> {
    watcher
        .watch_folder(dir, mode)
        .map_err(|e| watch_registration_error(dir, &e))
}

/// The startup failure when the CV's directory cannot be watched: an OS watch-slot or
/// permission fault, so the message names the directory (rule 16).
fn watch_registration_error(dir: &Path, error: &notify::Error) -> Error {
    Error::Internal(format!("could not watch '{}': {error}", dir.display()))
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
#[derive(Debug, PartialEq, Eq)]
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
    watcher: &mut dyn WatchFolders,
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
        let _ = watcher.unwatch_folder(&old);
    }
    if let Some(dir) = desired {
        match watcher.watch_folder(&dir, RecursiveMode::Recursive) {
            Ok(()) => *watched = Some(dir),
            // Watch failing on a folder that passed `is_dir` is a race (removed in
            // between) or an OS watch-slot fault; the build already states everything
            // about the folder, and the next sync retries (ADR-0029).
            Err(error) => eprint!("{}", unwatchable_folder(&dir, &error)),
        }
    }
}

/// The warning for a custom template folder the OS would not let us watch (ADR-0029):
/// loud enough to explain, not a reason to stop watching.
fn unwatchable_folder(dir: &Path, error: &notify::Error) -> String {
    format!(
        "warning: could not watch template folder '{}': {error}; changes there are picked up on the next YAML save\n",
        dir.display()
    )
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

    /// A folder watch that records calls and can be made to refuse: the OS watch-slot
    /// fault that no temp directory or real backend can be forced to produce.
    struct TestFolders {
        refuse: bool,
        watched: Vec<PathBuf>,
        unwatched: Vec<PathBuf>,
    }

    impl TestFolders {
        /// Registration succeeds, and every call is recorded.
        fn new() -> Self {
            Self {
                refuse: false,
                watched: Vec::new(),
                unwatched: Vec::new(),
            }
        }

        /// Registration always fails: the OS ran out of watch slots.
        fn refusing() -> Self {
            Self {
                refuse: true,
                ..Self::new()
            }
        }
    }

    impl WatchFolders for TestFolders {
        fn watch_folder(&mut self, dir: &Path, _mode: RecursiveMode) -> notify::Result<()> {
            self.watched.push(dir.to_path_buf());
            if self.refuse {
                return Err(notify::Error::generic("inotify watch limit reached"));
            }
            Ok(())
        }

        fn unwatch_folder(&mut self, dir: &Path) -> notify::Result<()> {
            self.unwatched.push(dir.to_path_buf());
            Ok(())
        }
    }

    /// A backend that refuses every folder: the watch-slot fault at startup.
    fn refusing_backend(
        _tx: Sender<notify::Result<notify::Event>>,
    ) -> notify::Result<WatcherHandle> {
        Ok(Box::new(TestFolders::refusing()))
    }

    /// A backend that cannot even be created.
    fn broken_backend(_tx: Sender<notify::Result<notify::Event>>) -> notify::Result<WatcherHandle> {
        Err(notify::Error::generic("too many open files"))
    }

    /// A working backend that drops the event sender: the loop's only exit, reached
    /// without killing anything.
    fn silent_backend(tx: Sender<notify::Result<notify::Event>>) -> notify::Result<WatcherHandle> {
        drop(tx);
        Ok(Box::new(TestFolders::new()))
    }

    /// A session whose event channel the test owns: `tx` injects the wakes and
    /// dropping it ends the loop. The folder watch is the recording double, so no
    /// filesystem timing is involved (rule 18).
    fn session<'a>(
        yaml: &Path,
        cli: &'a Cli,
    ) -> (mpsc::Sender<notify::Result<notify::Event>>, Watch<'a>) {
        let (tx, rx) = mpsc::channel();
        let watcher: WatcherHandle = Box::new(TestFolders::new());
        let build_args = BuildArgs {
            file: yaml.to_path_buf(),
            output: None,
            no_fit: false,
        };
        let targets = watch_targets(yaml);
        (tx, Watch::new(watcher, rx, build_args, targets, None, cli))
    }

    fn quiet_cli() -> Cli {
        Cli::try_parse_from(["cv", "--quiet", "watch"]).expect("cli parses")
    }

    #[test]
    fn a_wake_rebuilds_only_after_a_content_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let yaml = temp_yaml(dir.path(), "schema: 1\ncv:\n  name: Wake A\n");
        let cli = quiet_cli();
        let (tx, mut watch) = session(&yaml, &cli);

        tx.send(Ok(notify::Event::new(notify::EventKind::Any)))
            .expect("send");
        watch.turn(Duration::ZERO).expect("one turn");
        assert!(
            !dir.path().join("cv.pdf").exists(),
            "an unchanged snapshot must not rebuild"
        );

        std::fs::write(&yaml, "schema: 1\ncv:\n  name: Wake B\n").expect("edit");
        tx.send(Ok(notify::Event::new(notify::EventKind::Any)))
            .expect("send");
        watch.turn(Duration::ZERO).expect("one turn");
        assert!(
            dir.path().join("cv.pdf").exists(),
            "a content change is rebuilt through the wake"
        );
    }

    #[test]
    fn the_watch_loop_ends_when_the_event_channel_dies() {
        let dir = tempfile::tempdir().expect("temp dir");
        let yaml = temp_yaml(dir.path(), "schema: 1\ncv:\n  name: Loop End\n");
        let cli = quiet_cli();
        let (tx, mut watch) = session(&yaml, &cli);
        drop(tx);
        let err = watch.run().expect_err("a dead channel must end the loop");
        assert!(
            err.to_string().contains("file watcher stopped"),
            "got: {err}"
        );
    }

    #[test]
    fn start_builds_once_and_prints_the_banner() {
        let dir = tempfile::tempdir().expect("temp dir");
        let yaml = temp_yaml(dir.path(), "schema: 1\ncv:\n  name: Startup Build\n");
        let cli = Cli::try_parse_from(["cv", "watch"]).expect("cli parses");
        let args = WatchArgs { file: yaml.clone() };
        let watch = Watch::start(&args, &cli, platform_watcher).expect("the watch starts");
        assert!(
            dir.path().join("cv.pdf").exists(),
            "`cv watch` starts from a current PDF (spec §3)"
        );
        drop(watch);
    }

    /// `--quiet` drops the progress banner but not the startup build (ADR-0012).
    #[test]
    fn a_quiet_start_builds_without_the_banner() {
        let dir = tempfile::tempdir().expect("temp dir");
        let yaml = temp_yaml(dir.path(), "schema: 1\ncv:\n  name: Quiet Start\n");
        let args = WatchArgs { file: yaml };
        let cli = quiet_cli();
        let watch = Watch::start(&args, &cli, platform_watcher).expect("the watch starts");
        assert!(
            dir.path().join("cv.pdf").exists(),
            "quiet still builds the current PDF once"
        );
        drop(watch);
    }

    #[test]
    fn run_reports_a_missing_directory_before_watching_anything() {
        let cli = quiet_cli();
        let args = WatchArgs {
            file: PathBuf::from("no-such-watch-dir/cv.yaml"),
        };
        let err = run(&args, &cli).expect_err("startup must fail");
        assert!(
            err.to_string().contains("no-such-watch-dir"),
            "the error names the directory: {err}"
        );
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
        assert_eq!(wait_for_burst(&rx, Duration::ZERO), Burst::Wake);
        // The burst was fully drained: nothing left for the next wake.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn wait_for_burst_reports_a_disconnected_channel() {
        let (tx, rx) = events_channel();
        drop(tx);
        assert_eq!(wait_for_burst(&rx, Duration::ZERO), Burst::Disconnected);
    }

    #[test]
    fn wait_for_burst_notices_a_sender_lost_mid_drain() {
        let (tx, rx) = events_channel();
        tx.send(Ok(notify::Event::new(notify::EventKind::Any)))
            .expect("send");
        drop(tx);
        // The first event arrives, then the sender dies during the drain: still
        // a clean Disconnected, never a hang.
        assert_eq!(wait_for_burst(&rx, Duration::ZERO), Burst::Disconnected);
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
        let builds = std::cell::Cell::new(0);
        let resyncs = std::cell::Cell::new(0);
        let mut build = || builds.set(builds.get() + 1);
        let mut resync = |_: &WatchTargets| resyncs.set(resyncs.get() + 1);

        // Quiet disk: no build.
        catch_up(&mut targets, &mut before, &mut build, &mut resync);
        assert_eq!(builds.get(), 0, "nothing changed, nothing rebuilt");

        std::fs::write(&targets.yaml, "schema: 1\ncv:\n  name: B\n").expect("edit");
        catch_up(&mut targets, &mut before, &mut build, &mut resync);
        assert_eq!(builds.get(), 1, "one change, one rebuild");
        catch_up(&mut targets, &mut before, &mut build, &mut resync);
        assert_eq!(
            builds.get(),
            1,
            "caught up: no further rebuild without a change"
        );
        assert_eq!(
            resyncs.get(),
            builds.get(),
            "every rebuild re-checks the watch set, and nothing else does"
        );
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

    /// A template-only edit rebuilds, and the YAML-level target re-derivation is
    /// skipped: the reference cannot have moved if the file did not change.
    #[test]
    fn catch_up_rebuilds_a_template_change_without_re_deriving_the_targets() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("tpl")).expect("tpl dir");
        std::fs::write(dir.path().join("tpl/template.typ"), "one").expect("write source");
        let mut targets = WatchTargets {
            yaml: temp_yaml(dir.path(), "schema: 1\ncv:\n  name: X\n"),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: Some(dir.path().join("tpl")),
        };
        let mut before = snapshot(&targets);
        std::fs::write(dir.path().join("tpl/template.typ"), "two").expect("edit template");

        let mut builds = 0;
        let mut resynced = None;
        catch_up(&mut targets, &mut before, &mut || builds += 1, &mut |t| {
            resynced = t.template_dir.clone()
        });
        assert_eq!(builds, 1, "a template edit is a rebuild");
        assert_eq!(
            resynced, targets.template_dir,
            "an unchanged YAML keeps the targets it had"
        );
    }

    #[test]
    fn sync_template_watch_registers_existing_folders_and_moves_with_them() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut folders = TestFolders::new();
        let mut watched: Option<PathBuf> = None;

        // No custom template: nothing to register.
        let built_in = WatchTargets {
            yaml: dir.path().join("cv.yaml"),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: None,
        };
        sync_template_watch(&mut folders, &built_in, &mut watched);
        assert_eq!(watched, None);

        // Missing folder: not registered (its appearance wakes the loop later).
        let mut custom = WatchTargets {
            template_dir: Some(dir.path().join("tpl")),
            ..built_in.clone()
        };
        sync_template_watch(&mut folders, &custom, &mut watched);
        assert_eq!(
            watched, None,
            "a folder that does not exist yet stays unwatched"
        );

        std::fs::create_dir(dir.path().join("tpl")).expect("create tpl");
        sync_template_watch(&mut folders, &custom, &mut watched);
        assert_eq!(watched.as_deref(), Some(dir.path().join("tpl").as_ref()));

        // A second sync with the same state is a no-op (no churn).
        let before = folders.watched.len();
        sync_template_watch(&mut folders, &custom, &mut watched);
        assert_eq!(
            folders.watched.len(),
            before,
            "an unchanged reference must not re-register"
        );

        // Moving the reference moves the registration; deleting it unregisters.
        std::fs::create_dir(dir.path().join("tpl2")).expect("create tpl2");
        custom.template_dir = Some(dir.path().join("tpl2"));
        sync_template_watch(&mut folders, &custom, &mut watched);
        assert_eq!(watched.as_deref(), Some(dir.path().join("tpl2").as_ref()));
        assert_eq!(
            folders.unwatched,
            [dir.path().join("tpl")],
            "the folder the reference left is unregistered"
        );

        custom.template_dir = None;
        sync_template_watch(&mut folders, &custom, &mut watched);
        assert_eq!(watched, None);
    }

    #[test]
    fn a_refused_backend_is_a_startup_failure_that_names_the_cause() {
        let dir = tempfile::tempdir().expect("temp dir");
        let yaml = temp_yaml(dir.path(), "schema: 1\ncv:\n  name: Broken Backend\n");
        let args = WatchArgs { file: yaml };
        let cli = quiet_cli();
        let err = watch(&args, &cli, broken_backend).expect_err("no watcher, no watch");
        assert!(
            err.to_string().contains("too many open files")
                && err.to_string().contains("could not start file watching"),
            "the error explains the OS cause: {err}"
        );
    }

    #[test]
    fn a_refused_directory_watch_names_the_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let yaml = temp_yaml(dir.path(), "schema: 1\ncv:\n  name: Refused Dir\n");
        let args = WatchArgs { file: yaml };
        let cli = quiet_cli();
        let err = watch(&args, &cli, refusing_backend).expect_err("registration refused");
        assert!(
            err.to_string().contains(&dir.path().display().to_string())
                && err.to_string().contains("inotify watch limit reached"),
            "the error names the directory and the cause: {err}"
        );
    }

    #[test]
    fn the_loop_returns_when_the_backend_drops_the_channel() {
        let dir = tempfile::tempdir().expect("temp dir");
        let yaml = temp_yaml(dir.path(), "schema: 1\ncv:\n  name: Dead Channel\n");
        let args = WatchArgs { file: yaml };
        let cli = quiet_cli();
        let err = watch(&args, &cli, silent_backend).expect_err("the loop must end");
        assert!(
            err.to_string().contains("file watcher stopped"),
            "got: {err}"
        );
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

    /// The same registration dance on the platform backend: the trait methods that
    /// production actually calls are exercised, not only the test double's.
    #[test]
    fn the_platform_backend_registers_and_moves_folder_watches() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (tx, _rx) = mpsc::channel();
        let mut folders = platform_watcher(tx).expect("the platform watcher starts");
        let built_in = WatchTargets {
            yaml: dir.path().join("cv.yaml"),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: None,
        };
        let mut watched: Option<PathBuf> = None;

        std::fs::create_dir(dir.path().join("tpl")).expect("create tpl");
        let custom = WatchTargets {
            template_dir: Some(dir.path().join("tpl")),
            ..built_in.clone()
        };
        sync_template_watch(folders.as_mut(), &custom, &mut watched);
        assert_eq!(
            watched.as_deref(),
            Some(dir.path().join("tpl").as_ref()),
            "the real backend accepts an existing folder"
        );

        // Moving the reference unregisters the old folder through the real backend.
        let moved = WatchTargets {
            template_dir: None,
            ..custom
        };
        sync_template_watch(folders.as_mut(), &moved, &mut watched);
        assert_eq!(watched, None, "the registration is released");
    }

    #[test]
    fn a_refused_folder_watch_warns_and_leaves_it_unregistered() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("tpl")).expect("create tpl");
        let stale = dir.path().join("other");
        let targets = WatchTargets {
            yaml: temp_yaml(dir.path(), "schema: 1\ncv:\n  name: X\n"),
            yaml_dir: dir.path().to_path_buf(),
            template_dir: Some(dir.path().join("tpl")),
        };
        let mut folders = TestFolders::refusing();
        let mut watched = Some(stale.clone());
        sync_template_watch(&mut folders, &targets, &mut watched);

        assert_eq!(
            folders.unwatched,
            [stale],
            "the old registration is dropped"
        );
        assert_eq!(
            watched, None,
            "a folder that could not be registered is not remembered as watched"
        );

        let warning = unwatchable_folder(
            Path::new("./my-template"),
            &notify::Error::generic("inotify watch limit reached"),
        );
        assert!(
            warning.contains("./my-template")
                && warning.contains("inotify watch limit reached")
                && warning.contains("next YAML save"),
            "the warning names the folder, the cause and the way out: {warning}"
        );
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
