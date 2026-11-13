//! CLI surface tests (plan step 0.4: stubs must be real enough to test from day one).
//!
//! These tests invoke the real binary via `env!("CARGO_BIN_EXE_cv")`, so the shipped command
//! surface — not just the internal API — is exercised on every `cargo test` run.

use std::process::Command;

fn cv() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cv"))
}

#[test]
fn help_lists_every_command_from_spec_section_3() {
    let output = cv().arg("--help").output().expect("binary runs");
    assert!(output.status.success(), "exit code {:#?}", output.status);

    let stdout = String::from_utf8_lossy(&output.stdout);
    for command in [
        "init",
        "build",
        "watch",
        "check",
        "templates",
        "schema",
        "fonts",
    ] {
        assert!(
            stdout.contains(command),
            "`--help` must list `{command}` (spec §3); got:\n{stdout}"
        );
    }
}

#[test]
fn global_flags_are_listed() {
    let output = cv().arg("--help").output().expect("binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for flag in ["--font-path", "--quiet", "--verbose"] {
        assert!(
            stdout.contains(flag),
            "global flag `{flag}` missing from help"
        );
    }
}

#[test]
fn build_exposes_its_flags() {
    let output = cv()
        .args(["build", "--help"])
        .output()
        .expect("binary runs");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for flag in ["--output", "--no-fit"] {
        assert!(
            stdout.contains(flag),
            "`build` flag `{flag}` missing from help"
        );
    }
}

#[test]
fn init_exposes_its_flags() {
    let output = cv().args(["init", "--help"]).output().expect("binary runs");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for flag in ["--template", "--out", "--force"] {
        assert!(
            stdout.contains(flag),
            "`init` flag `{flag}` missing from help"
        );
    }
}

#[test]
fn unimplemented_commands_exit_non_zero_and_say_so() {
    for args in [["templates"], ["schema"], ["fonts"]] {
        let output = cv().args(args).output().expect("binary runs");
        assert!(
            !output.status.success(),
            "`cv {}` must not pretend to succeed",
            args[0]
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("not implemented yet"),
            "`cv {}` must state it is unimplemented; stderr:\n{stderr}",
            args[0]
        );
    }
}
