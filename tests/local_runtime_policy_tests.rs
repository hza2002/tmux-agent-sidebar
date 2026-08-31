use std::fs;

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

#[test]
fn local_runtime_never_checks_or_downloads_releases() {
    let installer = read("install-wizard.sh");
    let app = read("src/app.rs");
    let version = read("src/version.rs");
    let workers = read("src/app/workers.rs");
    let tmux_launcher = read("tmux-agent-sidebar.tmux");
    let hook_launcher = read("hook.sh");

    assert!(installer.contains("cargo build --release"));
    assert!(version.contains("pub struct UpdateNotice"));
    assert!(workers.contains("session_poll_loop"));

    for (path, contents) in [
        ("install-wizard.sh", installer),
        ("src/app.rs", app),
        ("src/version.rs", version),
        ("src/app/workers.rs", workers),
        ("tmux-agent-sidebar.tmux", tmux_launcher),
        ("hook.sh", hook_launcher),
    ] {
        assert!(
            !contents.contains("releases/latest")
                && !contents.contains("api.github.com/repos")
                && !contents.contains("download-binary")
                && !contents.contains("fetch_update_notice"),
            "{path} must not contain a remote update path"
        );
    }
}

#[test]
fn maintained_docs_do_not_restore_remote_runtime_installation() {
    let readme = read("README.md");
    let home = read("website/src/content/docs/index.mdx");
    let installation = read("website/src/content/docs/getting-started/installation.md");

    for (path, contents) in [
        ("README.md", readme),
        ("website/src/content/docs/index.mdx", home),
        (
            "website/src/content/docs/getting-started/installation.md",
            installation,
        ),
    ] {
        assert!(contents.contains(".config/tmux/plugins/tmux-agent-sidebar"));
        assert!(
            !contents.contains("set -g @plugin 'hiroppy/tmux-agent-sidebar'")
                && !contents.contains("set -g @plugin 'hza2002/tmux-agent-sidebar'")
                && !contents.contains("releases/latest/download")
                && !contents.contains("downloads a pre-built binary"),
            "{path} must document the local-source runtime lane"
        );
    }
}

#[test]
fn launchers_only_resolve_local_cargo_release_binary() {
    let tmux_launcher = read("tmux-agent-sidebar.tmux");
    let hook_launcher = read("hook.sh");
    let local_binary = "target/release/tmux-agent-sidebar";

    assert!(tmux_launcher.contains(local_binary));
    assert!(hook_launcher.contains(local_binary));
    assert!(tmux_launcher.contains("-newer \"$SIDEBAR_BINARY\""));

    for (path, contents) in [
        ("tmux-agent-sidebar.tmux", tmux_launcher),
        ("hook.sh", hook_launcher),
    ] {
        assert!(
            !contents.contains("bin/tmux-agent-sidebar")
                && !contents.contains("command -v tmux-agent-sidebar")
                && !contents.contains("command -v \"tmux-agent-sidebar\""),
            "{path} must not fall back to a downloaded or PATH binary"
        );
    }
}
