use std::collections::{HashMap, HashSet};

use crate::tmux;

use super::{
    SidebarPane, SidebarPosition, TmuxClient, resolve_sidebar_width, split_window_flags,
    target_pane_for_position,
};

const SIDEBAR_ROLE: &str = "sidebar";
const SLOT_ROLE: &str = "sidebar-slot";
pub(super) const SIDEBAR_OWNER_TOKEN: &str = "tmux-agent-sidebar";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SlotPane {
    pub(super) pane_id: String,
    pub(super) window_id: String,
}

pub(super) struct SlotTarget {
    pub(super) slot: SlotPane,
    pub(super) return_pane: String,
}

#[derive(Clone, Debug)]
struct PaneRecord {
    pane_id: String,
    window_id: String,
    role: String,
    pid: u32,
    dead: bool,
    tty: String,
    sidebar_pid: Option<u32>,
    sidebar_owner: String,
}

impl PaneRecord {
    fn is_empty_slot(&self) -> bool {
        self.role == SLOT_ROLE && self.pid == 0 && !self.dead && self.tty.is_empty()
    }

    fn is_sidebar(&self) -> bool {
        self.role == SIDEBAR_ROLE
            && (self.sidebar_owner == SIDEBAR_OWNER_TOKEN
                || (self.sidebar_pid == Some(self.pid) && self.pid > 0))
    }
}

#[derive(Default)]
struct Inventory {
    panes: Vec<PaneRecord>,
}

impl Inventory {
    fn query(client: &impl TmuxClient) -> Result<Self, String> {
        let format = format!(
            "#{{pane_id}}|#{{window_id}}|#{{{}}}|#{{pane_pid}}|#{{pane_dead}}|#{{pane_tty}}|#{{{}}}|#{{{}}}",
            tmux::PANE_ROLE,
            tmux::SIDEBAR_PID,
            tmux::SIDEBAR_OWNER,
        );
        let output = client
            .run(&["list-panes", "-a", "-F", &format])
            .ok_or_else(|| "failed to query sidebar topology".to_string())?;
        let mut seen = HashSet::new();
        let panes = output
            .lines()
            .filter_map(parse_record)
            .filter(|pane| seen.insert(pane.pane_id.clone()))
            .collect();
        Ok(Self { panes })
    }

    fn sidebars(&self) -> Vec<SidebarPane> {
        let mut panes: Vec<_> = self
            .panes
            .iter()
            .filter(|pane| pane.is_sidebar())
            .map(|pane| SidebarPane {
                pane_id: pane.pane_id.clone(),
                window_id: pane.window_id.clone(),
                process_alive: pane.pid > 0 && !pane.dead && !pane.tty.is_empty(),
            })
            .collect();
        panes.sort_by_key(|pane| pane_numeric_id(&pane.pane_id));
        panes
    }

    fn slots(&self) -> Vec<SlotPane> {
        let mut panes: Vec<_> = self
            .panes
            .iter()
            .filter(|pane| pane.is_empty_slot())
            .map(|pane| SlotPane {
                pane_id: pane.pane_id.clone(),
                window_id: pane.window_id.clone(),
            })
            .collect();
        panes.sort_by_key(|pane| pane_numeric_id(&pane.pane_id));
        panes
    }

    fn ordinary_pane(&self, window_id: &str, preferred: &str) -> Option<String> {
        self.panes
            .iter()
            .filter(|pane| {
                pane.window_id == window_id && !pane.is_empty_slot() && !pane.is_sidebar()
            })
            .min_by_key(|pane| {
                if pane.pane_id == preferred {
                    0
                } else {
                    pane_numeric_id(&pane.pane_id).saturating_add(1)
                }
            })
            .map(|pane| pane.pane_id.clone())
    }
}

fn parse_record(line: &str) -> Option<PaneRecord> {
    let mut fields = line.splitn(8, '|');
    Some(PaneRecord {
        pane_id: fields.next()?.to_string(),
        window_id: fields.next()?.to_string(),
        role: fields.next()?.to_string(),
        pid: fields.next()?.parse().ok()?,
        dead: fields.next()? == "1",
        tty: fields.next()?.to_string(),
        sidebar_pid: fields.next().and_then(|pid| pid.parse().ok()),
        sidebar_owner: fields.next().unwrap_or_default().to_string(),
    })
}

fn pane_numeric_id(pane_id: &str) -> u64 {
    pane_id
        .strip_prefix('%')
        .and_then(|id| id.parse().ok())
        .unwrap_or(u64::MAX)
}

pub(super) fn list_sidebars(client: &impl TmuxClient) -> Result<Vec<SidebarPane>, String> {
    let inventory = Inventory::query(client)?;
    normalize_invalid_sidebar_roles(client, &inventory)?;
    Ok(inventory.sidebars())
}

pub(super) fn ensure_slot(
    client: &impl TmuxClient,
    window_id: &str,
    preferred_return: &str,
) -> Result<SlotTarget, String> {
    let inventory = Inventory::query(client)?;
    normalize_invalid_sidebar_roles(client, &inventory)?;
    normalize_invalid_slot_roles(client, &inventory)?;
    let return_pane = inventory
        .ordinary_pane(window_id, preferred_return)
        .ok_or_else(|| "target window has no ordinary pane".to_string())?;

    let mut slots = inventory
        .slots()
        .into_iter()
        .filter(|slot| slot.window_id == window_id);
    if let Some(slot) = slots.next() {
        for duplicate in slots {
            kill_owned_slot(client, &duplicate)?;
        }
        return Ok(SlotTarget { slot, return_pane });
    }

    create_slot(client, window_id).map(|slot| SlotTarget { slot, return_pane })
}

fn kill_owned_slot(client: &impl TmuxClient, slot: &SlotPane) -> Result<(), String> {
    let killed = client.run(&["kill-pane", "-t", &slot.pane_id]).is_some();
    if !killed && client.display(&slot.pane_id, "#{pane_id}") == slot.pane_id {
        return Err(format!("failed to remove sidebar slot {}", slot.pane_id));
    }
    Ok(())
}

fn create_slot(client: &impl TmuxClient, window_id: &str) -> Result<SlotPane, String> {
    let position = SidebarPosition::from_setting(
        &client.display(window_id, &format!("#{{{}}}", tmux::SIDEBAR_POSITION)),
    );
    let geometry = client
        .run(&[
            "list-panes",
            "-t",
            window_id,
            "-F",
            "#{pane_left} #{pane_width} #{pane_id}",
        ])
        .ok_or_else(|| "failed to query slot target panes".to_string())?;
    let target = target_pane_for_position(&geometry, position)
        .ok_or_else(|| "target window has no pane for sidebar slot".to_string())?;
    let width = resolve_sidebar_width(client, window_id);
    let pane_id = client
        .run(&[
            "split-window",
            split_window_flags(position),
            "-d",
            "-l",
            &width,
            "-t",
            &target,
            "-P",
            "-F",
            "#{pane_id}",
            "",
        ])
        .map(|pane| pane.trim().to_string())
        .filter(|pane| !pane.is_empty())
        .ok_or_else(|| "failed to create empty sidebar slot".to_string())?;
    if client
        .run(&[
            "set-option",
            "-p",
            "-t",
            &pane_id,
            tmux::PANE_ROLE,
            SLOT_ROLE,
        ])
        .is_none()
    {
        let _ = client.run(&["kill-pane", "-t", &pane_id]);
        return Err("failed to mark empty sidebar slot".into());
    }

    let verified = Inventory::query(client)?
        .slots()
        .into_iter()
        .find(|slot| slot.pane_id == pane_id && slot.window_id == window_id);
    verified.ok_or_else(|| {
        let _ = client.run(&["kill-pane", "-t", &pane_id]);
        "created sidebar slot is not an empty pane".to_string()
    })
}

fn normalize_invalid_slot_roles(
    client: &impl TmuxClient,
    inventory: &Inventory,
) -> Result<(), String> {
    for pane in &inventory.panes {
        if pane.role == SLOT_ROLE
            && !pane.is_empty_slot()
            && client
                .run(&[
                    "set-option",
                    "-p",
                    "-u",
                    "-t",
                    &pane.pane_id,
                    tmux::PANE_ROLE,
                ])
                .is_none()
        {
            return Err(format!(
                "failed to release invalid sidebar slot role from {}",
                pane.pane_id
            ));
        }
    }
    Ok(())
}

fn normalize_invalid_sidebar_roles(
    client: &impl TmuxClient,
    inventory: &Inventory,
) -> Result<(), String> {
    for pane in &inventory.panes {
        if pane.role == SIDEBAR_ROLE
            && !pane.is_sidebar()
            && client
                .run(&[
                    "set-option",
                    "-p",
                    "-u",
                    "-t",
                    &pane.pane_id,
                    tmux::PANE_ROLE,
                ])
                .is_none()
        {
            return Err(format!(
                "failed to release invalid sidebar role from {}",
                pane.pane_id
            ));
        }
    }
    Ok(())
}

pub(super) fn cleanup_slots(client: &impl TmuxClient) -> Result<(), String> {
    let inventory = Inventory::query(client)?;
    normalize_invalid_sidebar_roles(client, &inventory)?;
    normalize_invalid_slot_roles(client, &inventory)?;
    for slot in inventory.slots() {
        kill_owned_slot(client, &slot)?;
    }
    Ok(())
}

pub(super) fn cleanup_slot_only_windows(client: &impl TmuxClient) -> Result<(), String> {
    let inventory = Inventory::query(client)?;
    normalize_invalid_sidebar_roles(client, &inventory)?;
    normalize_invalid_slot_roles(client, &inventory)?;
    let mut windows: HashMap<&str, Vec<&PaneRecord>> = HashMap::new();
    for pane in &inventory.panes {
        windows.entry(&pane.window_id).or_default().push(pane);
    }
    for panes in windows.values() {
        if !panes.is_empty() && panes.iter().all(|pane| pane.is_empty_slot()) {
            for pane in panes {
                kill_owned_slot(
                    client,
                    &SlotPane {
                        pane_id: pane.pane_id.clone(),
                        window_id: pane.window_id.clone(),
                    },
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn window_has_ordinary_pane(
    client: &impl TmuxClient,
    window_id: &str,
) -> Result<bool, String> {
    let inventory = Inventory::query(client)?;
    normalize_invalid_sidebar_roles(client, &inventory)?;
    Ok(inventory
        .panes
        .iter()
        .any(|pane| pane.window_id == window_id && !pane.is_empty_slot() && !pane.is_sidebar()))
}

pub(super) fn first_ordinary_target(
    client: &impl TmuxClient,
    excluded_window: &str,
) -> Result<Option<(String, String)>, String> {
    let inventory = Inventory::query(client)?;
    normalize_invalid_sidebar_roles(client, &inventory)?;
    Ok(inventory
        .panes
        .iter()
        .filter(|pane| {
            pane.window_id != excluded_window && !pane.is_empty_slot() && !pane.is_sidebar()
        })
        .min_by_key(|pane| pane_numeric_id(&pane.pane_id))
        .map(|pane| (pane.window_id.clone(), pane.pane_id.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque};

    #[derive(Default)]
    struct FixtureTmux {
        responses: RefCell<VecDeque<Option<String>>>,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl FixtureTmux {
        fn with_responses(responses: impl IntoIterator<Item = Option<&'static str>>) -> Self {
            Self {
                responses: RefCell::new(
                    responses
                        .into_iter()
                        .map(|response| response.map(str::to_string))
                        .collect(),
                ),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn called(&self, expected: &[&str]) -> bool {
            self.calls
                .borrow()
                .iter()
                .any(|call| call.iter().map(String::as_str).eq(expected.iter().copied()))
        }
    }

    impl TmuxClient for FixtureTmux {
        fn run(&self, args: &[&str]) -> Option<String> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|arg| arg.to_string()).collect());
            self.responses.borrow_mut().pop_front().flatten()
        }
    }

    #[test]
    fn parses_only_strict_empty_slot_shape() {
        let empty = parse_record("%2|@1|sidebar-slot|0|0|||").unwrap();
        let live = parse_record("%3|@1|sidebar-slot|123|0|/dev/ttys001||").unwrap();
        let dead = parse_record("%4|@1|sidebar-slot|123|1|||").unwrap();
        assert!(empty.is_empty_slot());
        assert!(!live.is_empty_slot());
        assert!(!dead.is_empty_slot());
    }

    #[test]
    fn reuses_owned_empty_slot_without_querying_geometry() {
        let tmux = FixtureTmux::with_responses([Some(
            "%1|@1||101|0|/dev/ttys001|||\n%2|@1|sidebar-slot|0|0|||",
        )]);

        let target = ensure_slot(&tmux, "@1", "%1").unwrap();

        assert_eq!(target.slot.pane_id, "%2");
        assert_eq!(target.return_pane, "%1");
        assert_eq!(tmux.calls.borrow().len(), 1);
        assert_eq!(tmux.calls.borrow()[0][0], "list-panes");
    }

    #[test]
    fn invalid_slot_role_is_removed_without_killing_the_process() {
        let tmux = FixtureTmux::with_responses([Some("")]);
        let inventory = Inventory {
            panes: vec![parse_record("%3|@1|sidebar-slot|123|0|/dev/ttys003||").unwrap()],
        };

        normalize_invalid_slot_roles(&tmux, &inventory).unwrap();

        assert!(tmux.called(&["set-option", "-p", "-u", "-t", "%3", tmux::PANE_ROLE,]));
        assert!(
            !tmux
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "kill-pane")
        );
    }

    #[test]
    fn invalid_sidebar_role_is_removed_without_killing_the_process() {
        let tmux = FixtureTmux::with_responses([Some("")]);
        let inventory = Inventory {
            panes: vec![parse_record("%3|@1|sidebar|123|0|/dev/ttys003|999|").unwrap()],
        };

        normalize_invalid_sidebar_roles(&tmux, &inventory).unwrap();

        assert!(tmux.called(&["set-option", "-p", "-u", "-t", "%3", tmux::PANE_ROLE,]));
        assert!(
            !tmux
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "kill-pane")
        );
    }

    #[test]
    fn slot_only_window_is_released() {
        let tmux = FixtureTmux::with_responses([
            Some("%3|@1|sidebar-slot|0|0|||\n%4|@2||404|0|/dev/ttys004||"),
            Some(""),
        ]);

        cleanup_slot_only_windows(&tmux).unwrap();

        assert!(tmux.called(&["kill-pane", "-t", "%3"]));
        assert!(!tmux.called(&["kill-pane", "-t", "%4"]));
    }
}
