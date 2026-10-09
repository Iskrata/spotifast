//! The interface's buttons: one icon button in three sizes, three tiers of
//! text button, and the green Play disc. Every control draws through here,
//! so its sizes, states and focus ring agree everywhere.

use egui::{Color32, CornerRadius, Response, Sense, Stroke, Ui, Vec2};

use super::tokens;
use crate::theme::{self, Icon, Palette};

/// How large an icon button is: its icon and the square it answers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSize {
    /// A 16-point icon in 28: rows, menus, the dense corners of panels.
    Compact,
    /// A 20-point icon in 32: toolbars and panel headers.
    Standard,
    /// A 24-point icon in 40: the action row under a page's header.
    Large,
}

impl IconSize {
    pub const fn icon(self) -> f32 {
        match self {
            Self::Compact => tokens::icon::SMALL,
            Self::Standard => tokens::icon::MEDIUM,
            Self::Large => tokens::icon::LARGE,
        }
    }

    pub const fn hit(self) -> f32 {
        match self {
            Self::Compact => tokens::hit::COMPACT,
            Self::Standard => tokens::hit::STANDARD,
            Self::Large => tokens::hit::LARGE,
        }
    }
}

/// A frameless icon control. Hovering lays a circle under the icon and
/// lifts its colour; a control that is on shows a green icon with a dot
/// under it. Its label is required: it is the tooltip and the name a
/// screen reader reads.
#[must_use = "an icon button does nothing until shown"]
pub struct IconButton<'a> {
    icon: Icon,
    label: &'a str,
    size: IconSize,
    active: bool,
    dimmed: bool,
    tint: Option<Color32>,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            icon,
            label,
            size: IconSize::Standard,
            active: false,
            dimmed: false,
            tint: None,
        }
    }

    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    /// A toggle that is on, or a panel that is open: green, with a dot.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Faded at rest while what it controls cannot answer, such as the
    /// transport with nothing to play. It still responds, and lifts on hover.
    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    /// An icon whose own colour carries meaning, such as a filled heart or
    /// a saved album's check. It keeps the colour on hover and shows no dot.
    pub fn tint(mut self, tint: impl Into<Option<Color32>>) -> Self {
        self.tint = tint.into();
        self
    }

    pub fn show(self, ui: &mut Ui, palette: &Palette) -> Response {
        debug_assert!(!self.label.is_empty(), "an icon button needs a label");
        let edge = self.size.hit();
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(edge), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), self.label)
        });
        if ui.is_rect_visible(rect) {
            let enabled = ui.is_enabled();
            let lifted = enabled && (response.hovered() || response.has_focus());
            let pressed = enabled && response.is_pointer_button_down_on();
            if pressed || lifted {
                let fill = if pressed {
                    palette.surface_active
                } else {
                    palette.surface_hover
                };
                ui.painter().circle_filled(rect.center(), edge / 2.0, fill);
            }
            let color = if !enabled {
                palette.dim
            } else if let Some(tint) = self.tint {
                tint
            } else if self.active {
                if lifted {
                    palette.accent_hover
                } else {
                    palette.accent
                }
            } else if lifted {
                palette.text
            } else if self.dimmed {
                palette.dim
            } else {
                palette.secondary
            };
            let size = self.size.icon();
            let icon_rect = egui::Rect::from_center_size(
                rect.center() + theme::play_glyph_offset(self.icon, size),
                Vec2::splat(size),
            );
            self.icon.image(color, size).paint_at(ui, icon_rect);
            if self.active {
                let gap = (edge - size) / 4.0;
                ui.painter().circle_filled(
                    egui::pos2(rect.center().x, rect.bottom() - gap),
                    ACTIVE_DOT / 2.0,
                    color,
                );
            }
        }
        focus_ring(ui, &response, CornerRadius::same(tokens::radius::ROUND));
        response.on_hover_text(self.label)
    }
}

/// The diameter of the dot under a control that is on.
const ACTIVE_DOT: f32 = 4.0;

/// Makes keyboard focus visible around a control, following its own shape
/// (`corner` of the control's rect), without changing its layout.
pub fn focus_ring(ui: &Ui, response: &Response, corner: CornerRadius) {
    if response.has_focus() {
        let rect = response.rect.expand(2.0);
        // Round shapes keep their roundness as the ring grows.
        let grow = |radius: u8| {
            if radius == 0 {
                0
            } else {
                radius.saturating_add(2)
            }
        };
        let corner = CornerRadius {
            nw: grow(corner.nw),
            ne: grow(corner.ne),
            sw: grow(corner.sw),
            se: grow(corner.se),
        };
        ui.painter().rect_stroke(
            rect,
            corner,
            Stroke::new(2.0, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Outside,
        );
    }
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
}
