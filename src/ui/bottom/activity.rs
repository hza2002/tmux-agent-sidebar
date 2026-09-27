use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::activity::ActivityEntry;
use crate::state::AppState;
use crate::ui::colors::ColorTheme;
use crate::ui::text::{display_width, truncate_to_width};

use super::syntax;

/// Cursor column. Every activity row starts with it so the entries stay in one
/// column; only the entry the cursor points at paints the marker.
const CURSOR_COLUMN: usize = 1;

/// Cursor marker, the same bar the agent list uses for the row it is on.
const CURSOR_MARKER: &str = "┃";

/// Longest tool name a non-command row keeps. The name is the only thing
/// separating a filename from a pattern, but a 24-column MCP name would eat the
/// label it introduces. MCP names are shortened before this applies (see
/// [`display_tool_name`]), so the cap is about native tool names.
const TOOL_NAME_MAX: usize = 12;

/// Tool name as the row shows it. An MCP name is `mcp__<server>__<tool>`, and
/// the row keeps the longest `__`-aligned suffix that fits the column: the
/// server stays visible when it can (`mcp__evil__Read` reads as `evil__Read`,
/// so a scoped tool is never mistaken for the native `Read`), and only the tool
/// name is left when the server is too long to fit
/// (`mcp__plugin-kimi-cu_mac__click` reads as `click`). The full name stays in
/// the log; this is display only.
fn display_tool_name(tool: &str) -> &str {
    if !tool.starts_with("mcp__") {
        return tool;
    }
    let mut cursor = tool;
    while let Some(index) = cursor.find("__") {
        let candidate = &cursor[index + 2..];
        if !candidate.is_empty() && display_width(candidate) <= TOOL_NAME_MAX {
            return candidate;
        }
        cursor = candidate;
    }
    tool
}

/// `max_lines` for the cursor entry: it wraps as far as the label needs, so a
/// long command can be read in full instead of being cut off at a fixed height.
const WRAP_FULLY: usize = usize::MAX;

pub(super) fn draw_activity_content(frame: &mut Frame, state: &mut AppState, inner: Rect) {
    if state.activity.entries.is_empty() {
        super::render_centered(frame, inner, "No activity yet", state.theme.text_muted);
        return;
    }

    let content = content(state, inner.width as usize);

    state.activity.scroll.total_lines = content.lines.len();
    state.activity.scroll.visible_height = inner.height as usize;
    // The cursor anchors the view: the highlighted entry is always on screen,
    // which is what makes `y` copy the command the reader is looking at.
    scroll_cursor_into_view(state, &content);
    // Clamp `offset` to the new total/visible. When entries shrink
    // (focus change, log trim) the stale offset would otherwise produce
    // an empty or over-scrolled paragraph.
    state.activity.scroll.scroll(0);

    let scroll_offset = state.activity.scroll.offset as u16;
    let paragraph = Paragraph::new(content.lines).scroll((scroll_offset, 0));
    frame.render_widget(paragraph, inner);
}

/// Rows the activity tab wants in its content area at `inner_w`.
pub(super) fn content_height(state: &AppState, inner_w: usize) -> u16 {
    if state.activity.entries.is_empty() {
        return 1;
    }
    content(state, inner_w).lines.len() as u16
}

/// The rendered block: the lines plus the entry each line belongs to, which is
/// what lets the cursor scroll itself into view.
struct ActivityContent {
    lines: Vec<Line<'static>>,
    entry_of_line: Vec<usize>,
}

/// Build the activity block at `inner_w`, without rendering. Split out so the
/// tab band can size itself to its content before it draws.
///
/// One row per entry, except the entry the cursor is on: that one wraps as far
/// as it needs to, so the command being read is shown in full without every
/// other entry paying for it. It wraps whether or not the sidebar has the
/// keyboard — the point of the cursor is that it marks what is being read, and
/// reading a log is not something that only happens while the block is focused.
fn content(state: &AppState, inner_w: usize) -> ActivityContent {
    // One column belongs to the cursor for every row, so the entries line up.
    let text_w = inner_w.saturating_sub(CURSOR_COLUMN);
    ActivityBuilder::new(text_w, state.activity.selected, &state.theme)
        .build(&state.activity.entries)
}

/// Accumulates the rows of one block. The builder carries the state every row
/// shares so `push_entry` stays about the entry it is laying out.
struct ActivityBuilder<'a> {
    lines: Vec<Line<'static>>,
    entry_of_line: Vec<usize>,
    width: usize,
    selected: usize,
    theme: &'a ColorTheme,
}

impl<'a> ActivityBuilder<'a> {
    fn new(width: usize, selected: usize, theme: &'a ColorTheme) -> Self {
        Self {
            lines: Vec::new(),
            entry_of_line: Vec::new(),
            width,
            selected,
            theme,
        }
    }

    fn build(mut self, entries: &[ActivityEntry]) -> ActivityContent {
        for (index, entry) in entries.iter().enumerate() {
            self.push_entry(index, entry);
        }
        ActivityContent {
            lines: self.lines,
            entry_of_line: self.entry_of_line,
        }
    }

    /// Push one entry: `HH:MM command…`, or `HH:MM Tool label…` when the label
    /// is not self-describing.
    ///
    /// A shell label *is* the command, so a `Bash` column would only take width
    /// away from the thing being read; a file tool's label is a basename, where
    /// the tool name is the only thing that says whether it was read or
    /// written. Gaps are single spaces: nothing is padded into columns.
    fn push_entry(&mut self, index: usize, entry: &ActivityEntry) {
        let selected = index == self.selected;
        let mut prefix: Vec<Span<'static>> = vec![Span::styled(
            entry.timestamp.clone(),
            Style::default().fg(self.theme.activity_timestamp),
        )];
        let mut prefix_w = display_width(&entry.timestamp) + 1;
        prefix.push(Span::raw(" "));
        if !entry.is_command_tool() && !entry.tool.is_empty() {
            let tool = truncate_to_width(display_tool_name(&entry.tool), TOOL_NAME_MAX);
            prefix_w += display_width(&tool) + 1;
            prefix.push(Span::styled(
                tool,
                Style::default().fg(self.theme.activity_color(entry.tool_color_class())),
            ));
            prefix.push(Span::raw(" "));
        }

        let max_lines = if selected { WRAP_FULLY } else { 1 };
        let wrapped = if entry.label.is_empty() {
            Vec::new()
        } else {
            // The first row pays for `HH:MM` (and the tool name); the rows
            // below it start flush at the gutter and use the whole width.
            let first_w = self.width.saturating_sub(prefix_w).max(1);
            let rest_w = self.width.max(1);
            syntax::wrap_spans(&label_spans(entry, self.theme), first_w, rest_w, max_lines)
        };

        if wrapped.is_empty() {
            let mut spans = vec![cursor_span(selected, self.theme)];
            spans.extend(prefix);
            self.push_line(spans, index);
            return;
        }

        for (line_index, mut chunk) in wrapped.into_iter().enumerate() {
            let mut spans = vec![cursor_span(selected, self.theme)];
            if line_index == 0 {
                spans.extend(prefix.clone());
            } else {
                // A wrap can land on a space; drop it so continuation rows
                // start on the text instead of one column in.
                if let Some(first) = chunk.first_mut()
                    && let Some(rest) = first.content.strip_prefix(' ')
                {
                    first.content = rest.to_string().into();
                }
            }
            spans.extend(chunk);
            self.push_line(spans, index);
        }
    }

    fn push_line(&mut self, spans: Vec<Span<'static>>, index: usize) {
        self.lines.push(Line::from(spans));
        self.entry_of_line.push(index);
    }
}

/// Scroll so the whole cursor entry is inside the viewport, mirroring how the
/// agent list keeps its selected pane visible.
fn scroll_cursor_into_view(state: &mut AppState, content: &ActivityContent) {
    let visible = state.activity.scroll.visible_height;
    if visible == 0 {
        return;
    }
    let selected = state.activity.selected;
    let mut first: Option<usize> = None;
    let mut last: Option<usize> = None;
    for (line, entry) in content.entry_of_line.iter().enumerate() {
        if *entry == selected {
            if first.is_none() {
                first = Some(line);
            }
            last = Some(line);
        }
    }
    let (Some(first), Some(last)) = (first, last) else {
        return;
    };
    let offset = state.activity.scroll.offset;
    if last - first + 1 >= visible {
        // The cursor entry is taller than the viewport, so both ends cannot be
        // shown. Align its head: the head is where the command starts being
        // read, and the tail-aligned alternative ("scroll it into view" applied
        // literally) hides the beginning of exactly the entry the reader asked
        // for.
        state.activity.scroll.offset = first;
    } else if first < offset {
        state.activity.scroll.offset = first.saturating_sub(1);
    } else if last >= offset + visible {
        state.activity.scroll.offset = (last + 1).saturating_sub(visible);
    }
}

/// The cursor column for one row. The cursor is marked by the guide line alone:
/// a background across the entry fights with the syntax colors and costs more
/// legibility than it buys, and the marker already spans every row of the entry.
fn cursor_span(selected: bool, theme: &ColorTheme) -> Span<'static> {
    if !selected {
        return Span::raw(" ");
    }
    Span::styled(CURSOR_MARKER.to_string(), Style::default().fg(theme.accent))
}

/// Shell tools log a command line; everything else logs a basename, a glob, a
/// URL, or a paragraph, and wants no command syntax painted onto it.
fn label_spans(entry: &ActivityEntry, theme: &ColorTheme) -> Vec<Span<'static>> {
    if entry.is_command_tool() {
        syntax::command_spans(&entry.label, theme)
    } else {
        syntax::plain_spans(&entry.label, theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_keeps_the_server_when_it_fits() {
        // A server and tool that fit together are both kept, so a scoped tool
        // cannot read as the native tool of the same name.
        assert_eq!(display_tool_name("mcp__evil__Read"), "evil__Read");
        assert_eq!(display_tool_name("mcp__exa__search"), "exa__search");
    }

    #[test]
    fn display_name_drops_a_server_that_cannot_fit() {
        assert_eq!(display_tool_name("mcp__context7__query-docs"), "query-docs");
        assert_eq!(display_tool_name("mcp__exa__query-docs"), "query-docs");
        assert_eq!(display_tool_name("mcp__plugin-kimi-cu_mac__click"), "click");
    }

    #[test]
    fn display_name_leaves_native_and_malformed_names_alone() {
        assert_eq!(display_tool_name("Read"), "Read");
        assert_eq!(display_tool_name("__task_reset__"), "__task_reset__");
        // A trailing separator leaves the server segment as the only suffix.
        assert_eq!(display_tool_name("mcp__server__"), "server__");
        // No separator after the prefix: nothing to shorten to.
        assert_eq!(
            display_tool_name("mcp__a-very-long-server-name"),
            "mcp__a-very-long-server-name"
        );
    }
}
