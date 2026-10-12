/// Whether the trailing `×` remove marker should even be considered
/// for this pane. Gated on sidebar-spawn + a visible worktree `+`
/// prefix so plain branches never get a spurious action affordance.
fn should_emit_remove_marker(git_info: &crate::group::PaneGitInfo, sidebar_spawned: bool) -> bool {
    sidebar_spawned && crate::ui::text::branch_label(git_info).starts_with("+ ")
}

/// Compute the column offset (within the full pane row) where the
/// trailing remove-`×` marker lands for a sidebar-spawned worktree.
/// The marker is pinned to the rightmost column of the row so it
/// mirrors the repo header's right-aligned `+` spawn button —
/// "action buttons always live at the right edge". Returns `None`
/// when the pane is not eligible.
///
/// The glyph itself is drawn by the status row; this stays the single
/// source of truth for the click-target column in `row_collector`.
pub fn sidebar_remove_marker_col(
    git_info: &crate::group::PaneGitInfo,
    sidebar_spawned: bool,
    inner_width: usize,
) -> Option<u16> {
    if !should_emit_remove_marker(git_info, sidebar_spawned) {
        return None;
    }
    // Row total width = marker(1) + space(1) + inner_width, so the
    // last column (0-indexed) is `inner_width + 1`.
    Some((inner_width + 1) as u16)
}
