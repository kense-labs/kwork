//! Harness terminal metadata and pure lifecycle projection.
//!
//! Roster updates only detach disappearing Agent terminals. User-created local
//! terminals stay outside the Harness roster and are never closed by it.

use crate::right_dock::{DockSurface, RightPanelTab};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};

/// One embedded terminal described by the authoritative Harness roster.
#[derive(Clone, Default, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentTerminalInfo {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) cwd: String,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) origin: String,
    pub(crate) control: String,
    pub(crate) state: String,
    #[serde(default)]
    pub(crate) sandbox_mode: Option<String>,
    pub(crate) shell: String,
    pub(crate) exit_code: Option<u32>,
}

/// Agent control takes precedence; an unrestricted taken-over terminal has no
/// sandbox badge.
pub(crate) fn agent_terminal_badge(info: &AgentTerminalInfo) -> Option<(&'static str, String)> {
    const AGENT_HINT: &str = "由 Agent 打开。你一输入就由你接管，Agent 之后只能读取。";
    let sandbox_hint = match info.sandbox_mode.as_deref() {
        Some("read-only") => Some("命令在 Agent 的只读沙箱里运行，包括你在这里输入的命令。"),
        Some("workspace-write") => {
            Some("命令在 Agent 的工作区沙箱里运行，包括你在这里输入的命令。")
        }
        _ => None,
    };
    if info.origin != "agent" {
        return None;
    }
    if info.control == "agent" {
        Some((
            "Agent",
            sandbox_hint.map_or_else(
                || AGENT_HINT.to_owned(),
                |hint| format!("{AGENT_HINT} {hint}"),
            ),
        ))
    } else {
        sandbox_hint.map(|hint| ("沙箱", hint.to_owned()))
    }
}

#[derive(Default)]
pub(crate) struct AgentRosterProjection {
    pub(crate) controlled: BTreeSet<String>,
    pub(crate) detached: Vec<(u64, String)>,
    pub(crate) opened: Vec<(u64, AgentTerminalInfo)>,
    pub(crate) updates: Vec<(u64, AgentTerminalInfo)>,
    pub(crate) closed_pane: bool,
}

/// Only the stable terminal id is needed by the pure projection. The GPUI tab
/// implements it in the WASM frontend; native tests use a plain String.
pub(crate) trait TerminalIdentity {
    fn terminal_id(&self) -> &str;
}

impl TerminalIdentity for String {
    fn terminal_id(&self) -> &str {
        self
    }
}

/// Reconcile only one session's dock and open terminals. No foreground UI or
/// lifecycle IPC is accessed here, so a parked session cannot affect the active
/// session's same-named tabs.
pub(crate) fn project_agent_roster(
    surface: &mut DockSurface,
    open_terminals: &mut HashMap<u64, impl TerminalIdentity>,
    agent_origins: &BTreeSet<u64>,
    previous_controlled: &BTreeSet<String>,
    terminals: Vec<AgentTerminalInfo>,
    next_terminal_tab: &mut u64,
) -> AgentRosterProjection {
    let mut projection = AgentRosterProjection::default();
    let controlled: BTreeSet<String> = terminals
        .iter()
        .filter(|info| info.origin == "agent" && info.control == "agent")
        .map(|info| info.id.clone())
        .collect();
    for key in agent_origins {
        let Some(id) = open_terminals
            .get(key)
            .map(|tab| tab.terminal_id().to_owned())
        else {
            continue;
        };
        if controlled.contains(&id) {
            continue;
        }
        // Only the transition out of the previous Agent-controlled set ends
        // its tab. A taken-over terminal remains represented when it leaves
        // the Agent retain set, and a restored tab never invents a prior set.
        if previous_controlled.contains(&id) {
            let tab = RightPanelTab::Terminal(*key);
            let pane = surface
                .layout
                .all_panes()
                .find(|pane| pane.tabs().contains(&tab))
                .map(|pane| pane.id);
            if let Some(pane) = pane {
                projection.closed_pane |= surface.layout.close(pane, tab);
            }
            open_terminals.remove(key);
            surface.terminals.remove(key);
            projection.detached.push((*key, id));
        }
    }
    for info in terminals {
        let key = open_terminals
            .iter()
            .find_map(|(key, id)| (id.terminal_id() == info.id).then_some(*key))
            .or_else(|| {
                surface
                    .terminals
                    .iter()
                    .find_map(|(key, checkpoint)| (checkpoint.id == info.id).then_some(*key))
            })
            .unwrap_or_else(|| {
                if info.origin == "agent" && info.state == "running" {
                    *next_terminal_tab =
                        (*next_terminal_tab).max(surface.layout.last_terminal_key()) + 1;
                    *next_terminal_tab
                } else {
                    0
                }
            });
        if key == 0 {
            continue;
        }
        if info.origin == "agent" && info.state == "running" && !open_terminals.contains_key(&key) {
            projection.opened.push((key, info.clone()));
        } else if open_terminals.contains_key(&key) {
            projection.updates.push((key, info));
        }
    }
    projection.controlled = controlled;
    projection
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::right_dock::{DockPaneId, TerminalCheckpoint};

    fn info(control: &str) -> AgentTerminalInfo {
        AgentTerminalInfo {
            id: "terminal".into(),
            title: "Build".into(),
            cwd: "/workspace".into(),
            cols: 80,
            rows: 24,
            origin: "agent".into(),
            control: control.into(),
            state: "running".into(),
            sandbox_mode: Some("workspace-write".into()),
            shell: "zsh".into(),
            exit_code: None,
        }
    }

    #[test]
    fn badges_match_agent_control_and_restricted_sandbox_rules() {
        let mut value = AgentTerminalInfo {
            origin: "agent".into(),
            control: "agent".into(),
            sandbox_mode: Some("workspace-write".into()),
            ..AgentTerminalInfo::default()
        };
        assert_eq!(
            agent_terminal_badge(&value).map(|badge| badge.0),
            Some("Agent")
        );
        value.control = "user".into();
        assert_eq!(
            agent_terminal_badge(&value),
            Some((
                "沙箱",
                "命令在 Agent 的工作区沙箱里运行，包括你在这里输入的命令。".into()
            ))
        );
        value.sandbox_mode = Some("read-only".into());
        assert_eq!(
            agent_terminal_badge(&value),
            Some((
                "沙箱",
                "命令在 Agent 的只读沙箱里运行，包括你在这里输入的命令。".into()
            ))
        );
        value.sandbox_mode = Some("danger-full-access".into());
        assert_eq!(agent_terminal_badge(&value), None);
        value.control = "agent".into();
        assert_eq!(
            agent_terminal_badge(&value),
            Some((
                "Agent",
                "由 Agent 打开。你一输入就由你接管，Agent 之后只能读取。".into()
            ))
        );
    }

    #[test]
    fn disappearance_covers_taken_over_tabs_and_is_a_detach_not_a_close() {
        let mut surface = DockSurface::default();
        let mut open = HashMap::from([(4, String::from("terminal"))]);
        let origins = BTreeSet::from([4]);
        surface.terminals.insert(
            4,
            TerminalCheckpoint {
                id: "terminal".into(),
                title: Some("Build".into()),
            },
        );
        surface
            .layout
            .open(DockPaneId::Primary, RightPanelTab::Terminal(4));
        let mut next = 4;
        let taken_over = project_agent_roster(
            &mut surface,
            &mut open,
            &origins,
            &BTreeSet::new(),
            vec![info("user")],
            &mut next,
        );
        assert!(taken_over.detached.is_empty());
        assert_eq!(taken_over.updates.len(), 1);
        // A roster update publishes only Agent-controlled retains. The user's
        // terminal may still exist and be detached, but its tab stays open.
        let taken_over = project_agent_roster(
            &mut surface,
            &mut open,
            &origins,
            &BTreeSet::new(),
            vec![],
            &mut next,
        );
        assert!(taken_over.detached.is_empty());
        assert_eq!(open.get(&4).map(String::as_str), Some("terminal"));

        // Agent-controlled disappearance is detach-only, never a process close.
        project_agent_roster(
            &mut surface,
            &mut open,
            &origins,
            &BTreeSet::new(),
            vec![info("agent")],
            &mut next,
        );
        let removed = project_agent_roster(
            &mut surface,
            &mut open,
            &origins,
            &BTreeSet::from([String::from("terminal")]),
            vec![],
            &mut next,
        );
        assert_eq!(removed.detached, vec![(4, "terminal".into())]);
        assert!(removed.closed_pane);
        assert!(open.is_empty());
        assert!(surface.terminals.is_empty());
        assert!(!surface.layout.terminal_keys().any(|key| key == 4));

        // A user takeover seen on the first roster is retained and attached;
        // it need not have an earlier Agent-controlled frame.
        let taken_over = project_agent_roster(
            &mut surface,
            &mut open,
            &origins,
            &BTreeSet::new(),
            vec![info("user")],
            &mut next,
        );
        assert_eq!(taken_over.opened, vec![(5, info("user"))]);
        open.insert(5, String::from("terminal"));
        let taken_over = project_agent_roster(
            &mut surface,
            &mut open,
            &origins,
            &BTreeSet::new(),
            vec![info("user")],
            &mut next,
        );
        assert!(taken_over.detached.is_empty());
        assert_eq!(open.get(&5).map(String::as_str), Some("terminal"));
    }

    #[test]
    fn opening_is_idempotent_and_does_not_reopen_exited_or_local_terminals() {
        let mut surface = DockSurface::default();
        let mut open = HashMap::from([(7, "local".into())]);
        let origins = BTreeSet::new();
        let mut next = 0;
        let first = project_agent_roster(
            &mut surface,
            &mut open,
            &origins,
            &BTreeSet::new(),
            vec![info("agent")],
            &mut next,
        );
        assert_eq!(first.opened, vec![(1, info("agent"))]);
        open.insert(1, String::from("terminal"));
        let second = project_agent_roster(
            &mut surface,
            &mut open,
            &origins,
            &BTreeSet::from([String::from("terminal")]),
            vec![info("agent")],
            &mut next,
        );
        assert!(
            second.opened.is_empty(),
            "a retained terminal id never allocates another key"
        );
        assert_eq!(next, 1);
        let exited = AgentTerminalInfo {
            state: "exited".into(),
            exit_code: Some(0),
            ..info("agent")
        };
        assert!(
            project_agent_roster(
                &mut surface,
                &mut open,
                &origins,
                &BTreeSet::from([String::from("terminal")]),
                vec![exited],
                &mut next
            )
            .opened
            .is_empty()
        );
        assert_eq!(open.get(&7).map(String::as_str), Some("local"));
        assert!(
            project_agent_roster(
                &mut surface,
                &mut open,
                &origins,
                &BTreeSet::new(),
                vec![],
                &mut next
            )
            .detached
            .is_empty()
        );
    }
}
