//! The header every side panel opens with: Lyrics, Queue and Friend
//! Activity (titled Now playing while a song plays). One title size, one
//! row of standard icon buttons with Close always last, and one gap
//! before the panel's contents.

use egui::Ui;

use super::buttons::IconButton;
use super::tokens;
use crate::theme::{self, Icon, Palette};

/// A side panel's header.
#[must_use = "a panel header does nothing until shown"]
pub struct PanelHeader<'a> {
    title: &'a str,
    close: &'a str,
}

impl<'a> PanelHeader<'a> {
    /// `close` is the Close button's label.
    pub fn new(title: &'a str, close: &'a str) -> Self {
        Self { title, close }
    }

    /// Draws the header. `actions` adds the panel's own controls, right to
    /// left, before Close. Returns whether Close was clicked.
    pub fn show(self, ui: &mut Ui, palette: &Palette, actions: impl FnOnce(&mut Ui)) -> bool {
        let closed = self.title_row(ui, palette, actions);
        ui.add_space(tokens::panel::BELOW);
        closed
    }

    /// [`Self::show`] with a line of tabs under the title, such as the
    /// Queue's Queue and Recent.
    pub fn show_with_tabs(
        self,
        ui: &mut Ui,
        palette: &Palette,
        actions: impl FnOnce(&mut Ui),
        tabs: impl FnOnce(&mut Ui),
    ) -> bool {
        let closed = self.title_row(ui, palette, actions);
        ui.add_space(tokens::panel::TABS_GAP);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(tokens::panel::INSET);
            tabs(ui);
        });
        ui.add_space(tokens::panel::BELOW);
        closed
    }

    fn title_row(self, ui: &mut Ui, palette: &Palette, actions: impl FnOnce(&mut Ui)) -> bool {
        let mut closed = false;
        // The controls are measured first, so a long title shortens
        // instead of running under them.
        egui::Sides::new()
            .shrink_left()
            .height(tokens::hit::STANDARD)
            .show(
                ui,
                |ui| {
                    ui.add_space(tokens::panel::INSET);
                    theme::text(
                        ui,
                        self.title,
                        theme::semibold(tokens::panel::TITLE),
                        palette.text,
                    );
                },
                |ui| {
                    ui.spacing_mut().item_spacing.x = tokens::gap::ICONS;
                    closed = IconButton::new(Icon::X, self.close)
                        .show(ui, palette)
                        .clicked();
                    actions(ui);
                },
            );
        closed
    }
}
