//! End-to-end smoke tests for the `telink-ota` binary: argument handling and
//! error paths only. No hardware, no device, so this runs anywhere.

use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_telink-ota");
const MISSING_DEVICE: &str = "/nonexistent/telink-ota-smoke/hidraw99";

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run telink-ota")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn no_args_prints_usage_and_exits_2() {
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("usage:"));
}

#[test]
fn unknown_command_prints_usage_and_exits_2() {
    let out = run(&["/dev/null", "bogus"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("usage:"));
}

#[test]
fn version_on_missing_device_fails_to_open() {
    let out = run(&[MISSING_DEVICE, "version"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains(MISSING_DEVICE));
}

#[test]
fn flash_without_confirm_exits_before_opening_the_device() {
    let out = run(&[MISSING_DEVICE, "flash", "firmware.bin"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--confirm-flash"));
}

#[test]
fn flash_with_confirm_on_a_missing_device_fails_to_open() {
    let out = run(&[MISSING_DEVICE, "flash", "firmware.bin", "--confirm-flash"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains(MISSING_DEVICE));
}

#[test]
fn version_refuses_a_regular_file_without_writing_to_it() {
    let path = std::env::temp_dir().join(format!("telink-ota-not-a-device-{}", std::process::id()));
    std::fs::write(&path, b"untouched").unwrap();

    let out = run(&[path.to_str().unwrap(), "version"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read(&path).unwrap(), b"untouched", "the file must not be written to");
    std::fs::remove_file(&path).ok();
}

// /dev/null is a character device but not a hidraw node, so it must be refused
// before the version report is written to it.
#[cfg(target_os = "linux")]
#[test]
fn version_refuses_a_character_device_that_is_not_hidraw() {
    let out = run(&["/dev/null", "version"]);

    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("hidraw"));
}
