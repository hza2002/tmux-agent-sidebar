use crate::tmux;
use serde_json::Value;

/// Session-scoped child stop observations prevent older snapshots resurrecting work.
pub(in crate::cli::hook) const KEY: &str = tmux::PANE_STOPPED_SUBAGENTS;

pub(in crate::cli::hook) fn stopped(pane: &str, id: &str) -> bool {
    !id.is_empty() && read(pane).iter().any(|stopped| stopped == id)
}

pub(in crate::cli::hook) fn record(pane: &str, id: &str) {
    if id.is_empty() {
        return;
    }
    let mut ids = read(pane);
    if !ids.iter().any(|stopped| stopped == id) {
        ids.push(id.to_string());
        write(pane, ids);
    }
}

/// A SubagentStart under the same ID begins a new run of that child.
pub(in crate::cli::hook) fn forget(pane: &str, id: &str) {
    let mut ids = read(pane);
    let previous_len = ids.len();
    ids.retain(|stopped| stopped != id);
    if ids.len() != previous_len {
        write(pane, ids);
    }
}

pub(in crate::cli::hook) fn clear(pane: &str) {
    tmux::unset_pane_option(pane, KEY);
}

fn read(pane: &str) -> Vec<String> {
    let raw = tmux::get_pane_option_value(pane, KEY);
    let Ok(Value::Array(ids)) = serde_json::from_str(&raw) else {
        return Vec::new();
    };
    ids.into_iter()
        .filter_map(|id| id.as_str().map(str::to_string))
        .collect()
}

fn write(pane: &str, ids: Vec<String>) {
    if ids.is_empty() {
        clear(pane);
    } else {
        let value = Value::Array(ids.into_iter().map(Value::String).collect());
        tmux::set_pane_option(pane, KEY, &value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_stop_records_one_identity() {
        let _guard = tmux::test_mock::install();
        let pane = "%STOPPED_DUPLICATE";
        record(pane, "child-1");
        record(pane, "child-1");
        record(pane, "");

        assert!(stopped(pane, "child-1"));
        assert!(!stopped(pane, "child-2"));
        assert!(!stopped(pane, ""));
        assert_eq!(read(pane), vec!["child-1"]);
    }

    #[test]
    fn resumed_child_forgets_only_its_previous_stop() {
        let _guard = tmux::test_mock::install();
        let pane = "%STOPPED_RESUME";
        record(pane, "child-1");
        record(pane, "child-2");

        forget(pane, "child-1");
        assert!(!stopped(pane, "child-1"));
        assert!(stopped(pane, "child-2"));

        forget(pane, "child-2");
        assert!(!tmux::test_mock::contains(pane, KEY));
    }

    #[test]
    fn session_cleanup_forgets_all_stop_observations() {
        let _guard = tmux::test_mock::install();
        let pane = "%STOPPED_CLEAR";
        record(pane, "child-1");
        clear(pane);
        assert!(!stopped(pane, "child-1"));
        assert!(!tmux::test_mock::contains(pane, KEY));
    }
}
