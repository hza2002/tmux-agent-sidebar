use ratatui::style::Color;

use crate::{
    activity::ToolColorClass,
    tmux::{self, AgentType, PaneStatus},
};

/// Runtime color theme, loaded from tmux @sidebar_color_* variables on startup.
/// Overrides may be xterm-256 indexes or six-digit RGB hex values.
/// Falls back to defaults if tmux variables are not set.
#[derive(Debug, Clone)]
pub struct ColorTheme {
    /// Accent color shared by every "active / focused" affordance:
    /// the `┃` marker on the active pane, the focused repo header, the
    /// bottom panel border when Activity/Git is focused, and the repo
    /// popup border.
    pub accent: Color,
    pub border_inactive: Color,
    pub status_all: Color,
    pub status_running: Color,
    pub status_background: Color,
    pub status_waiting: Color,
    pub status_idle: Color,
    pub status_error: Color,
    pub status_unknown: Color,
    pub filter_inactive: Color,
    pub agent_claude: Color,
    pub agent_codex: Color,
    pub agent_opencode: Color,
    pub pet_body: Color,
    pub pet_eye: Color,
    pub text_active: Color,
    pub text_muted: Color,
    pub text_inactive: Color,
    pub session_header: Color,
    pub port: Color,
    pub wait_reason: Color,
    pub selection_bg: Color,
    pub branch: Color,
    pub badge_danger: Color,
    pub badge_auto: Color,
    pub badge_plan: Color,
    pub task_progress: Color,
    pub subagent: Color,
    pub commit_hash: Color,
    pub diff_added: Color,
    pub diff_deleted: Color,
    pub file_change: Color,
    pub pr_link: Color,
    pub section_title: Color,
    pub activity_timestamp: Color,
    pub response_arrow: Color,
    pub activity_edit: Color,
    pub activity_command: Color,
    pub activity_read: Color,
    pub activity_agent: Color,
    pub activity_network: Color,
    pub activity_interaction: Color,
    pub activity_unknown: Color,
    pub pet_nose: Color,
    pub pet_desk: Color,
    pub pet_chair: Color,
    pub pet_paper: Color,
}

pub const GRUVBOX_DARK2: Color = Color::Rgb(0x50, 0x49, 0x45);
pub const GRUVBOX_DARK4: Color = Color::Rgb(0x7c, 0x6f, 0x64);
pub const GRUVBOX_GRAY: Color = Color::Rgb(0x92, 0x83, 0x74);
pub const GRUVBOX_LIGHT1: Color = Color::Rgb(0xeb, 0xdb, 0xb2);
pub const GRUVBOX_LIGHT3: Color = Color::Rgb(0xbd, 0xae, 0x93);
pub const GRUVBOX_LIGHT4: Color = Color::Rgb(0xa8, 0x99, 0x84);
pub const GRUVBOX_BRIGHT_RED: Color = Color::Rgb(0xfb, 0x49, 0x34);
pub const GRUVBOX_BRIGHT_GREEN: Color = Color::Rgb(0xb8, 0xbb, 0x26);
pub const GRUVBOX_BRIGHT_YELLOW: Color = Color::Rgb(0xfa, 0xbd, 0x2f);
pub const GRUVBOX_BRIGHT_BLUE: Color = Color::Rgb(0x83, 0xa5, 0x98);
pub const GRUVBOX_BRIGHT_AQUA: Color = Color::Rgb(0x8e, 0xc0, 0x7c);
pub const GRUVBOX_MATERIAL_RED: Color = Color::Rgb(0xea, 0x69, 0x62);
pub const GRUVBOX_MATERIAL_ORANGE: Color = Color::Rgb(0xe7, 0x8a, 0x4e);
pub const GRUVBOX_MATERIAL_YELLOW: Color = Color::Rgb(0xd8, 0xa6, 0x57);
pub const GRUVBOX_MATERIAL_GREEN: Color = Color::Rgb(0xa9, 0xb6, 0x65);
pub const GRUVBOX_MATERIAL_AQUA: Color = Color::Rgb(0x89, 0xb4, 0x82);
pub const GRUVBOX_MATERIAL_BLUE: Color = Color::Rgb(0x7d, 0xae, 0xa3);
pub const GRUVBOX_MATERIAL_PURPLE: Color = Color::Rgb(0xd3, 0x86, 0x9b);

impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            accent: GRUVBOX_BRIGHT_YELLOW,
            border_inactive: GRUVBOX_DARK2,
            status_all: GRUVBOX_LIGHT4,
            status_running: GRUVBOX_BRIGHT_GREEN,
            status_background: GRUVBOX_BRIGHT_AQUA,
            status_waiting: GRUVBOX_BRIGHT_YELLOW,
            status_idle: GRUVBOX_BRIGHT_BLUE,
            status_error: GRUVBOX_BRIGHT_RED,
            status_unknown: GRUVBOX_GRAY,
            filter_inactive: GRUVBOX_DARK4,
            agent_claude: GRUVBOX_MATERIAL_ORANGE,
            agent_codex: GRUVBOX_MATERIAL_BLUE,
            agent_opencode: GRUVBOX_MATERIAL_AQUA,
            pet_body: GRUVBOX_MATERIAL_ORANGE,
            pet_eye: GRUVBOX_BRIGHT_GREEN,
            text_active: GRUVBOX_LIGHT1,
            text_muted: GRUVBOX_GRAY,
            text_inactive: GRUVBOX_DARK4,
            session_header: GRUVBOX_LIGHT3,
            port: GRUVBOX_MATERIAL_BLUE,
            wait_reason: GRUVBOX_BRIGHT_YELLOW,
            selection_bg: GRUVBOX_DARK2,
            branch: GRUVBOX_BRIGHT_AQUA,
            badge_danger: GRUVBOX_BRIGHT_RED,
            badge_auto: GRUVBOX_MATERIAL_YELLOW,
            badge_plan: GRUVBOX_MATERIAL_BLUE,
            task_progress: GRUVBOX_MATERIAL_YELLOW,
            subagent: GRUVBOX_MATERIAL_BLUE,
            commit_hash: GRUVBOX_GRAY,
            diff_added: GRUVBOX_MATERIAL_GREEN,
            diff_deleted: GRUVBOX_MATERIAL_RED,
            file_change: GRUVBOX_MATERIAL_YELLOW,
            pr_link: GRUVBOX_MATERIAL_BLUE,
            section_title: GRUVBOX_LIGHT3,
            activity_timestamp: GRUVBOX_DARK4,
            response_arrow: GRUVBOX_MATERIAL_AQUA,
            activity_edit: GRUVBOX_MATERIAL_YELLOW,
            activity_command: GRUVBOX_MATERIAL_GREEN,
            activity_read: GRUVBOX_MATERIAL_BLUE,
            activity_agent: GRUVBOX_MATERIAL_PURPLE,
            activity_network: GRUVBOX_MATERIAL_AQUA,
            activity_interaction: GRUVBOX_MATERIAL_ORANGE,
            activity_unknown: GRUVBOX_GRAY,
            pet_nose: GRUVBOX_MATERIAL_RED,
            pet_desk: GRUVBOX_DARK4,
            pet_chair: GRUVBOX_DARK2,
            pet_paper: GRUVBOX_LIGHT1,
        }
    }
}

impl ColorTheme {
    /// Load colors from tmux @sidebar_color_* variables, falling back to defaults.
    /// Fetches all global options in a single tmux call to avoid N subprocess forks.
    pub fn from_tmux() -> Self {
        let all_opts = tmux::get_all_global_options();
        Self::from_options(&all_opts)
    }

    fn from_options(all_opts: &std::collections::HashMap<String, String>) -> Self {
        let mut theme = Self::default();

        let read = |var: &str, fallback: Color| -> Color {
            all_opts
                .get(var)
                .and_then(|s| parse_tmux_color(s))
                .unwrap_or(fallback)
        };

        theme.accent = read(tmux::SIDEBAR_COLOR_ACCENT, theme.accent);
        theme.border_inactive = read(tmux::SIDEBAR_COLOR_BORDER, theme.border_inactive);
        theme.status_all = read(tmux::SIDEBAR_COLOR_ALL, theme.status_all);
        theme.status_running = read(tmux::SIDEBAR_COLOR_RUNNING, theme.status_running);
        theme.status_background = read(tmux::SIDEBAR_COLOR_BACKGROUND, theme.status_background);
        theme.status_waiting = read(tmux::SIDEBAR_COLOR_WAITING, theme.status_waiting);
        theme.status_idle = read(tmux::SIDEBAR_COLOR_IDLE, theme.status_idle);
        theme.status_error = read(tmux::SIDEBAR_COLOR_ERROR, theme.status_error);
        theme.filter_inactive = read(tmux::SIDEBAR_COLOR_FILTER_INACTIVE, theme.filter_inactive);
        theme.agent_claude = read(tmux::SIDEBAR_COLOR_AGENT_CLAUDE, theme.agent_claude);
        theme.agent_codex = read(tmux::SIDEBAR_COLOR_AGENT_CODEX, theme.agent_codex);
        theme.agent_opencode = read(tmux::SIDEBAR_COLOR_AGENT_OPENCODE, theme.agent_opencode);
        theme.pet_body = read(tmux::SIDEBAR_COLOR_PET_BODY, theme.pet_body);
        theme.pet_eye = read(tmux::SIDEBAR_COLOR_PET_EYE, theme.pet_eye);
        theme.text_active = read(tmux::SIDEBAR_COLOR_TEXT_ACTIVE, theme.text_active);
        theme.text_muted = read(tmux::SIDEBAR_COLOR_TEXT_MUTED, theme.text_muted);
        theme.text_inactive = read(tmux::SIDEBAR_COLOR_TEXT_INACTIVE, theme.text_inactive);
        theme.session_header = read(tmux::SIDEBAR_COLOR_SESSION, theme.session_header);
        theme.port = read(tmux::SIDEBAR_COLOR_PORT, theme.port);
        theme.wait_reason = read(tmux::SIDEBAR_COLOR_WAIT_REASON, theme.wait_reason);
        theme.selection_bg = read(tmux::SIDEBAR_COLOR_SELECTION, theme.selection_bg);
        theme.branch = read(tmux::SIDEBAR_COLOR_BRANCH, theme.branch);
        theme.task_progress = read(tmux::SIDEBAR_COLOR_TASK_PROGRESS, theme.task_progress);
        theme.subagent = read(tmux::SIDEBAR_COLOR_SUBAGENT, theme.subagent);
        theme.commit_hash = read(tmux::SIDEBAR_COLOR_COMMIT_HASH, theme.commit_hash);
        theme.diff_added = read(tmux::SIDEBAR_COLOR_DIFF_ADDED, theme.diff_added);
        theme.diff_deleted = read(tmux::SIDEBAR_COLOR_DIFF_DELETED, theme.diff_deleted);
        theme.file_change = read(tmux::SIDEBAR_COLOR_FILE_CHANGE, theme.file_change);
        theme.pr_link = read(tmux::SIDEBAR_COLOR_PR_LINK, theme.pr_link);
        theme.section_title = read(tmux::SIDEBAR_COLOR_SECTION_TITLE, theme.section_title);
        theme.activity_timestamp = read(
            tmux::SIDEBAR_COLOR_ACTIVITY_TIMESTAMP,
            theme.activity_timestamp,
        );
        theme.response_arrow = read(tmux::SIDEBAR_COLOR_RESPONSE_ARROW, theme.response_arrow);

        theme
    }

    pub fn status_color(&self, status: &PaneStatus, attention: bool) -> Color {
        if attention {
            return self.status_waiting;
        }
        match status {
            PaneStatus::Running => self.status_running,
            PaneStatus::Background => self.status_background,
            PaneStatus::Waiting => self.status_waiting,
            PaneStatus::Idle => self.status_idle,
            PaneStatus::Error => self.status_error,
            PaneStatus::Unknown => self.status_unknown,
        }
    }

    pub fn agent_color(&self, agent: &AgentType) -> Color {
        match agent {
            AgentType::Claude => self.agent_claude,
            AgentType::Codex => self.agent_codex,
            AgentType::OpenCode => self.agent_opencode,
            AgentType::Unknown => self.status_unknown,
        }
    }

    pub fn activity_color(&self, class: ToolColorClass) -> Color {
        match class {
            ToolColorClass::Edit => self.activity_edit,
            ToolColorClass::Command => self.activity_command,
            ToolColorClass::Read => self.activity_read,
            ToolColorClass::Agent => self.activity_agent,
            ToolColorClass::Network => self.activity_network,
            ToolColorClass::Interaction => self.activity_interaction,
            ToolColorClass::Unknown => self.activity_unknown,
        }
    }
}

fn parse_tmux_color(value: &str) -> Option<Color> {
    let value = value.trim();
    if let Ok(index) = value.parse::<u8>() {
        return Some(Color::Indexed(index));
    }

    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::Rgb(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn status_color_attention_overrides() {
        let theme = ColorTheme::default();
        // attention=true should always return status_waiting regardless of status
        assert_eq!(
            theme.status_color(&PaneStatus::Idle, true),
            theme.status_waiting
        );
        assert_eq!(
            theme.status_color(&PaneStatus::Running, true),
            theme.status_waiting
        );
        assert_eq!(
            theme.status_color(&PaneStatus::Error, true),
            theme.status_waiting
        );
    }

    #[test]
    fn status_color_normal() {
        let theme = ColorTheme::default();
        assert_eq!(
            theme.status_color(&PaneStatus::Running, false),
            GRUVBOX_BRIGHT_GREEN
        );
        assert_eq!(
            theme.status_color(&PaneStatus::Background, false),
            GRUVBOX_BRIGHT_AQUA
        );
        assert_eq!(
            theme.status_color(&PaneStatus::Waiting, false),
            GRUVBOX_BRIGHT_YELLOW
        );
        assert_eq!(
            theme.status_color(&PaneStatus::Idle, false),
            GRUVBOX_BRIGHT_BLUE
        );
        assert_eq!(
            theme.status_color(&PaneStatus::Error, false),
            GRUVBOX_BRIGHT_RED
        );
        assert_eq!(
            theme.status_color(&PaneStatus::Unknown, false),
            GRUVBOX_GRAY
        );
    }

    #[test]
    fn agent_color_all() {
        let theme = ColorTheme::default();
        assert_eq!(
            theme.agent_color(&AgentType::Claude),
            GRUVBOX_MATERIAL_ORANGE
        );
        assert_eq!(theme.agent_color(&AgentType::Codex), GRUVBOX_MATERIAL_BLUE);
        assert_eq!(
            theme.agent_color(&AgentType::OpenCode),
            GRUVBOX_MATERIAL_AQUA
        );
        assert_eq!(theme.agent_color(&AgentType::Unknown), theme.status_unknown);
    }

    #[test]
    fn pet_color_defaults_match_gruvbox_palette() {
        let theme = ColorTheme::default();
        assert_eq!(theme.pet_body, GRUVBOX_MATERIAL_ORANGE);
        assert_eq!(theme.pet_eye, GRUVBOX_BRIGHT_GREEN);
    }

    #[test]
    fn from_options_accepts_hex_and_indexed_colors() {
        let mut options = std::collections::HashMap::new();
        options.insert(
            tmux::SIDEBAR_COLOR_ACCENT.to_string(),
            "#1a2b3c".to_string(),
        );
        options.insert(
            tmux::SIDEBAR_COLOR_AGENT_CODEX.to_string(),
            "d0e7ff".to_string(),
        );
        options.insert(tmux::SIDEBAR_COLOR_BORDER.to_string(), "42".to_string());

        let theme = ColorTheme::from_options(&options);

        assert_eq!(theme.accent, Color::Rgb(0x1a, 0x2b, 0x3c));
        assert_eq!(theme.agent_codex, Color::Rgb(0xd0, 0xe7, 0xff));
        assert_eq!(theme.border_inactive, Color::Indexed(42));
    }

    #[test]
    fn from_options_falls_back_for_invalid_colors() {
        let mut options = std::collections::HashMap::new();
        options.insert(tmux::SIDEBAR_COLOR_ACCENT.to_string(), "#12".to_string());
        options.insert(
            tmux::SIDEBAR_COLOR_AGENT_CLAUDE.to_string(),
            "not-a-color".to_string(),
        );
        options.insert(tmux::SIDEBAR_COLOR_BORDER.to_string(), "256".to_string());

        let theme = ColorTheme::from_options(&options);
        let default_theme = ColorTheme::default();

        assert_eq!(theme.accent, default_theme.accent);
        assert_eq!(theme.agent_claude, default_theme.agent_claude);
        assert_eq!(theme.border_inactive, default_theme.border_inactive);
    }
}
