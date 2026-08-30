use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn assert_relative_symlink(link: &Path, target: &Path) {
    let metadata = fs::symlink_metadata(link)
        .unwrap_or_else(|error| panic!("failed to inspect {}: {error}", link.display()));
    assert!(
        metadata.file_type().is_symlink(),
        "{} must be a compatibility symlink",
        link.display()
    );
    assert_eq!(
        fs::read_link(link).expect("compatibility symlink must be readable"),
        target
    );
}

#[test]
fn claude_harness_paths_alias_codex_sources() {
    let root = repo_root();
    let expected = expected_skill_names();

    assert_relative_symlink(&root.join("CLAUDE.md"), Path::new("AGENTS.md"));

    assert_eq!(
        fs::canonicalize(root.join("CLAUDE.md")).expect("AGENTS.md must exist"),
        fs::canonicalize(root.join("AGENTS.md")).expect("AGENTS.md must exist")
    );

    let claude_names = fs::read_dir(root.join(".claude/skills"))
        .expect("Claude skills directory must be readable")
        .map(|entry| {
            entry
                .expect("Claude skill entry must be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(claude_names, expected);

    for name in expected {
        let alias = root.join(".claude/skills").join(&name);
        let canonical = root.join(".agents/skills").join(&name);
        assert_relative_symlink(&alias, Path::new(&format!("../../.agents/skills/{name}")));
        assert_eq!(
            fs::canonicalize(&alias).expect("Claude skill alias must resolve"),
            fs::canonicalize(&canonical).expect("Codex skill must exist")
        );
    }
}

fn expected_skill_names() -> BTreeSet<String> {
    [
        "docs-audit",
        "regenerate-captures",
        "sync-upstream-features",
        "ui-showcase",
        "version-release",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[test]
fn codex_skills_have_discoverable_frontmatter() {
    let skills_dir = repo_root().join(".agents/skills");
    let mut names = BTreeSet::new();

    for entry in fs::read_dir(&skills_dir).expect("Codex skills directory must be readable") {
        let entry = entry.expect("skill directory entry must be readable");
        if !entry
            .file_type()
            .expect("skill type must be readable")
            .is_dir()
        {
            continue;
        }

        let directory_name = entry.file_name().to_string_lossy().into_owned();
        let skill_path = entry.path().join("SKILL.md");
        let contents = fs::read_to_string(&skill_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", skill_path.display()));
        let mut sections = contents.splitn(3, "---");
        assert_eq!(
            sections.next(),
            Some(""),
            "{} needs frontmatter",
            skill_path.display()
        );
        let frontmatter = sections
            .next()
            .unwrap_or_else(|| panic!("{} needs closing frontmatter", skill_path.display()));
        assert!(
            sections.next().is_some(),
            "{} needs a body after frontmatter",
            skill_path.display()
        );

        let name = frontmatter
            .lines()
            .find_map(|line| line.strip_prefix("name:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| panic!("{} needs a non-empty name", skill_path.display()));
        let description = frontmatter
            .lines()
            .find_map(|line| line.strip_prefix("description:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| panic!("{} needs a non-empty description", skill_path.display()));

        assert_eq!(name, directory_name, "skill name must match its directory");
        assert!(
            names.insert(name.to_owned()),
            "duplicate skill name: {name}"
        );
        assert!(!description.is_empty());
    }

    assert_eq!(names, expected_skill_names());
}

#[test]
fn verification_surfaces_keep_activity_logs_private() {
    let root = repo_root();
    let fixture = fs::read_to_string(root.join("fixtures/scenarios/common/_lib.sh"))
        .expect("scenario fixture must be readable");
    let verifier = fs::read_to_string(root.join("scripts/verify.sh"))
        .expect("verification script must be readable");

    assert!(fixture.contains("TMUX_AGENT_ACTIVITY_DIR=\"$TMUX_DIR/activity\""));
    assert!(!fixture.contains("/tmp/tmux-agent-activity"));
    assert!(verifier.contains("TMUX_AGENT_ACTIVITY_DIR=\"$test_tmux_dir/activity\""));
}
