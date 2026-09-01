use std::collections::HashMap;

use crate::tmux::{self, PaneStatus};

const DEFAULT_RUNNING_ICON: &str = "";
const RUNNING_FRAMES: [&str; 4] = ["", "", "", ""];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusIcons {
    /// Icon for the "All" filter in the top filter bar.
    all: String,
    running: String,
    background: String,
    waiting: String,
    idle: String,
    ready: String,
    reviewing: String,
    error: String,
    unknown: String,
    animate_running: bool,
}

impl Default for StatusIcons {
    fn default() -> Self {
        Self {
            all: "".into(),
            running: DEFAULT_RUNNING_ICON.into(),
            background: "".into(),
            waiting: "".into(),
            idle: "".into(),
            ready: "".into(),
            reviewing: "".into(),
            error: "".into(),
            unknown: "".into(),
            animate_running: true,
        }
    }
}

impl StatusIcons {
    /// Load status icons from tmux @sidebar_icon_* variables, falling back to defaults.
    pub fn from_tmux() -> Self {
        let all_opts = tmux::get_all_global_options();
        Self::from_options(&all_opts)
    }

    pub fn from_options(all_opts: &HashMap<String, String>) -> Self {
        let mut icons = Self::default();

        let read = |var: &str, fallback: &str| -> String {
            all_opts
                .get(var)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| fallback.to_string())
        };

        icons.all = read(tmux::SIDEBAR_ICON_ALL, &icons.all);
        let running_override = all_opts
            .get(tmux::SIDEBAR_ICON_RUNNING)
            .is_some_and(|value| !value.trim().is_empty());
        icons.running = read(tmux::SIDEBAR_ICON_RUNNING, &icons.running);
        icons.animate_running = !running_override;
        icons.background = read(tmux::SIDEBAR_ICON_BACKGROUND, &icons.background);
        icons.waiting = read(tmux::SIDEBAR_ICON_WAITING, &icons.waiting);
        icons.idle = read(tmux::SIDEBAR_ICON_IDLE, &icons.idle);
        icons.ready = read(tmux::SIDEBAR_ICON_READY, &icons.ready);
        icons.reviewing = read(tmux::SIDEBAR_ICON_REVIEWING, &icons.reviewing);
        icons.error = read(tmux::SIDEBAR_ICON_ERROR, &icons.error);
        icons.unknown = read(tmux::SIDEBAR_ICON_UNKNOWN, &icons.unknown);
        icons
    }

    /// Icon used for the "All" filter (not tied to any PaneStatus).
    pub fn all_icon(&self) -> &str {
        self.all.as_str()
    }

    pub fn status_icon(&self, status: &PaneStatus) -> &str {
        match status {
            PaneStatus::Running => self.running.as_str(),
            PaneStatus::Background => self.background.as_str(),
            PaneStatus::Waiting => self.waiting.as_str(),
            PaneStatus::Idle => self.idle.as_str(),
            PaneStatus::Error => self.error.as_str(),
            PaneStatus::Unknown => self.unknown.as_str(),
        }
    }

    pub fn pane_icon(&self, status: &PaneStatus, wait_reason: &str) -> &str {
        match wait_reason {
            crate::tmux::WAIT_REASON_RESPONSE_READY => self.ready.as_str(),
            crate::tmux::WAIT_REASON_RESPONSE_REVIEWING => self.reviewing.as_str(),
            _ => self.status_icon(status),
        }
    }

    /// Return the pane icon for this second, animating only the default running icon.
    /// User-configured running icons remain static.
    pub fn pane_icon_at(&self, status: &PaneStatus, wait_reason: &str, now: u64) -> &str {
        if now > 0
            && *status == PaneStatus::Running
            && wait_reason.is_empty()
            && self.animate_running
        {
            let phase = (now % RUNNING_FRAMES.len() as u64) as usize;
            RUNNING_FRAMES[phase]
        } else {
            self.pane_icon(status, wait_reason)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_icons_match_current_glyphs() {
        let icons = StatusIcons::default();
        assert_eq!(icons.all_icon(), "");
        assert_eq!(icons.status_icon(&PaneStatus::Running), "");
        assert_eq!(icons.status_icon(&PaneStatus::Background), "");
        assert_eq!(icons.status_icon(&PaneStatus::Waiting), "");
        assert_eq!(icons.status_icon(&PaneStatus::Idle), "");
        assert_eq!(icons.status_icon(&PaneStatus::Error), "");
        assert_eq!(icons.status_icon(&PaneStatus::Unknown), "");
        assert_eq!(
            icons.pane_icon(&PaneStatus::Waiting, tmux::WAIT_REASON_RESPONSE_READY),
            ""
        );
    }

    #[test]
    fn tmux_options_override_defaults() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_ICON_ALL.into(), "∀".into());
        opts.insert(tmux::SIDEBAR_ICON_RUNNING.into(), "◉".into());
        opts.insert(tmux::SIDEBAR_ICON_BACKGROUND.into(), "⊙".into());
        opts.insert(tmux::SIDEBAR_ICON_UNKNOWN.into(), "∎".into());

        let icons = StatusIcons::from_options(&opts);
        assert_eq!(icons.all_icon(), "∀");
        assert_eq!(icons.status_icon(&PaneStatus::Running), "◉");
        assert_eq!(icons.status_icon(&PaneStatus::Background), "⊙");
        assert_eq!(icons.status_icon(&PaneStatus::Unknown), "∎");
        assert_eq!(icons.status_icon(&PaneStatus::Waiting), "");
    }

    #[test]
    fn default_running_icon_animates_at_one_second_phases() {
        let icons = StatusIcons::default();

        assert_eq!(
            icons.pane_icon_at(&PaneStatus::Running, "", 0),
            DEFAULT_RUNNING_ICON
        );

        for (phase, expected) in RUNNING_FRAMES.iter().enumerate() {
            let now = phase as u64 + RUNNING_FRAMES.len() as u64;
            assert_eq!(icons.pane_icon_at(&PaneStatus::Running, "", now), *expected);
            assert_eq!(crate::ui::text::display_width(expected), 1);
        }
        assert_eq!(
            icons.pane_icon_at(&PaneStatus::Running, "", 8),
            RUNNING_FRAMES[0]
        );
    }

    #[test]
    fn custom_or_overridden_icons_remain_static() {
        let mut opts = HashMap::new();
        opts.insert(tmux::SIDEBAR_ICON_RUNNING.into(), "◆".into());
        let icons = StatusIcons::from_options(&opts);

        assert_eq!(icons.pane_icon_at(&PaneStatus::Running, "", 2), "◆");
        opts.insert(
            tmux::SIDEBAR_ICON_RUNNING.into(),
            DEFAULT_RUNNING_ICON.into(),
        );
        assert_eq!(
            StatusIcons::from_options(&opts).pane_icon_at(&PaneStatus::Running, "", 2),
            DEFAULT_RUNNING_ICON
        );
        assert_eq!(
            StatusIcons::default().pane_icon_at(
                &PaneStatus::Running,
                tmux::WAIT_REASON_RESPONSE_READY,
                2,
            ),
            ""
        );
    }
}
