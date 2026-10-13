//! The Copy Settings dialog: choose which groups of settings to copy.
//!
//! Lifted from Lightcraft's Copy Settings dialog (`crates/ui-egui/src/panels/dialogs.rs`
//! at commit `701f78e`), restyled with OmaLux's theme.

use egui::{CornerRadius, Frame, Margin, RichText, Stroke};
use omalux_engine::copy::Group;

use crate::theme::Theme;

/// What happened in the Copy Settings dialog this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    KeepOpen,
    Copied(Vec<Group>),
    Cancelled,
}

/// The Copy Settings modal dialog.
pub struct Checklist {
    chosen: Vec<Group>,
}

impl Checklist {
    pub fn new(groups: &[Group]) -> Self {
        let chosen = Group::ALL.iter().copied().filter(|g| groups.contains(g)).collect();
        Self { chosen }
    }

    #[cfg(test)]
    pub fn chosen(&self) -> &[Group] {
        &self.chosen
    }

    /// Draw the dialog. Returns what happened this frame.
    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme) -> Outcome {
        let enter_pressed = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let (mut copy_clicked, mut cancel_clicked) = (false, false);
        let frame = Frame::new()
            .fill(theme.background)
            .stroke(Stroke::new(1.0, theme.muted))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::symmetric(16, 12));
        // Esc, and a click on the dim behind it, close an egui modal.
        let modal = egui::Modal::new(egui::Id::new("copy settings")).frame(frame).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new("Copy Settings").strong().color(theme.foreground));
            ui.columns(2, |cols| {
                for (i, group) in Group::ALL.iter().enumerate() {
                    let mut on = self.chosen.contains(group);
                    if cols[i % 2].checkbox(&mut on, group.label()).changed() {
                        self.chosen.retain(|g| g != group);
                        if on {
                            self.chosen.push(*group);
                        }
                    }
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Check All").clicked() {
                    self.chosen = Group::ALL.to_vec();
                }
                if ui.button("Check None").clicked() {
                    self.chosen.clear();
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                cancel_clicked = ui.button("Cancel").clicked();
                copy_clicked = ui.button("Copy").clicked();
            });
        });
        if cancel_clicked || modal.should_close() {
            return Outcome::Cancelled;
        }
        if enter_pressed || copy_clicked {
            // In the panels' order, however they were checked.
            let chosen: Vec<Group> = Group::ALL.iter().copied().filter(|g| self.chosen.contains(g)).collect();
            return if chosen.is_empty() { Outcome::Cancelled } else { Outcome::Copied(chosen) };
        }
        Outcome::KeepOpen
    }
}
