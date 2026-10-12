#[test]
fn bridge_forwards_native_titles_on_creation_rename_and_resume() {
    let output = match std::process::Command::new("node")
        .args([
            "--no-warnings",
            "--experimental-vm-modules",
            "tests/opencode_bridge.test.mjs",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("Node is unavailable; skipping the JavaScript bridge fixture");
            return;
        }
        Err(error) => panic!("cannot run bridge fixture: {error}"),
    };
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
