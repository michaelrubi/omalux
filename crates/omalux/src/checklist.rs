//! The Copy Settings dialog: choose which groups of settings to copy.
//!
//! Lifted from Lightcraft's Copy Settings dialog (`crates/ui-egui/src/panels/dialogs.rs`
//! at commit `701f78e`), restyled with OmaLux's theme.

use egui::{self, Align2, Color32, CornerRadius, Frame, Margin, Order, Stroke};
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
        let esc_pressed = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));

        let screen = ctx.content_rect();
        let backdrop = egui::Area::new(egui::Id::new("checklist-dim"))
            .order(Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (_, response) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(140));
                response
            });

        let mut copy_clicked = false;
        let mut cancel_clicked = false;

        let frame = Frame::new()
            .fill(theme.background)
            .stroke(Stroke::new(1.0, theme.muted))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::symmetric(16, 12));

        let window = egui::Window::new("Copy Settings")
            .id(egui::Id::new("copy_settings_dialog"))
            .collapsible(false)
            .resizable(false)
            .frame(frame)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.columns(2, |cols| {
                    for (i, group) in Group::ALL.iter().enumerate() {
                        let mut on = self.chosen.contains(group);
                        let response = cols[i % 2].checkbox(&mut on, group.label());
                        #[cfg(test)]
                        cols[i % 2].data_mut(|d| {
                            d.insert_temp(egui::Id::new(group.label()), response.rect);
                        });
                        if response.changed() {
                            if on {
                                if !self.chosen.contains(group) {
                                    self.chosen.push(*group);
                                }
                            } else {
                                self.chosen.retain(|g| g != group);
                            }
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let all = ui.button("Check All");
                    #[cfg(test)]
                    ui.data_mut(|d| d.insert_temp(egui::Id::new("Check All"), all.rect));
                    if all.clicked() {
                        self.chosen = Group::ALL.to_vec();
                    }
                    let none = ui.button("Check None");
                    #[cfg(test)]
                    ui.data_mut(|d| d.insert_temp(egui::Id::new("Check None"), none.rect));
                    if none.clicked() {
                        self.chosen.clear();
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let cancel = ui.button("Cancel");
                    #[cfg(test)]
                    ui.data_mut(|d| d.insert_temp(egui::Id::new("Cancel"), cancel.rect));
                    if cancel.clicked() {
                        cancel_clicked = true;
                    }
                    let copy = ui.button("Copy");
                    #[cfg(test)]
                    ui.data_mut(|d| d.insert_temp(egui::Id::new("Copy"), copy.rect));
                    if copy.clicked() {
                        copy_clicked = true;
                    }
                });
            });

        let clicked_outside = if let Some(shown) = &window {
            let window_rect = shown.response.rect;
            #[cfg(test)]
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("Copy Settings"), window_rect));
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos().or(i.pointer.latest_pos())) {
                let was_click = ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Primary))
                    || backdrop.inner.clicked();
                was_click && !window_rect.contains(pos)
            } else {
                false
            }
        } else {
            false
        };

        if esc_pressed || cancel_clicked || clicked_outside {
            return Outcome::Cancelled;
        }

        if enter_pressed || copy_clicked {
            if self.chosen.is_empty() {
                return Outcome::Cancelled;
            }
            let ordered: Vec<Group> = Group::ALL.iter().copied().filter(|g| self.chosen.contains(g)).collect();
            return Outcome::Copied(ordered);
        }

        Outcome::KeepOpen
    }
}
