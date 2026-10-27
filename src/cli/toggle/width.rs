//! Restore configured width only after a real window-size change. The legacy
//! layout hook also fires for manual pane edits and zoom, so dimensions are
//! recorded on the window instead of treating every hook as a resize.

use super::{SidebarIntent, TmuxClient, enabled_intent, resolve_sidebar_width, topology};

pub(super) const STATE_OPTION: &str = "@agent_sidebar_width_state";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WidthState {
    width: u32,
    height: u32,
    pending: bool,
    revision: u64,
}

impl WidthState {
    fn parse(value: &str) -> Option<Self> {
        let mut fields = value.split(',');
        let state = Self {
            width: fields.next()?.parse().ok()?,
            height: fields.next()?.parse().ok()?,
            pending: match fields.next()? {
                "0" => false,
                "1" => true,
                _ => return None,
            },
            revision: fields.next().map(str::parse).transpose().ok()?.unwrap_or(0),
        };
        (state.width > 0 && state.height > 0 && fields.next().is_none()).then_some(state)
    }

    fn encode(self) -> String {
        format!(
            "{},{},{},{}",
            self.width,
            self.height,
            u8::from(self.pending),
            self.revision
        )
    }
}

struct Snapshot {
    current: WidthState,
    zoomed: bool,
    previous: Option<WidthState>,
}

fn snapshot(client: &impl TmuxClient, window_id: &str) -> Option<Snapshot> {
    let value = client.display(
        window_id,
        &format!(
            "#{{window_width}}|#{{window_height}}|#{{window_zoomed_flag}}|#{{{STATE_OPTION}}}"
        ),
    );
    let mut fields = value.split('|');
    let width = fields.next()?.parse().ok()?;
    let height = fields.next()?.parse().ok()?;
    let zoomed = match fields.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let previous = WidthState::parse(fields.next().unwrap_or_default());
    (width > 0 && height > 0).then_some(Snapshot {
        current: WidthState {
            width,
            height,
            pending: false,
            revision: previous.map_or_else(crate::time::now_epoch_millis, |state| state.revision),
        },
        zoomed,
        previous,
    })
}

fn save(client: &impl TmuxClient, window_id: &str, state: WidthState) -> Result<(), String> {
    client
        .run(&[
            "set-option",
            "-w",
            "-t",
            window_id,
            STATE_OPTION,
            &state.encode(),
        ])
        .ok_or_else(|| "failed to record sidebar window dimensions".to_string())?;
    Ok(())
}

/// New slots start at the configured width. Establish the baseline before
/// queued layout hooks can mistake the first manual edit for a resize.
pub(super) fn initialize(client: &impl TmuxClient, window_id: &str) {
    if let Some(snapshot) = snapshot(client, window_id) {
        let mut state = snapshot.current;
        state.revision = crate::time::now_epoch_millis().max(state.revision.saturating_add(1));
        let _ = save(client, window_id, state);
    }
}

/// Config reload seeds existing installations without altering their layout or
/// discarding pending zoom repairs. Resize hooks never scan other windows.
pub(super) fn initialize_existing(client: &impl TmuxClient) -> Result<(), String> {
    for window_id in topology::owned_window_ids(client)? {
        if let Some(snapshot) = snapshot(client, &window_id)
            && snapshot.previous.is_none()
        {
            save(client, &window_id, snapshot.current)?;
        }
    }
    Ok(())
}

pub(super) fn clear(client: &impl TmuxClient, window_id: &str) {
    let _ = client.run(&["set-option", "-w", "-u", "-t", window_id, STATE_OPTION]);
}

/// Ordinary pane swaps emit layout hooks too. Their captured baseline proves
/// there is no resize to serialize; never queue them behind lifecycle work.
pub(super) fn event_needs_observation(observed: Option<(u32, u32)>, state: Option<&str>) -> bool {
    match (observed, state.and_then(WidthState::parse)) {
        (Some(size), Some(previous)) => {
            previous.pending || size != (previous.width, previous.height)
        }
        _ => true,
    }
}

pub(super) fn observe_event(
    client: &impl TmuxClient,
    window_id: &str,
    observed: Option<(u32, u32)>,
    event_state: Option<&str>,
) -> Result<(), String> {
    // The hook passes a physical window ID, never a session-relative name.
    if window_id
        .strip_prefix('@')
        .and_then(|id| id.parse::<u64>().ok())
        .is_none()
    {
        return Ok(());
    }
    let Some(snapshot) = snapshot(client, window_id) else {
        return Ok(()); // A queued hook may outlive its window.
    };
    let Ok(panes) = topology::owned_panes_in_window(client, window_id) else {
        return Ok(());
    };
    if panes.is_empty() {
        if snapshot.previous.is_some() {
            clear(client, window_id);
        }
        return Ok(());
    }

    let enabled = enabled_intent(client).ok() == Some(SidebarIntent::Enabled);
    let single_client = client
        .run(&["list-clients", "-F", "#{client_name}"])
        .is_some_and(|clients| clients.lines().filter(|line| !line.is_empty()).count() == 1);
    let Some(previous) = snapshot.previous.filter(|_| enabled && single_client) else {
        // Initial observation, disabled intent, and multi-client layouts only
        // establish a baseline. Old deferred repairs cannot escape this guard.
        if snapshot.previous != Some(snapshot.current) {
            let mut next = snapshot.current;
            if snapshot.previous.is_some_and(|previous| {
                (previous.width, previous.height) != (next.width, next.height)
            }) {
                next.revision = next.revision.saturating_add(1);
            }
            save(client, window_id, next)?;
        }
        return Ok(());
    };

    // A captured event describes a transition from its captured revision.
    // After accepting any resize, older queued callbacks cannot expire a newer
    // manual width, even when dimensions have returned to the same values.
    let current_event = event_state
        .and_then(WidthState::parse)
        .is_some_and(|state| state.revision == previous.revision);
    let resized = previous.width != snapshot.current.width
        || previous.height != snapshot.current.height
        || (current_event
            && observed.is_some_and(|size| size != (previous.width, previous.height)));
    let mut next = snapshot.current;
    if resized {
        next.revision = next.revision.saturating_add(1);
    }
    next.pending = snapshot.zoomed && (resized || previous.pending);
    if next != previous {
        // Commit before resize-pane can emit another layout event. This also
        // bounds repairs when tmux clamps an impossible configured width.
        save(client, window_id, next)?;
    }
    if !resized && !previous.pending {
        return Ok(());
    }

    let Some(target) = resolve_sidebar_width(client, window_id)
        .parse::<u32>()
        .ok()
        .filter(|width| *width > 0)
    else {
        return Ok(());
    };
    let target = target.to_string();
    for pane_id in panes {
        if pane_id
            .strip_prefix('%')
            .and_then(|id| id.parse::<u64>().ok())
            .is_some()
            && (next.pending || client.display(&pane_id, "#{pane_width}") != target)
        {
            let completed = WidthState {
                pending: false,
                ..next
            };
            let resize = format!(
                "resize-pane -t {pane_id} -x {target} ; set-option -w -t {window_id} {STATE_OPTION} {}",
                completed.encode()
            );
            let pending = WidthState {
                pending: true,
                ..next
            };
            let defer = format!(
                "set-option -w -t {window_id} {STATE_OPTION} {}",
                pending.encode()
            );
            // Layout repair is best effort; an exited pane or tmux clamp does
            // not interfere with normal lifecycle reconciliation. Evaluate
            // zoom and resize in the same tmux command queue: the user may
            // zoom or unzoom after the snapshot. Complete a repair even when
            // an unzoom callback captured the old, nonpending baseline and
            // was correctly rejected as a no-op before acquiring the lock.
            // All interpolated IDs and the target width are numeric-validated.
            let condition = format!(
                "#{{&&:#{{==:#{{window_id}},{window_id}}},#{{==:#{{window_zoomed_flag}},0}}}}"
            );
            let _ = client.run(&[
                "if-shell", "-F", "-t", &pane_id, &condition, &resize, &defer,
            ]);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    fn observe(client: &impl TmuxClient, window_id: &str) -> Result<(), String> {
        observe_event(client, window_id, None, None)
    }

    struct FixtureTmux {
        dimensions: (u32, u32),
        zoomed: bool,
        clients: &'static str,
        enabled: &'static str,
        setting: &'static str,
        panes: &'static str,
        state: RefCell<String>,
        widths: RefCell<HashMap<String, String>>,
        repairs: RefCell<Vec<String>>,
        fail_save: bool,
        zoom_on_repair: bool,
        unzoom_on_repair: bool,
        moved_on_repair: bool,
    }

    impl Default for FixtureTmux {
        fn default() -> Self {
            Self {
                dimensions: (150, 50),
                zoomed: false,
                clients: "client-a",
                enabled: "on",
                setting: "35",
                panes: "%1|@1|sidebar|101|0|/dev/tty1|101|\n%2|@1|sidebar-slot|0|0|||\n%3|@1|sidebar-slot|103|0|/dev/tty3||\n%4|@1|sidebar|104|0|/dev/tty4|999|\n%5|@1||105|0|/dev/tty5||",
                state: RefCell::new("200,50,0,0".into()),
                widths: RefCell::new(HashMap::from([
                    ("%1".into(), "10".into()),
                    ("%2".into(), "10".into()),
                ])),
                repairs: RefCell::new(Vec::new()),
                fail_save: false,
                zoom_on_repair: false,
                unzoom_on_repair: false,
                moved_on_repair: false,
            }
        }
    }

    impl TmuxClient for FixtureTmux {
        fn run(&self, args: &[&str]) -> Option<String> {
            match args[0] {
                "list-panes" => {
                    if args[1] != "-a" {
                        assert_eq!(&args[..4], &["list-panes", "-t", "@1", "-F"]);
                    }
                    Some(self.panes.into())
                }
                "list-clients" => Some(self.clients.into()),
                "show-option" => Some(self.enabled.into()),
                "display-message" => {
                    let target = args[2];
                    let format = args[4];
                    Some(match format {
                        "#{window_width}" => self.dimensions.0.to_string(),
                        "#{window_zoomed_flag}" => u8::from(self.zoomed).to_string(),
                        "#{@sidebar_width}" => self.setting.into(),
                        "#{pane_width}" => self.widths.borrow().get(target).cloned().unwrap(),
                        _ => format!(
                            "{}|{}|{}|{}",
                            self.dimensions.0,
                            self.dimensions.1,
                            u8::from(self.zoomed),
                            self.state.borrow()
                        ),
                    })
                }
                "set-option" if args.contains(&"-u") => {
                    self.state.borrow_mut().clear();
                    Some(String::new())
                }
                "set-option" => {
                    if self.fail_save {
                        return None;
                    }
                    *self.state.borrow_mut() = args[5].into();
                    Some(String::new())
                }
                "if-shell" => {
                    assert!(args[3].starts_with('%'));
                    assert_eq!(
                        args[4],
                        "#{&&:#{==:#{window_id},@1},#{==:#{window_zoomed_flag},0}}"
                    );
                    if ((self.zoomed || self.zoom_on_repair) && !self.unzoom_on_repair)
                        || self.moved_on_repair
                    {
                        *self.state.borrow_mut() =
                            args[6].split_whitespace().last().unwrap().into();
                        return Some(String::new());
                    }
                    if let Some((_, save)) = args[5].split_once(" ; ") {
                        *self.state.borrow_mut() = save.split_whitespace().last().unwrap().into();
                    }
                    let command: Vec<_> = args[5].split_whitespace().collect();
                    self.repairs.borrow_mut().push(command[2].into());
                    let desired = command[4].parse::<u32>().unwrap();
                    self.widths.borrow_mut().insert(
                        command[2].into(),
                        desired.min(self.dimensions.0.saturating_sub(2)).to_string(),
                    );
                    Some(String::new())
                }
                _ => panic!("unexpected command: {args:?}"),
            }
        }
    }

    #[test]
    fn real_resize_repairs_only_strict_owned_panes_once() {
        let tmux = FixtureTmux::default();
        observe(&tmux, "@1").unwrap();
        assert_eq!(*tmux.repairs.borrow(), vec!["%1", "%2"]);
        assert_eq!(*tmux.state.borrow(), "150,50,0,1");
        observe(&tmux, "@1").unwrap();
        assert_eq!(tmux.repairs.borrow().len(), 2);
    }

    #[test]
    fn captured_resize_survives_a_burst_back_to_the_original_size() {
        let tmux = FixtureTmux {
            dimensions: (200, 50),
            ..FixtureTmux::default()
        };
        // Both delayed hooks see final width 200, but the first carries the
        // intervening resize that changed pane geometry.
        observe_event(&tmux, "@1", Some((120, 50)), Some("200,50,0,0")).unwrap();
        assert_eq!(tmux.widths.borrow()["%1"], "35");
        observe_event(&tmux, "@1", Some((200, 50)), Some("200,50,0,0")).unwrap();
        assert_eq!(tmux.repairs.borrow().len(), 2);
    }

    #[test]
    fn obsolete_callbacks_cannot_expire_a_newer_manual_width() {
        let mut tmux = FixtureTmux {
            dimensions: (200, 50),
            ..FixtureTmux::default()
        };
        observe_event(&tmux, "@1", Some((120, 50)), Some("200,50,0,0")).unwrap();
        assert_eq!(*tmux.state.borrow(), "200,50,0,1");
        // An ABA transition back to the same dimensions must not make the old
        // event current again after the user adjusts the width.
        tmux.widths.borrow_mut().insert("%1".into(), "44".into());
        observe_event(&tmux, "@1", Some((120, 50)), Some("200,50,0,0")).unwrap();
        assert_eq!(tmux.widths.borrow()["%1"], "44");
        tmux.dimensions = (240, 50);
        observe_event(&tmux, "@1", Some((240, 50)), Some("200,50,0,1")).unwrap();
        assert_eq!(*tmux.state.borrow(), "240,50,0,2");
        tmux.widths.borrow_mut().insert("%1".into(), "44".into());
        observe_event(&tmux, "@1", Some((150, 50)), Some("200,50,0,0")).unwrap();
        assert_eq!(tmux.widths.borrow()["%1"], "44");
        assert_eq!(tmux.repairs.borrow().len(), 3);
    }

    #[test]
    fn new_slot_generation_rejects_callbacks_from_previous_ownership() {
        let tmux = FixtureTmux::default();
        initialize(&tmux, "@1");
        let fresh = WidthState::parse(&tmux.state.borrow()).unwrap();
        assert!(fresh.revision > 0);
        tmux.widths.borrow_mut().insert("%1".into(), "44".into());
        observe_event(&tmux, "@1", Some((200, 50)), Some("200,50,0,0")).unwrap();
        assert_eq!(tmux.widths.borrow()["%1"], "44");
    }

    #[test]
    fn reads_legacy_three_field_state_as_revision_zero() {
        let legacy = WidthState::parse("200,50,1").unwrap();
        assert_eq!(legacy.revision, 0);
        assert_eq!(legacy.encode(), "200,50,1,0");
    }

    #[test]
    fn manual_layout_changes_preserve_width_and_first_observation_only_seeds() {
        let tmux = FixtureTmux::default();
        *tmux.state.borrow_mut() = "150,50,0,0".into();
        observe(&tmux, "@1").unwrap();
        assert!(tmux.repairs.borrow().is_empty());
        tmux.state.borrow_mut().clear();
        observe(&tmux, "@1").unwrap();
        assert!(tmux.repairs.borrow().is_empty());
        assert!(tmux.state.borrow().starts_with("150,50,0,"));
        assert!(WidthState::parse(&tmux.state.borrow()).unwrap().revision > 0);
    }

    #[test]
    fn config_reload_initializes_missing_baseline_without_erasing_pending_zoom() {
        let tmux = FixtureTmux::default();
        tmux.state.borrow_mut().clear();
        initialize_existing(&tmux).unwrap();
        assert!(tmux.state.borrow().starts_with("150,50,0,"));
        assert!(WidthState::parse(&tmux.state.borrow()).unwrap().revision > 0);
        assert!(tmux.repairs.borrow().is_empty());
        *tmux.state.borrow_mut() = "200,50,1,0".into();
        initialize_existing(&tmux).unwrap();
        assert_eq!(*tmux.state.borrow(), "200,50,1,0");
    }

    #[test]
    fn zoom_defers_resize_and_unzoom_consumes_it_once() {
        let mut tmux = FixtureTmux {
            zoomed: true,
            ..FixtureTmux::default()
        };
        observe(&tmux, "@1").unwrap();
        assert!(tmux.repairs.borrow().is_empty());
        assert_eq!(*tmux.state.borrow(), "150,50,1,1");
        observe(&tmux, "@1").unwrap();
        assert_eq!(*tmux.state.borrow(), "150,50,1,1");
        tmux.zoomed = false;
        observe(&tmux, "@1").unwrap();
        assert_eq!(tmux.repairs.borrow().len(), 2);
        assert_eq!(*tmux.state.borrow(), "150,50,0,1");
        observe(&tmux, "@1").unwrap();
        assert_eq!(tmux.repairs.borrow().len(), 2);
    }

    #[test]
    fn zoom_without_resize_preserves_manual_width() {
        let mut tmux = FixtureTmux {
            zoomed: true,
            ..FixtureTmux::default()
        };
        *tmux.state.borrow_mut() = "150,50,0,0".into();
        observe(&tmux, "@1").unwrap();
        tmux.zoomed = false;
        observe(&tmux, "@1").unwrap();
        assert!(tmux.repairs.borrow().is_empty());
    }

    #[test]
    fn unzoom_after_snapshot_does_not_depend_on_a_queued_noop_callback() {
        let tmux = FixtureTmux {
            zoomed: true,
            unzoom_on_repair: true,
            ..FixtureTmux::default()
        };
        observe(&tmux, "@1").unwrap();
        assert_eq!(tmux.repairs.borrow().len(), 2);
        assert_eq!(*tmux.state.borrow(), "150,50,0,1");
    }

    #[test]
    fn zoom_entered_after_snapshot_defers_instead_of_unzooming() {
        let mut tmux = FixtureTmux {
            zoom_on_repair: true,
            ..FixtureTmux::default()
        };
        observe(&tmux, "@1").unwrap();
        assert!(tmux.repairs.borrow().is_empty());
        assert_eq!(*tmux.state.borrow(), "150,50,1,1");
        tmux.zoom_on_repair = false;
        observe(&tmux, "@1").unwrap();
        assert_eq!(tmux.repairs.borrow().len(), 2);
        assert_eq!(*tmux.state.borrow(), "150,50,0,1");
    }

    #[test]
    fn pane_moved_to_another_window_is_not_resized() {
        let tmux = FixtureTmux {
            moved_on_repair: true,
            ..FixtureTmux::default()
        };
        observe(&tmux, "@1").unwrap();
        assert!(tmux.repairs.borrow().is_empty());
    }

    #[test]
    fn disabled_or_multiple_clients_discard_deferred_repairs() {
        for (clients, enabled) in [("a\nb", "on"), ("", "on"), ("a", "off")] {
            let mut tmux = FixtureTmux {
                clients,
                enabled,
                ..FixtureTmux::default()
            };
            *tmux.state.borrow_mut() = "200,50,1,0".into();
            observe(&tmux, "@1").unwrap();
            assert!(tmux.repairs.borrow().is_empty());
            assert_eq!(*tmux.state.borrow(), "150,50,0,1");
            tmux.clients = "a";
            tmux.enabled = "on";
            observe(&tmux, "@1").unwrap();
            assert!(tmux.repairs.borrow().is_empty());
        }
    }

    #[test]
    fn percentage_uses_final_width_and_height_only_resize_also_repairs() {
        let tmux = FixtureTmux {
            setting: "20%",
            ..FixtureTmux::default()
        };
        *tmux.state.borrow_mut() = "150,60,0,0".into();
        observe(&tmux, "@1").unwrap();
        assert_eq!(tmux.widths.borrow()["%1"], "30");
    }

    #[test]
    fn clamped_target_cannot_retrigger_repair_and_missing_ownership_clears_state() {
        let mut tmux = FixtureTmux {
            dimensions: (30, 50),
            setting: "100%",
            ..FixtureTmux::default()
        };
        observe(&tmux, "@1").unwrap();
        assert_eq!(tmux.widths.borrow()["%1"], "28");
        observe(&tmux, "@1").unwrap();
        assert_eq!(tmux.repairs.borrow().len(), 2);
        tmux.panes = "%5|@1||105|0|/dev/tty5||";
        observe(&tmux, "@1").unwrap();
        assert!(tmux.state.borrow().is_empty());
    }

    #[test]
    fn state_write_failure_prevents_recursive_repair() {
        let tmux = FixtureTmux {
            fail_save: true,
            ..FixtureTmux::default()
        };
        assert!(observe(&tmux, "@1").is_err());
        assert!(tmux.repairs.borrow().is_empty());
    }
}
