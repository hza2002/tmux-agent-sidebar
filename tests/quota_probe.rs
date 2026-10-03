//! Live-network smoke probes for the subscription quota fetchers.
//!
//! The unit tests inject `CurlRunner` mocks, which verify what we *believe*
//! the outside world looks like: they all passed while a real curl 8.7.1
//! pretty-printed `%{header_json}` over many lines and every live fetch
//! failed. These probes close that gap by exercising the whole stack — the
//! developer's real credentials, the real curl binary, the real providers,
//! and the real parsers.
//!
//! They are `#[ignore]`d out of the default `cargo test` run: they need the
//! network and a machine logged into both subscriptions, and they can fail
//! for environmental reasons (throttling, provider outages, VPN). Run them
//! manually after changing `src/quota/`:
//!
//! ```bash
//! cargo test --test quota_probe -- --ignored
//! ```

use tmux_agent_sidebar::quota::{QuotaFetch, QuotaWindow};

/// The probes assert the numbers are *plausible*, not merely that the fetch
/// did not error: labels the block can render, a percentage in range, and a
/// reset stamp that is a future Unix-seconds value.
fn assert_sane_windows(subscription: &str, windows: &[QuotaWindow]) {
    assert!(
        !windows.is_empty(),
        "{subscription}: an Available fetch must carry at least one window"
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before the epoch")
        .as_secs();
    for window in windows {
        assert!(
            window.label == "5h" || window.label == "wk",
            "{subscription}: unexpected window label {:?}",
            window.label
        );
        assert!(
            window.remaining_percent <= 100,
            "{subscription}: {} window remaining_percent {} out of range",
            window.label,
            window.remaining_percent
        );
        if let Some(resets_at) = window.resets_at {
            assert!(
                resets_at > now,
                "{subscription}: {} window resets_at {resets_at} is not a future Unix-seconds \
                 stamp (now {now}); a provider that is late rolling a window can pass a stale \
                 stamp briefly, so re-run before suspecting the parser",
                window.label
            );
        }
        println!(
            "{subscription}: {} remaining {}% resets_at {:?}",
            window.label, window.remaining_percent, window.resets_at
        );
    }
}

#[test]
#[ignore = "live network probe; run manually after changing src/quota/"]
fn codex_quota_fetch_returns_sane_windows() {
    match tmux_agent_sidebar::quota::codex::fetch_quota() {
        Ok(QuotaFetch::Available(windows)) => assert_sane_windows("codex", &windows),
        // Not being logged into one provider must not fail the probe run.
        Ok(QuotaFetch::Unavailable) => {
            println!("codex: no ChatGPT login in ~/.codex/auth.json; skipping assertions");
        }
        Err(error) => panic!(
            "codex live fetch failed: {error} (kind {:?}); check whether the provider is \
             throttling or unreachable before suspecting the code",
            error.kind
        ),
    }
}

#[test]
#[ignore = "live network probe; run manually after changing src/quota/"]
fn kimi_quota_fetch_returns_sane_windows() {
    match tmux_agent_sidebar::quota::kimi::fetch_quota() {
        Ok(QuotaFetch::Available(windows)) => assert_sane_windows("kimi", &windows),
        Ok(QuotaFetch::Unavailable) => {
            println!("kimi: no credentials in ~/.kimi-code; skipping assertions");
        }
        Err(error) => panic!(
            "kimi live fetch failed: {error} (kind {:?}); check whether the provider is \
             throttling or unreachable before suspecting the code",
            error.kind
        ),
    }
}
