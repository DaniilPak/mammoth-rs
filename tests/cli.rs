//! Tests for the `mammoth-rs` command-line tool.
#![cfg(feature = "cli")]

use std::process::Command;

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/mammoth-js/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn mammoth() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mammoth-rs"))
}

#[test]
fn converts_to_stdout() {
    let output = mammoth()
        .arg(fixture("single-paragraph.docx"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "<p>Walking on imported air</p>"
    );
}

#[test]
fn converts_to_markdown() {
    let output = mammoth()
        .arg(fixture("simple-list.docx"))
        .arg("--output-format=markdown")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "- Apple\n- Banana\n\n"
    );
}

#[test]
fn output_dir_saves_images_as_files() {
    let dir = std::env::temp_dir().join(format!("mammoth-rs-cli-test-{}", std::process::id()));
    let status = mammoth()
        .arg(fixture("tiny-picture.docx"))
        .arg(format!("--output-dir={}", dir.display()))
        .status()
        .unwrap();
    assert!(status.success());
    let html = std::fs::read_to_string(dir.join("tiny-picture.html")).unwrap();
    assert_eq!(html, "<p><img src=\"1.png\" /></p>");
    assert_eq!(
        std::fs::read(dir.join("1.png")).unwrap(),
        std::fs::read(fixture("tiny-picture.png")).unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_input_is_a_usage_error() {
    let output = mammoth().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}
