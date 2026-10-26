//! Deterministic UI fixture: no workers, credentials, session logs, or network.
use std::sync::atomic::Ordering;
use tmux_agent_sidebar::{
    quota::{QuotaFetch, QuotaWindow, Subscription},
    state::{AppState, PopupState},
    usage::{
        Spend, Tokens,
        metering::{Snapshot, Summary, Total},
    },
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = AppState::new("%fixture".into());
    state.now = 1_793_000_000;
    state.bottom_panel_height = 0;
    state.band_enabled = false;
    let end = state.now + 3 * 86400;
    state.quota.received_first_result = true;
    state.quota.apply(
        Subscription::Codex,
        Ok(QuotaFetch::Available(vec![
            QuotaWindow {
                label: "5h".into(),
                remaining_percent: 62,
                resets_at: Some(state.now + 7800),
            },
            QuotaWindow {
                label: "wk".into(),
                remaining_percent: 35,
                resets_at: Some(end),
            },
        ])),
    );
    state.usage.received = true;
    state.usage.snapshot = Some(Spend {
        tokens: Tokens {
            uncached: 200_000,
            cached: 12_000_000,
            output: 100_000,
        },
        cost_cny: 1.47,
        unpriced: false,
    });
    state.usage.cycle_end.store(end, Ordering::Relaxed);
    let total = Total {
        tokens: Tokens {
            uncached: 200_000,
            cached: 17_000_000,
            output: 1_000_000,
        },
        cost: 128_400_000_000_000,
        assumed_standard: 4,
        ..Total::default()
    };
    let summary = Summary {
        start: (end - 604800) as i64,
        end: state.now as i64,
        total: total.clone(),
        models: [("gpt-6.1-sol".into(), total)].into(),
    };
    state.usage.codex = Some(Snapshot {
        cycle_end: end,
        cycle: Some(summary.clone()),
        recent: std::array::from_fn(|i| Summary {
            start: state.now as i64 - [86_400, 604_800, 2_592_000][i],
            ..summary.clone()
        }),
        observed_at: state.now as i64,
        incomplete: false,
    });
    if std::env::args().nth(1).as_deref() == Some("error") {
        state.apply_quota_result(tmux_agent_sidebar::quota::QuotaFetchResult {
            codex: Some(Err(tmux_agent_sidebar::quota::FetchError::with_kind(
                "fixture timeout",
                tmux_agent_sidebar::quota::FetchKind::Timeout,
            ))),
            kimi: None,
            forced: false,
        });
    }
    if std::env::args().nth(1).as_deref() == Some("details") {
        state.popup = PopupState::Usage {
            recent: false,
            scroll: 0,
        };
    }
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    terminal.clear()?;
    terminal.hide_cursor()?;
    terminal.draw(|frame| tmux_agent_sidebar::ui::draw(frame, &mut state))?;
    std::thread::sleep(std::time::Duration::from_secs(30));
    Ok(())
}
