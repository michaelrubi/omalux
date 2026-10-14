//! The keyboard shortcuts legend (Lightroom's Develop Shortcuts, Ctrl+/):
//! every command and the key it's on now, and the keys and clicks that
//! aren't commands.

use crate::theme::Theme;

/// Draw the legend over everything. Returns whether it stays open.
pub fn show(ctx: &egui::Context, theme: &Theme) -> bool {
    let _ = (ctx, theme);
    false
}
