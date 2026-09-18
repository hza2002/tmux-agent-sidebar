use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::activity::ActivityEntry;
use crate::state::{AppState, BottomTab, Focus};
use crate::ui::colors::ColorTheme;
use crate::ui::text::{display_width, truncate_to_width};

use super::syntax;

/// Cursor column. Every activity row starts with it so the entries stay in one
/// column; only the entry the cursor points at paints the marker.
const CURSOR_COLUMN: usize = 1;

/// Cursor marker, the same bar the agent list uses for the row it is on.
const CURSOR_MARKER: &str = "┃";

/// Indent of a wrapped entry's continuation rows.
const CONTINUATION_INDENT: usize = 2;

/// Longest tool name a non-command row keeps. The name is the only thing
/// separating a filename from a pattern, but a 24-column MCP name would eat the
/// label it introduces.
const TOOL_NAME_MAX: usize = 12;

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
/// One row per entry, except the cursor entry: while the block owns the
/// keyboard that entry wraps as far as it needs to, so the command being read
/// is shown in full without every other entry paying for it.
fn content(state: &AppState, inner_w: usize) -> ActivityContent {
    // One column belongs to the cursor for every row, so the entries line up.
    let text_w = inner_w.saturating_sub(CURSOR_COLUMN);
    ActivityBuilder::new(
        text_w,
        state.activity.selected,
        expanded(state),
        &state.theme,
    )
    .build(&state.activity.entries)
}

/// Accumulates the rows of one block. The builder carries the state every row
/// shares so `push_entry` stays about the entry it is laying out.
struct ActivityBuilder<'a> {
    lines: Vec<Line<'static>>,
    entry_of_line: Vec<usize>,
    width: usize,
    selected: usize,
    cursor_wraps: bool,
    theme: &'a ColorTheme,
}

impl<'a> ActivityBuilder<'a> {
    fn new(width: usize, selected: usize, cursor_wraps: bool, theme: &'a ColorTheme) -> Self {
        Self {
            lines: Vec::new(),
            entry_of_line: Vec::new(),
            width,
            selected,
            cursor_wraps,
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
            let tool = truncate_to_width(&entry.tool, TOOL_NAME_MAX);
            prefix_w += display_width(&tool) + 1;
            prefix.push(Span::styled(
                tool,
                Style::default().fg(self.theme.activity_color(entry.tool_color_class())),
            ));
            prefix.push(Span::raw(" "));
        }

        let max_lines = if selected && self.cursor_wraps {
            WRAP_FULLY
        } else {
            1
        };
        let wrapped = if entry.label.is_empty() {
            Vec::new()
        } else {
            let label_w = self
                .width
                .saturating_sub(prefix_w + CONTINUATION_INDENT)
                .max(1);
            syntax::wrap_spans(&label_spans(entry, self.theme), label_w, max_lines)
        };

        if wrapped.is_empty() {
            let mut spans = vec![cursor_span(selected, self.theme)];
            spans.extend(prefix);
            self.push_line(spans, index, selected);
            return;
        }

        for (line_index, mut chunk) in wrapped.into_iter().enumerate() {
            let mut spans = vec![cursor_span(selected, self.theme)];
            if line_index == 0 {
                spans.extend(prefix.clone());
            } else {
                // A wrap can land on a space; drop it so continuation rows
                // start on the text instead of looking ragged against the
                // indent.
                if let Some(first) = chunk.first_mut()
                    && let Some(rest) = first.content.strip_prefix(' ')
                {
                    first.content = rest.to_string().into();
                }
                spans.push(Span::raw(" ".repeat(CONTINUATION_INDENT)));
            }
            spans.extend(chunk);
            self.push_line(spans, index, selected);
        }
    }

    fn push_line(&mut self, spans: Vec<Span<'static>>, index: usize, selected: bool) {
        let spans = paint_selection(spans, selected, self.theme);
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
    if first < offset {
        state.activity.scroll.offset = first.saturating_sub(1);
    } else if last >= offset + visible {
        state.activity.scroll.offset = (last + 1).saturating_sub(visible);
    }
}

/// The cursor column for one row.
fn cursor_span(selected: bool, theme: &ColorTheme) -> Span<'static> {
    if !selected {
        return Span::raw(" ");
    }
    Span::styled(
        CURSOR_MARKER.to_string(),
        Style::default().fg(theme.accent).bg(theme.selection_bg),
    )
}

/// Paint the cursor row's background across every span of the entry, so a
/// wrapped command reads as one selected block.
fn paint_selection(
    spans: Vec<Span<'static>>,
    selected: bool,
    theme: &ColorTheme,
) -> Vec<Span<'static>> {
    if !selected {
        return spans;
    }
    let bg = Style::default().bg(theme.selection_bg);
    spans.into_iter().map(|span| span.patch_style(bg)).collect()
}

/// Whether the activity block is the one the keyboard is driving. Stacked in
/// the band, that is the block the accent border marks; in the bottom panel it
/// is the active tab while the panel holds the focus.
fn expanded(state: &AppState) -> bool {
    state.focus_state.focus == Focus::ActivityLog && state.bottom_tab == BottomTab::Activity
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
