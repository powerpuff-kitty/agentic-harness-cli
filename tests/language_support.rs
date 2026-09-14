use serde_json::Value;
use std::process::Command;

#[test]
fn architecture_languages_reports_honest_capabilities() {
    let output = Command::new(env!("CARGO_BIN_EXE_ah"))
        .args(["architecture", "languages"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["format_version"], 1);
    assert_eq!(value["kind"], "language-support-matrix");

    let frontends = value["frontends"].as_array().unwrap();
    let js = frontends
        .iter()
        .find(|entry| entry["language"] == "javascript-typescript")
        .unwrap();
    assert_eq!(js["implementation"], "oxc");
    assert!(
        js["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability["capability"] == "parse"
                && capability["support"] == "supported")
    );

    for language in ["python", "rust", "go"] {
        let frontend = frontends
            .iter()
            .find(|entry| entry["language"] == language)
            .unwrap();
        assert!(
            frontend["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .all(|capability| capability["support"] == "unsupported")
        );
    }
}

#[test]
fn architecture_languages_rejects_extra_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_ah"))
        .args(["architecture", "languages", "extra"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
