use std::fs;

#[test]
fn upstream_sync_releases_are_prereleases_and_never_latest() {
    let workflow = fs::read_to_string(".github/workflows/release.yml")
        .expect("release workflow must be readable");

    assert!(workflow.contains("prerelease: ${{ github.event_name == 'repository_dispatch' }}"));
    assert!(workflow.contains("make_latest: ${{ github.event_name == 'push' }}"));
}
