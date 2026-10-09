//! Side panels and the sidebar slide open and closed over
//! [`tokens::motion::PANEL`], toward the window edge they hang from.
//!
//! The right panels (Queue, Lyrics, Friend Activity) take one another's
//! place: opening one while another shows swaps them at once, and only
//! opening into an empty place, or closing, slides.

use egui::{Id, InnerResponse};

use super::{motion, tokens};
use crate::app::App;

/// How a sliding panel moves this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slide {
    /// Whether it is open, or closing.
    pub open: bool,
    /// Open at once instead of sliding, as when it takes another panel's
    /// place.
    pub instant: bool,
}

impl Slide {
    /// Open, and no longer moving.
    pub const OPEN: Self = Self {
        open: true,
        instant: false,
    };
}

/// What a sliding panel drew.
pub(crate) struct Slid<R> {
    pub inner: InnerResponse<R>,
    /// Whether it has finished sliding, so its width is the one the person
    /// chose rather than a step of the slide.
    pub settled: bool,
}

/// Shows `panel` while any of it is in view, sliding it over the panel
/// duration. Its id must be `id`. Dragging its edge resizes it as before;
/// it never drags closed or open.
pub(crate) fn show<R>(
    ui: &mut egui::Ui,
    panel: egui::Panel,
    id: &str,
    slide: Slide,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<Slid<R>> {
    let seconds = if slide.instant {
        0.0
    } else {
        motion::time(ui.ctx(), tokens::motion::PANEL)
    };
    // egui's slide reads this animation again in the same frame, which
    // moves it no further, so it follows the motion duration; both ease
    // out along the same cubic.
    let shown = ui.ctx().animate_bool_with_time_and_easing(
        animation_id(id),
        slide.open,
        seconds,
        tokens::motion::ease,
    );
    // A drag past the panel's least width or a double-click on its edge
    // would close it here; the toggles stay the only way to.
    let mut expanded = slide.open;
    let inner = panel
        .drag_to_open(false)
        .show_collapsible(ui, &mut expanded, add_contents)?;
    Some(Slid {
        inner,
        settled: slide.open && shown >= 1.0,
    })
}

/// The animation egui's collapsible panel slides by.
fn animation_id(id: &str) -> Id {
    Id::new(id).with("animation")
}

/// The right panels, in the order they are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RightPanel {
    Queue,
    Lyrics,
    Friends,
}

impl RightPanel {
    const ALL: [Self; 3] = [Self::Queue, Self::Lyrics, Self::Friends];

    fn is_open(self, app: &App) -> bool {
        match self {
            Self::Queue => app.show_queue_panel,
            Self::Lyrics => app.show_lyrics_panel,
            Self::Friends => app.show_friends_panel,
        }
    }

    /// Draws the panel, open or sliding closed.
    fn show(self, app: &mut App, ui: &mut egui::Ui, slide: Slide) {
        match self {
            Self::Queue => super::queue::side_panel(app, ui, slide),
            Self::Lyrics => super::lyrics::side_panel(app, ui, slide),
            Self::Friends => super::friends::side_panel(app, ui, slide),
        }
    }
}

/// Draws the right panels that are open or closing. One taking another's
/// place in the same frame, as the toggles do, swaps with it at once.
pub(crate) fn right_panels(app: &mut App, ui: &mut egui::Ui) {
    let memory = Id::new("right-panels-open");
    let open = RightPanel::ALL.map(|panel| panel.is_open(app));
    let before = ui
        .ctx()
        .data(|data| data.get_temp::<[bool; 3]>(memory))
        .unwrap_or(open);
    if before != open {
        ui.ctx().data_mut(|data| data.insert_temp(memory, open));
    }
    let opened = (0..3).any(|index| open[index] && !before[index]);
    let closed = (0..3).any(|index| !open[index] && before[index]);
    let swap = opened && closed;
    for (index, panel) in RightPanel::ALL.into_iter().enumerate() {
        let changed = open[index] != before[index];
        panel.show(
            app,
            ui,
            Slide {
                open: open[index],
                instant: swap && changed,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame `dt` after the last with a right panel open or closing;
    /// returns its visible width, if any shows, and whether it settled.
    fn frame(ctx: &egui::Context, time: &mut f64, dt: f32, open: bool) -> Option<(f32, bool)> {
        *time += f64::from(dt);
        let input = egui::RawInput {
            time: Some(*time),
            predicted_dt: dt,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let mut result = None;
        let mut output = ctx.run_ui(input, |ui| {
            let panel = egui::Panel::right("test-panel")
                .resizable(true)
                .exact_size(300.0);
            result = show(
                ui,
                panel,
                "test-panel",
                Slide {
                    open,
                    instant: false,
                },
                |_| (),
            )
            .map(|slid| {
                let visible = slid.inner.response.rect.intersect(ui.max_rect());
                (visible.width(), slid.settled)
            });
        });
        output.textures_delta.clear();
        result
    }

    #[test]
    fn a_panel_slides_open_over_the_panel_duration() {
        let ctx = egui::Context::default();
        let mut now = 0.0;
        assert!(frame(&ctx, &mut now, 0.016, false).is_none());
        // Halfway through, it is partly in view and not yet settled.
        let (width, settled) =
            frame(&ctx, &mut now, tokens::motion::PANEL / 2.0, true).expect("sliding in");
        assert!(width > 0.0 && width < 299.0, "{width}");
        assert!(!settled);
        let (_, settled) = frame(&ctx, &mut now, tokens::motion::PANEL / 2.0, true).unwrap();
        assert!(settled);
        // Closing slides out, and then nothing is drawn.
        let (width, settled) = frame(&ctx, &mut now, tokens::motion::PANEL / 2.0, false).unwrap();
        assert!(width < 299.0 && !settled);
        let _ = frame(&ctx, &mut now, tokens::motion::PANEL / 2.0, false);
        assert!(frame(&ctx, &mut now, 0.016, false).is_none());
    }

    #[test]
    fn reduce_motion_opens_a_panel_at_once() {
        let ctx = egui::Context::default();
        let mut now = 0.0;
        motion::set_reduced(&ctx, true);
        assert!(frame(&ctx, &mut now, 0.016, false).is_none());
        let (_, settled) = frame(&ctx, &mut now, 0.016, true).expect("open");
        assert!(settled);
        assert!(frame(&ctx, &mut now, 0.016, false).is_none());
    }
}
