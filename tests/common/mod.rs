#![allow(dead_code)]

use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

pub static TEMP_SOURCE_ID: AtomicUsize = AtomicUsize::new(0);

pub fn run_example(path: &str) -> String {
    let binary = env!("CARGO_BIN_EXE_simply");
    let output = Command::new(binary)
        .args(["run", path])
        .output()
        .expect("failed to run Simply example");
    assert!(output.status.success(), "example failed: {path}");
    String::from_utf8(output.stdout).expect("example output was not UTF-8")
}

pub fn run_source(source: &str) -> (bool, String) {
    let source_id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("simply-test-{}-{source_id}.si", std::process::id()));
    fs::write(&path, source).expect("failed to write temporary Simply source");
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", path.to_str().expect("temporary path was not UTF-8")])
        .output()
        .expect("failed to run temporary Simply source");
    let _ = fs::remove_file(path);
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.success(), message)
}

pub fn run_source_stdout(source: &str) -> (bool, String) {
    let source_id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "simply-stdout-{}-{source_id}.si",
        std::process::id()
    ));
    fs::write(&path, source).expect("failed to write temporary Simply source");
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", path.to_str().expect("temporary path was not UTF-8")])
        .output()
        .expect("failed to run temporary Simply source");
    let _ = fs::remove_file(path);
    let message = String::from_utf8_lossy(&output.stdout).into_owned();
    (output.status.success(), message)
}

pub fn check_source(source: &str) -> (bool, String, String) {
    let source_id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "simply-check-{}-{source_id}.si",
        std::process::id()
    ));
    fs::write(&path, source).expect("failed to write temporary Simply source");
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "check",
            path.to_str().expect("temporary path was not UTF-8"),
        ])
        .output()
        .expect("failed to run Simply check");
    let _ = fs::remove_file(path);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

pub fn explain_flow_source(source: &str) -> (bool, String) {
    let source_id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "simply-explain-flow-{}-{source_id}.si",
        std::process::id()
    ));
    fs::write(&path, source).expect("failed to write temporary Simply source");
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "explain-flow",
            path.to_str().expect("temporary path was not UTF-8"),
        ])
        .output()
        .expect("failed to run Simply explain-flow");
    let _ = fs::remove_file(path);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

pub fn run_test_project(files: &[(&str, &str)]) -> (bool, String, String) {
    let project_id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "simply-test-project-{}-{project_id}",
        std::process::id()
    ));
    for (relative_path, source) in files {
        let path = root.join(relative_path);
        fs::create_dir_all(path.parent().expect("test file should have a parent"))
            .expect("failed to create test project directory");
        fs::write(path, source).expect("failed to write test project file");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg("test")
        .current_dir(&root)
        .output()
        .expect("failed to run Simply test command");
    let _ = fs::remove_dir_all(root);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}
