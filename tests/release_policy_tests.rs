use std::fs;

#[test]
fn releases_require_explicit_version_tags() {
    let workflow = fs::read_to_string(".github/workflows/release.yml")
        .expect("release workflow must be readable");
    let triggers: Vec<_> = workflow
        .lines()
        .skip_while(|line| *line != "on:")
        .skip(1)
        .take_while(|line| line.is_empty() || line.starts_with(' '))
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(triggers, ["  push:", "    tags:", "      - \"v*\""]);
    assert!(!workflow.contains("repository_dispatch"));
    assert!(!std::path::Path::new(".github/workflows/sync-upstream.yml").exists());
    assert!(workflow.contains("prerelease: false"));
    assert!(workflow.contains("make_latest: true"));
}
