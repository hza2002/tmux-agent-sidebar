//! Shell syntax highlighting for activity labels.
//!
//! A hook label is a raw string the agent captured: for shell tools it is a
//! command line, for file tools a basename, for subagents a paragraph. A
//! command line is much easier to scan when the command word, its options, its
//! quoted strings, and its operators are not all the same shade of gray.
//!
//! The classification comes from the real bash grammar ([`tree_sitter_bash`])
//! and the highlight query shipped with it, not from a hand-rolled guess at
//! what a command looks like. This module only does two things: turn the
//! grammar's highlight events into styled spans, and map the grammar's
//! highlight names onto the sidebar's existing theme slots.

use std::sync::OnceLock;

use ratatui::{
    style::{Modifier, Style},
    text::Span,
};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};
use unicode_width::UnicodeWidthChar;

use crate::ui::colors::ColorTheme;

/// The grammar's highlight names this fork recognizes, in the order
/// [`HighlightConfiguration::configure`] expects: a highlight index is an index
/// into this list. `configure` reports nothing for names it is not given, so a
/// name the theme does not know stays on the body color instead of picking up a
/// wrong color.
const RECOGNIZED: &[&str] = &[
    "function", "string", "constant", "operator", "number", "keyword", "property", "embedded",
    "comment",
];

/// The bash grammar's highlight configuration, compiled once per process. The
/// query compile costs a few milliseconds, so it is paid on the first activity
/// line that needs it rather than at startup.
fn configuration() -> Option<&'static HighlightConfiguration> {
    static CONFIG: OnceLock<Option<HighlightConfiguration>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut config = HighlightConfiguration::new(
                tree_sitter_bash::LANGUAGE.into(),
                "bash",
                tree_sitter_bash::HIGHLIGHT_QUERY,
                "",
                "",
            )
            .ok()?;
            config.configure(RECOGNIZED);
            Some(config)
        })
        .as_ref()
}

/// Style for one of [`RECOGNIZED`]'s names. Everything the grammar calls
/// something else — including a path, which the shell itself has no opinion
/// about — keeps the body color.
fn style_for(name: &str, theme: &ColorTheme) -> Style {
    match name {
        // The command word (`rg`, `cargo`, `git`): the one token worth
        // emphasizing in a log of one-line commands.
        "function" => Style::default()
            .fg(theme.text_active)
            .add_modifier(Modifier::BOLD),
        // Options: the grammar captures `-n`, `--color=always`, `-20`.
        "constant" => Style::default().fg(theme.activity_interaction),
        // Quoted runs, quotes included.
        "string" => Style::default().fg(theme.activity_edit),
        "operator" | "keyword" | "comment" => Style::default().fg(theme.text_inactive),
        "number" | "property" | "embedded" => Style::default().fg(theme.activity_read),
        _ => body_style(theme),
    }
}

fn body_style(theme: &ColorTheme) -> Style {
    Style::default().fg(theme.text_muted)
}

/// Parse `line` with the bash grammar and paint what the grammar recognizes.
/// Text the grammar leaves unlabelled — a bare word, a glob, a path — keeps the
/// body color, and so does a line the parser or the encoding defeats.
pub(super) fn command_spans(line: &str, theme: &ColorTheme) -> Vec<Span<'static>> {
    let Some(config) = configuration() else {
        return plain_spans(line, theme);
    };
    let mut highlighter = Highlighter::new();
    let Ok(events) = highlighter.highlight(config, line.as_bytes(), None, |_| None) else {
        return plain_spans(line, theme);
    };

    let mut spans: Vec<Span<'static>> = Vec::new();
    // Highlights nest; the innermost open highlight wins, which is what the
    // grammar means by a query that captures a node inside another node.
    let mut open: Vec<Style> = Vec::new();
    let mut cursor = 0usize;

    for event in events {
        let Ok(event) = event else {
            return plain_spans(line, theme);
        };
        match event {
            HighlightEvent::HighlightStart(highlight) => {
                let name = RECOGNIZED.get(highlight.0).copied().unwrap_or_default();
                open.push(style_for(name, theme));
            }
            HighlightEvent::HighlightEnd => {
                open.pop();
            }
            HighlightEvent::Source { start, end } => {
                // The grammar works on bytes; a range that is not a character
                // boundary would panic on a slice, so anything unexpected
                // falls back to the plain label rather than guessing.
                let (Some(gap), Some(text)) = (line.get(cursor..start), line.get(start..end))
                else {
                    return plain_spans(line, theme);
                };
                if !gap.is_empty() {
                    spans.push(Span::styled(gap.to_string(), body_style(theme)));
                }
                let style = open.last().copied().unwrap_or_else(|| body_style(theme));
                spans.push(Span::styled(text.to_string(), style));
                cursor = end;
            }
        }
    }
    if let Some(tail) = line.get(cursor..)
        && !tail.is_empty()
    {
        spans.push(Span::styled(tail.to_string(), body_style(theme)));
    }
    if spans.is_empty() {
        return plain_spans(line, theme);
    }
    spans
}

/// A label that is not a command line — a basename, a glob, a paragraph — stays
/// on the body color it has always used.
pub(super) fn plain_spans(line: &str, theme: &ColorTheme) -> Vec<Span<'static>> {
    vec![Span::styled(line.to_string(), body_style(theme))]
}

/// Wrap styled text by display width, `max_lines` at most, truncating the last
/// line with an ellipsis when the text does not fit. Mirrors
/// [`crate::ui::text::wrap_text_char`] (character, not word, boundaries) but
/// keeps every character's style, which is what lets a highlighted command wrap
/// across lines without losing its colors.
///
/// `first_width` is the room the first line has — an entry spends part of it on
/// `HH:MM` and, for a file tool, on the tool name — and `rest_width` is the room
/// every following line has, which is the full row because those lines start
/// flush at the gutter.
pub(super) fn wrap_spans(
    spans: &[Span<'static>],
    first_width: usize,
    rest_width: usize,
    max_lines: usize,
) -> Vec<Vec<Span<'static>>> {
    if first_width == 0 || rest_width == 0 || max_lines == 0 {
        return Vec::new();
    }
    let chars: Vec<(char, Style)> = spans
        .iter()
        .flat_map(|span| {
            let style = span.style;
            span.content.chars().map(move |ch| (ch, style))
        })
        .collect();

    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut pos = 0;
    while pos < chars.len() && lines.len() < max_lines {
        let last_line = lines.len() + 1 == max_lines;
        let max_width = if lines.is_empty() {
            first_width
        } else {
            rest_width
        };
        // Collect what fits at full width first: when everything that is left
        // still fits there is no need to spend a column on the ellipsis.
        let mut chunk: Vec<(char, Style)> = Vec::new();
        let mut width = 0;
        let mut end = pos;
        while end < chars.len() {
            let ch_w = UnicodeWidthChar::width(chars[end].0).unwrap_or(0);
            if width + ch_w > max_width {
                break;
            }
            chunk.push(chars[end]);
            width += ch_w;
            end += 1;
        }

        // A line that collected nothing (a glyph wider than the whole line)
        // falls through to the truncating branch below instead of emitting a
        // blank row.
        if end >= chars.len() || (!last_line && end > pos) {
            pos = end;
            lines.push(merge_styles(&chunk));
            continue;
        }

        // Last allowed line with text left over: leave a column for `…` and
        // stop, so the caller never sees text that silently disappeared.
        let mut truncated: Vec<(char, Style)> = Vec::new();
        let mut width = 0;
        for &(ch, style) in &chars[pos..] {
            let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + ch_w + 1 > max_width {
                break;
            }
            truncated.push((ch, style));
            width += ch_w;
        }
        truncated.push(('…', style_at(&chars, pos + truncated.len())));
        lines.push(merge_styles(&truncated));
        pos = chars.len();
    }

    lines
}

/// Style of the character that a line was cut before, so the ellipsis belongs
/// to the token it replaced.
fn style_at(chars: &[(char, Style)], pos: usize) -> Style {
    chars.get(pos).map(|&(_, style)| style).unwrap_or_default()
}

fn merge_styles(chars: &[(char, Style)]) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let mut current_style: Option<Style> = None;

    for &(ch, style) in chars {
        if current_style != Some(style) {
            if let Some(flush_style) = current_style {
                spans.push(Span::styled(std::mem::take(&mut current), flush_style));
            }
            current_style = Some(style);
        }
        current.push(ch);
    }
    if let Some(style) = current_style {
        spans.push(Span::styled(current, style));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::text::display_width;

    fn text(spans: &[Span<'static>]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    fn spans_width(spans: &[Span<'static>]) -> usize {
        spans.iter().map(|span| display_width(&span.content)).sum()
    }

    fn styled(spans: &[Span<'static>], content: &str) -> Option<Style> {
        spans
            .iter()
            .find(|span| span.content == content)
            .map(|span| span.style)
    }

    fn fg(spans: &[Span<'static>], content: &str) -> Option<ratatui::style::Color> {
        styled(spans, content).and_then(|style| style.fg)
    }

    /// Wrap with the same width on every row, which is the simple case these
    /// tests care about.
    fn wrap(spans: &[Span<'static>], width: usize, max_lines: usize) -> Vec<Vec<Span<'static>>> {
        wrap_spans(spans, width, width, max_lines)
    }

    #[test]
    fn command_spans_preserve_every_character() {
        let theme = ColorTheme::default();
        for line in [
            r#"rg -n "setup guide" README.md | head -20"#,
            "cargo test -- --nocapture",
            "git commit -m 'wip'",
            "",
            "   ",
            "ls",
            r#"echo "unclosed"#,
            "日本語のコマンド",
        ] {
            assert_eq!(
                text(&command_spans(line, &theme)),
                line,
                "highlighting must not rewrite {line:?}"
            );
        }
    }

    #[test]
    fn command_spans_follow_the_grammar() {
        let theme = ColorTheme::default();
        let spans = command_spans(r#"rg -n "setup guide" src/main.rs | head -20"#, &theme);

        // Command words come from the grammar's `function` captures.
        let command = styled(&spans, "rg").expect("command word span");
        assert_eq!(command.fg, Some(theme.text_active));
        assert!(command.add_modifier.contains(Modifier::BOLD));
        assert_eq!(fg(&spans, "head"), Some(theme.text_active));

        // Options, quoted runs, and operators each get their own slot.
        assert_eq!(fg(&spans, "-n"), Some(theme.activity_interaction));
        assert_eq!(fg(&spans, "-20"), Some(theme.activity_interaction));
        assert_eq!(fg(&spans, r#""setup guide""#), Some(theme.activity_edit));
        assert_eq!(fg(&spans, "|"), Some(theme.text_inactive));

        // A bare path is not a shell token: the grammar leaves it alone, so it
        // stays on the body color instead of being guessed at.
        let body = spans
            .iter()
            .find(|span| span.content.contains("src/main.rs"))
            .expect("path stays in an unhighlighted gap");
        assert_eq!(body.style.fg, Some(theme.text_muted));
    }

    #[test]
    fn command_spans_read_numbers_and_redirects() {
        let theme = ColorTheme::default();
        let spans = command_spans("2>/dev/null", &theme);
        // The grammar reads `2` as a number and `>` as an operator; the file is
        // a plain word.
        assert_eq!(fg(&spans, "2"), Some(theme.activity_read));
        assert_eq!(fg(&spans, ">"), Some(theme.text_inactive));
        assert_eq!(text(&spans), "2>/dev/null");
    }

    #[test]
    fn command_spans_survive_text_the_grammar_cannot_parse() {
        let theme = ColorTheme::default();
        // Unterminated quotes and stray operators leave ERROR nodes; the line
        // must still render as itself.
        for line in [r#"echo "unterminated"#, "| | |", "for do done"] {
            assert_eq!(text(&command_spans(line, &theme)), line);
        }
    }

    #[test]
    fn wrap_spans_keeps_the_full_text_when_it_fits() {
        let theme = ColorTheme::default();
        let spans = command_spans("rg foo", &theme);
        let lines = wrap(&spans, 20, 3);
        assert_eq!(lines.len(), 1);
        assert_eq!(spans_width(&lines[0]), 6);
    }

    #[test]
    fn wrap_spans_truncates_the_last_allowed_line() {
        let theme = ColorTheme::default();
        let spans = command_spans("rg --files-with-matches pattern", &theme);
        let lines = wrap(&spans, 10, 2);
        assert_eq!(lines.len(), 2);
        for line in &lines {
            assert!(spans_width(line) <= 10);
        }
        assert!(text(&lines[1]).ends_with('…'));
    }

    #[test]
    fn wrap_spans_keeps_token_colors_per_character() {
        let theme = ColorTheme::default();
        let spans = command_spans("rg --color=always", &theme);
        let lines = wrap(&spans, 6, 2);
        assert_eq!(lines.len(), 2);
        // The command word starts the first line; the second line continues
        // inside the option, so it must keep the option's color instead of
        // inheriting the color of whatever started the line above it.
        assert_eq!(lines[0][0].style.fg, Some(theme.text_active));
        assert_eq!(lines[1][0].style.fg, Some(theme.activity_interaction));
    }

    #[test]
    fn wrap_spans_never_emits_a_blank_line() {
        let theme = ColorTheme::default();
        // Each glyph is wider than the whole line, so nothing can be shown —
        // the row has to say so instead of rendering empty.
        let spans = command_spans("日本語", &theme);
        let lines = wrap(&spans, 1, 3);
        assert_eq!(lines.len(), 1);
        assert_eq!(text(&lines[0]), "…");
    }

    #[test]
    fn wrap_spans_gives_the_first_row_less_room() {
        // A narrow first row (the entry's `HH:MM` prefix) and full-width
        // continuation rows: the text after the first row must use the extra
        // room instead of leaving it empty.
        let theme = ColorTheme::default();
        let spans = command_spans("rg --files-with-matches pattern", &theme);
        let lines = wrap_spans(&spans, 6, 10, 3);
        assert_eq!(spans_width(&lines[0]), 6);
        assert_eq!(spans_width(&lines[1]), 10);
    }

    #[test]
    fn plain_spans_carry_the_body_color() {
        let theme = ColorTheme::default();
        let spans = plain_spans("main.rs", &theme);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(theme.text_muted));
    }
}
