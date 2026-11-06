//! The command line of the real binary, on every system: help, audio files rendered and
//! compared on the dummy backend, the device list, keys, plugin scans and the MCP server.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use audiostrada_media::{AudioData, write_wav};

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_audiostrada"))
}

fn run(args: &[&str]) -> Output {
    Command::new(exe())
        .args(args)
        .env("AUDIOSTRADA_LOG", "warn")
        .output()
        .expect("the program runs")
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("audiostrada-cli-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    dir
}

fn tone(path: &Path, amplitude: f32) {
    let samples: Vec<f32> = (0..24_000)
        .map(|i| (i as f32 * 0.03).sin() * amplitude)
        .collect();
    write_wav(
        path,
        &AudioData::new(vec![samples.clone(), samples], 48_000),
    )
    .expect("written");
}

#[test]
fn help_and_errors() {
    let help = run(&["help"]);
    assert!(help.status.success());
    assert!(text(&help).contains("render"), "{}", text(&help));
    // A word is a file to open; an unknown option is an error.
    let wrong = run(&["--no-such-option"]);
    assert!(!wrong.status.success());
    assert!(
        !run(&["render", "a.wav"]).status.success(),
        "render needs --output"
    );
    assert!(
        !run(&["plugin-scan", "clap"]).status.success(),
        "a file is needed"
    );
}

#[test]
fn files_render_and_compare() {
    let dir = folder("render");
    let (a, b, out) = (dir.join("a.wav"), dir.join("b.wav"), dir.join("mix.wav"));
    tone(&a, 0.3);
    tone(&b, 0.1);
    let render = run(&[
        "render",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    assert!(render.status.success(), "{}", text(&render));
    assert!(std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0) > 1000);
    let same = run(&["compare", a.to_str().unwrap(), a.to_str().unwrap()]);
    assert!(same.status.success(), "{}", text(&same));
    assert!(text(&same).contains("Identical"));
    let different = run(&["compare", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert!(!text(&different).contains("Identical"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_dummy_devices_are_listed() {
    let devices = run(&["devices", "--backend", "dummy"]);
    assert!(devices.status.success(), "{}", text(&devices));
    assert!(run(&["diagnose"]).status.success());
}

#[test]
fn keys_are_made() {
    let keys = run(&["license", "keygen"]);
    assert!(keys.status.success());
    let out = text(&keys);
    assert!(out.contains("secret ") && out.contains("public "), "{out}");
}

#[test]
fn the_test_plugin_is_scanned_in_its_own_process() {
    let file =
        audiostrada_test_plugin::built_library().expect("the test plugin is built with the tests");
    let scan = run(&["plugin-scan", "clap", file.to_str().unwrap()]);
    assert!(scan.status.success(), "{}", text(&scan));
    assert!(
        text(&scan).contains("com.audiostrada.test-gain"),
        "{}",
        text(&scan)
    );
}

#[test]
fn the_mcp_server_answers_on_stdio() {
    let mut child = Command::new(exe())
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the server starts");
    let mut stdin = child.stdin.take().expect("its input");
    for message in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"inspect_project","arguments":{}}}"#,
    ] {
        writeln!(stdin, "{message}").expect("sent");
    }
    drop(stdin);
    let output = child.wait_with_output().expect("it ends with its input");
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(
        out.contains("\"id\":2") && out.contains("inspect_project"),
        "{out}"
    );
    assert!(out.contains("\"id\":3"), "{out}");
}
