//! Deterministic UI fixture: no workers, credentials, session logs, or network.
use tmux_agent_sidebar::{
    quota::{QuotaFetch, QuotaWindow, Subscription},
    state::{AppState, PopupState},
    usage::{
        Spend, Tokens,
        metering::{Snapshot, Summary, Total},
    },
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let mut state = AppState::new("%fixture".into());
    state.now = if mode == "off-peak" {
        1_792_990_800
    } else {
        1_793_000_000
    };
    if mode == "click" {
        state.usage.selected = tmux_agent_sidebar::usage::Window::SevenDays;
        state.now =
            (tmux_agent_sidebar::usage::Window::Today.start(state.now as i64) + 12 * 3600) as u64;
    }
    state.bottom_panel_height = 0;
    state.band_enabled = false;
    let end = state.now
        + if mode == "click" {
            604_800 - 8 * 3600
        } else {
            3 * 86400
        };
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
    state.usage.cycle.lock().unwrap().end = end;
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
    let snapshot = Snapshot {
        cycle_end: end,
        cycle: Some(summary.clone()),
        recent: std::array::from_fn(|i| Summary {
            start: if i == 3 {
                tmux_agent_sidebar::usage::Window::Today.start(state.now as i64)
            } else {
                state.now as i64 - [86_400, 604_800, 2_592_000][i]
            },
            ..summary.clone()
        }),
        observed_at: state.now as i64,
        incomplete: false,
    };
    state.usage.codex_all = Some(snapshot.clone());
    state.usage.codex_account = Some(snapshot);
    if mode == "error" {
        state.apply_quota_result(tmux_agent_sidebar::quota::QuotaFetchResult {
            codex_account: None,
            codex: Some(Err(tmux_agent_sidebar::quota::FetchError::with_kind(
                "fixture timeout",
                tmux_agent_sidebar::quota::FetchKind::Timeout,
            ))),
            kimi: None,
            forced: false,
        });
    }
    if matches!(mode.as_str(), "wide" | "daily" | "compact" | "both-daily") {
        state.usage.selected = tmux_agent_sidebar::usage::Window::SevenDays;
        state.usage.codex_all_window = tmux_agent_sidebar::usage::Window::SevenDays;
        state.usage.codex_account_window = tmux_agent_sidebar::usage::CodexWindow::Recent(
            tmux_agent_sidebar::usage::Window::SevenDays,
        );
    }
    if mode == "click" {
        state.usage.codex_all_window = tmux_agent_sidebar::usage::Window::SevenDays;
        state.usage.codex_account_window = tmux_agent_sidebar::usage::CodexWindow::Recent(
            tmux_agent_sidebar::usage::Window::SevenDays,
        );
    }
    if matches!(mode.as_str(), "daily" | "compact" | "both-daily") {
        for row in [
            tmux_agent_sidebar::usage::UsageRow::DeepSeek,
            tmux_agent_sidebar::usage::UsageRow::CodexAll,
            tmux_agent_sidebar::usage::UsageRow::CodexAccount,
        ] {
            state.usage.toggle_daily(row, state.now);
        }
    }
    if matches!(mode.as_str(), "empty" | "zero" | "empty-cycle") {
        state.usage.snapshot = if mode == "zero" {
            Some(Spend::default())
        } else {
            None
        };
        state.usage.codex_all.as_mut().unwrap().recent
            [state.usage.codex_all_window.recent().index()]
        .total
        .tokens = Tokens {
            uncached: 0,
            cached: 2_000_000_000,
            output: 0,
        };
        if mode == "empty-cycle" {
            state.usage.cycle.lock().unwrap().end = 0;
        }
    }
    if mode.starts_with("history-error") {
        state.usage.history.notice = Some("用量历史保存失败".into());
        state.usage.history.durable = false;
        state.usage.history.last_success = Some(state.now - 300);
        state.usage.history.details = vec![
            "Database write failed: disk full".into(),
            "Existing history is preserved.".into(),
            "Inspect: usage-history status".into(),
        ];
        if mode == "history-error-popup" {
            state.popup = PopupState::Notices { area: None };
        }
    }
    if mode == "history-import" {
        state.usage.history.importing = true;
        state.usage.history.durable = true;
    }
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    terminal.clear()?;
    terminal.hide_cursor()?;
    terminal.draw(|frame| tmux_agent_sidebar::ui::draw(frame, &mut state))?;
    if mode == "click" {
        use crossterm::event::{Event, MouseButton, MouseEventKind};
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(30) {
            if crossterm::event::poll(std::time::Duration::from_millis(100))? {
                match crossterm::event::read()? {
                    Event::Mouse(mouse)
                        if mouse.kind == MouseEventKind::Down(MouseButton::Left) =>
                    {
                        state.handle_mouse_click(mouse.row, mouse.column);
                    }
                    Event::Resize(..) => {}
                    _ => continue,
                }
                terminal.draw(|frame| tmux_agent_sidebar::ui::draw(frame, &mut state))?;
            }
        }
        crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)?;
        crossterm::terminal::disable_raw_mode()?;
    } else {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
    Ok(())
}
