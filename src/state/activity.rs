use std::time::{Instant, SystemTime};

use super::AppState;
use crate::activity::ActivityEntry;
use crate::state::ScrollState;

#[derive(Debug, Clone)]
pub struct ActivityState {
    pub entries: Vec<ActivityEntry>,
    pub scroll: ScrollState,
    /// Cursor into `entries`: the entry the block highlights and the one `y`
    /// copies. `0` is the newest entry, which is the top row.
    pub selected: usize,
    pub max_entries: usize,
    /// `(focused_pane_id, mtime)` of the activity log most recently
    /// rendered into `entries`. `refresh_activity_log` skips re-reading
    /// the log when neither field has changed.
    pub log_cache: Option<(String, SystemTime)>,
}

impl ActivityState {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            scroll: ScrollState::default(),
            selected: 0,
            max_entries: 50,
            log_cache: None,
        }
    }

    /// Replace the log with a freshly read one, keeping the cursor on the entry
    /// the reader picked.
    ///
    /// `selected == 0` means "the newest", so a log that grows under a
    /// top-row cursor keeps pointing at the newest command — the one the reader
    /// is watching for. A cursor further down follows its entry by identity, so
    /// an arrival at the head cannot silently move the copy target onto a
    /// different command.
    pub fn replace_entries(&mut self, entries: Vec<ActivityEntry>) {
        let anchor = if self.selected > 0 {
            self.entries.get(self.selected).cloned()
        } else {
            None
        };
        self.entries = entries;
        self.selected = match anchor {
            Some(previous) => self
                .entries
                .iter()
                .position(|entry| *entry == previous)
                .unwrap_or(self.selected),
            None => 0,
        };
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    /// Move the cursor by `delta` entries, clamped to the log.
    pub fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            self.selected = 0;
            return;
        }
        let last = self.entries.len() - 1;
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, last as isize) as usize;
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    pub fn select_last(&mut self) {
        self.selected = self.entries.len().saturating_sub(1);
    }

    /// The entry the cursor points at, or the newest one while the log is
    /// still empty of a valid index.
    pub fn selected_entry(&self) -> Option<&ActivityEntry> {
        self.entries
            .get(self.selected)
            .or_else(|| self.entries.first())
    }
}

impl Default for ActivityState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    // ─── Flash banner ────────────────────────────────────────────────

    /// The command `y` copies out of the activity block: the most recent
    /// entry's label, without its timestamp.
    pub fn activity_copy_text(&self) -> Option<String> {
        let entry = self.activity.selected_entry()?;
        if entry.label.is_empty() {
            None
        } else {
            Some(entry.label.clone())
        }
    }

    /// Queue the focused activity entry's command for every clipboard surface:
    /// the OS clipboard and tmux's paste buffer are written by the main loop
    /// from [`AppState::pending_clipboard_copy`], the upstream terminal from
    /// [`AppState::pending_osc52_copy`]. Kept to bookkeeping only so the input
    /// path stays free of process and platform calls.
    pub fn request_activity_copy(&mut self) -> bool {
        let Some(text) = self.activity_copy_text() else {
            self.set_flash("copy: nothing to copy");
            return false;
        };
        let preview = crate::ui::text::truncate_to_width(&text, 24);
        self.pending_osc52_copy = Some(text.clone());
        self.pending_clipboard_copy = Some(text);
        self.set_flash(format!("copied: {preview}"));
        true
    }

    pub fn set_flash(&mut self, msg: impl Into<String>) {
        self.flash = Some((
            msg.into(),
            Instant::now() + std::time::Duration::from_secs(4),
        ));
    }

    /// Return the current flash text if still valid, clearing it once the
    /// deadline passes. Called by the UI once per frame.
    pub fn take_flash(&mut self) -> Option<String> {
        match &self.flash {
            Some((text, exp)) if Instant::now() < *exp => Some(text.clone()),
            Some(_) => {
                self.flash = None;
                None
            }
            None => None,
        }
    }

    pub fn apply_git_data(&mut self, data: crate::git::GitData) {
        self.git = data;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_initializes_expected_defaults() {
        let state = ActivityState::new();
        assert!(state.entries.is_empty());
        assert_eq!(state.max_entries, 50);
        assert_eq!(state.scroll.offset, 0);
        assert_eq!(state.scroll.total_lines, 0);
        assert_eq!(state.scroll.visible_height, 0);
        assert!(state.log_cache.is_none());
    }

    #[test]
    fn default_delegates_to_new() {
        let default_state = ActivityState::default();
        let new_state = ActivityState::new();
        assert_eq!(default_state.entries.len(), new_state.entries.len());
        assert_eq!(default_state.max_entries, new_state.max_entries);
        assert_eq!(default_state.scroll.offset, new_state.scroll.offset);
        assert!(default_state.log_cache.is_none());
    }

    // ─── Flash banner / apply_git_data ───────────────────────────────

    #[test]
    fn set_flash_stores_message_with_future_expiry() {
        let mut state = AppState::new("%99".into());
        state.set_flash("hello");
        let (msg, exp) = state.flash.as_ref().expect("flash must be set");
        assert_eq!(msg, "hello");
        assert!(*exp > Instant::now());
    }

    #[test]
    fn take_flash_returns_message_then_noop_on_expiry() {
        let mut state = AppState::new("%99".into());
        state.set_flash("msg");
        assert_eq!(state.take_flash().as_deref(), Some("msg"));
        // Force-expire by rewinding the deadline into the past.
        state.flash = Some((
            "stale".into(),
            Instant::now() - std::time::Duration::from_secs(1),
        ));
        assert!(state.take_flash().is_none());
        assert!(state.flash.is_none());
    }

    #[test]
    fn take_flash_none_when_unset() {
        let mut state = AppState::new("%99".into());
        assert!(state.take_flash().is_none());
    }

    // ─── Activity copy ───────────────────────────────────────────────

    fn state_with_activity(entries: Vec<ActivityEntry>) -> AppState {
        let mut state = AppState::new("%99".into());
        state.activity.entries = entries;
        state
    }

    #[test]
    fn activity_copy_text_is_the_newest_label() {
        let state = state_with_activity(vec![
            ActivityEntry {
                timestamp: "10:32".into(),
                tool: "Bash".into(),
                label: "cargo test".into(),
            },
            ActivityEntry {
                timestamp: "10:31".into(),
                tool: "Edit".into(),
                label: "main.rs".into(),
            },
        ]);
        assert_eq!(state.activity_copy_text().as_deref(), Some("cargo test"));
    }

    #[test]
    fn activity_copy_text_follows_the_cursor() {
        let mut state = state_with_activity(vec![
            ActivityEntry {
                timestamp: "10:32".into(),
                tool: "Bash".into(),
                label: "cargo test".into(),
            },
            ActivityEntry {
                timestamp: "10:31".into(),
                tool: "Edit".into(),
                label: "main.rs".into(),
            },
        ]);
        state.activity.select_last();
        assert_eq!(state.activity_copy_text().as_deref(), Some("main.rs"));
    }

    // ─── Cursor ──────────────────────────────────────────────────────

    fn entry(timestamp: &str, tool: &str, label: &str) -> ActivityEntry {
        ActivityEntry {
            timestamp: timestamp.into(),
            tool: tool.into(),
            label: label.into(),
        }
    }

    #[test]
    fn move_selection_clamps_to_the_log() {
        let mut state = state_with_activity(vec![
            entry("10:32", "Bash", "one"),
            entry("10:31", "Bash", "two"),
        ]);
        state.activity.move_selection(5);
        assert_eq!(state.activity.selected, 1);
        state.activity.move_selection(-5);
        assert_eq!(state.activity.selected, 0);

        let mut empty = state_with_activity(vec![]);
        empty.activity.move_selection(3);
        assert_eq!(empty.activity.selected, 0);
    }

    #[test]
    fn replace_entries_keeps_the_cursor_on_the_same_command() {
        let mut state = state_with_activity(vec![
            entry("10:32", "Bash", "cargo test"),
            entry("10:31", "Bash", "cargo build"),
            entry("10:30", "Edit", "main.rs"),
        ]);
        state.activity.move_selection(2);

        // New activity arrives at the head: the cursor must follow the command
        // the reader picked, not the row number it used to sit on.
        state.activity.replace_entries(vec![
            entry("10:33", "Bash", "cargo clippy"),
            entry("10:32", "Bash", "cargo test"),
            entry("10:31", "Bash", "cargo build"),
            entry("10:30", "Edit", "main.rs"),
        ]);
        assert_eq!(state.activity.selected, 3);
        assert_eq!(
            state.activity.selected_entry().map(|e| e.label.as_str()),
            Some("main.rs")
        );
    }

    #[test]
    fn replace_entries_keeps_a_top_cursor_on_the_newest() {
        let mut state = state_with_activity(vec![entry("10:32", "Bash", "cargo test")]);
        // Row 0 means "the newest", so it stays on the newest as the log grows.
        state.activity.replace_entries(vec![
            entry("10:33", "Bash", "cargo clippy"),
            entry("10:32", "Bash", "cargo test"),
        ]);
        assert_eq!(state.activity.selected, 0);
        assert_eq!(state.activity_copy_text().as_deref(), Some("cargo clippy"));
    }

    #[test]
    fn replace_entries_clamps_a_cursor_whose_entry_is_gone() {
        let mut state = state_with_activity(vec![
            entry("10:32", "Bash", "cargo test"),
            entry("10:31", "Bash", "cargo build"),
            entry("10:30", "Edit", "main.rs"),
        ]);
        state.activity.move_selection(2);

        // The log was trimmed past the entry the cursor was on.
        state
            .activity
            .replace_entries(vec![entry("10:40", "Bash", "cargo clippy")]);
        assert_eq!(state.activity.selected, 0);

        let mut emptied = state_with_activity(vec![entry("10:32", "Bash", "cargo test")]);
        emptied.activity.replace_entries(vec![]);
        assert_eq!(emptied.activity.selected, 0);
        assert!(emptied.activity_copy_text().is_none());
    }

    #[test]
    fn activity_copy_text_needs_a_label() {
        assert!(state_with_activity(vec![]).activity_copy_text().is_none());
        let state = state_with_activity(vec![ActivityEntry {
            timestamp: "10:32".into(),
            tool: "TaskStop".into(),
            label: String::new(),
        }]);
        // An entry with no label has no command to hand over; copying the tool
        // name instead would paste a word that runs nothing.
        assert!(state.activity_copy_text().is_none());
    }

    #[test]
    fn request_activity_copy_queues_both_sinks_and_flashes() {
        let mut state = state_with_activity(vec![ActivityEntry {
            timestamp: "10:32".into(),
            tool: "Bash".into(),
            label: "cargo test --all-targets".into(),
        }]);
        assert!(state.request_activity_copy());
        assert_eq!(
            state.pending_clipboard_copy.as_deref(),
            Some("cargo test --all-targets")
        );
        assert_eq!(
            state.pending_osc52_copy.as_deref(),
            Some("cargo test --all-targets")
        );
        assert_eq!(
            state.take_flash().as_deref(),
            Some("copied: cargo test --all-targets")
        );
    }

    #[test]
    fn request_activity_copy_without_a_command_only_flashes() {
        let mut state = state_with_activity(vec![]);
        assert!(!state.request_activity_copy());
        assert!(state.pending_clipboard_copy.is_none());
        assert!(state.pending_osc52_copy.is_none());
        assert_eq!(state.take_flash().as_deref(), Some("copy: nothing to copy"));
    }

    #[test]
    fn apply_git_data_overwrites_previous_state() {
        let mut state = AppState::new("%99".into());
        state.git.branch = "old".into();
        state.apply_git_data(crate::git::GitData {
            branch: "new".into(),
            diff_stat: Some((3, 1)),
            ..Default::default()
        });
        assert_eq!(state.git.branch, "new");
        assert_eq!(state.git.diff_stat, Some((3, 1)));
    }
}
