//! The interface's buttons: one icon button in three sizes and three
//! tiers of text button. Every control draws through here,
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

/// The three kinds of text button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tier {
    /// The one green action a view leads with.
    Primary,
    /// Every other labelled action: a grey capsule.
    Secondary,
    /// One choice among several: grey, or inverted while chosen.
    Chip { selected: bool },
}

/// Metrics shared by every text button.
const TEXT_SIZE: f32 = 13.0;
const TEXT_ICON: f32 = tokens::icon::SMALL;
const TEXT_ICON_GAP: f32 = 6.0;

fn padding(tier: Tier) -> f32 {
    match tier {
        Tier::Primary => 16.0,
        Tier::Secondary | Tier::Chip { .. } => 14.0,
    }
}

fn font(tier: Tier) -> egui::FontId {
    match tier {
        Tier::Primary => theme::semibold(TEXT_SIZE),
        Tier::Secondary | Tier::Chip { .. } => theme::medium(TEXT_SIZE),
    }
}

/// The green capsule for the one action a view leads with: Create, Save,
/// Sign in, Apply.
pub fn primary(ui: &mut Ui, palette: &Palette, label: &str) -> Response {
    text_button(
        ui,
        palette,
        Tier::Primary,
        None,
        label,
        tokens::button::PRIMARY,
    )
    .0
}

/// A taller [`primary`] for the sign-in screen, where it is the only
/// control: as wide as the column, up to 300 points.
pub fn primary_large(ui: &mut Ui, palette: &Palette, label: &str) -> Response {
    let width = ui.available_width().min(300.0);
    text_button_inner(
        ui,
        palette,
        Tier::Primary,
        None,
        label,
        Vec2::new(width, tokens::button::PRIMARY_LARGE),
        false,
    )
    .0
}

/// A grey capsule for every other labelled action: Cancel, Retry, Follow,
/// Load more. `icon` leads the label.
pub fn secondary(ui: &mut Ui, palette: &Palette, icon: Option<Icon>, label: &str) -> Response {
    text_button(
        ui,
        palette,
        Tier::Secondary,
        icon,
        label,
        tokens::button::SECONDARY,
    )
    .0
}

/// One choice among several, such as a filter or a bitrate: inverted while
/// it is the chosen one.
pub fn chip(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    selected: bool,
) -> Response {
    let response = text_button(
        ui,
        palette,
        Tier::Chip { selected },
        icon,
        label,
        tokens::button::SECONDARY,
    )
    .0;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    response
}

/// A [`chip`] whose icon turns into a cross on hover, so the entry can be
/// dismissed. The flag reports clicks on that cross.
pub fn chip_dismissible(
    ui: &mut Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
) -> (Response, bool) {
    text_button_inner(
        ui,
        palette,
        Tier::Chip { selected: false },
        Some(icon),
        label,
        Vec2::new(0.0, tokens::button::SECONDARY),
        true,
    )
}

/// The width a [`secondary`] button or a [`chip`] without an icon takes,
/// for laying out a row of them before drawing it.
pub fn text_button_width(ui: &Ui, label: &str) -> f32 {
    let tier = Tier::Secondary;
    let galley = crate::bidi::layout_line(ui.painter(), label, font(tier), Color32::WHITE);
    galley.size().x + padding(tier) * 2.0
}

fn text_button(
    ui: &mut Ui,
    palette: &Palette,
    tier: Tier,
    icon: Option<Icon>,
    label: &str,
    height: f32,
) -> (Response, bool) {
    text_button_inner(
        ui,
        palette,
        tier,
        icon,
        label,
        Vec2::new(0.0, height),
        false,
    )
}

fn text_button_inner(
    ui: &mut Ui,
    palette: &Palette,
    tier: Tier,
    icon: Option<Icon>,
    label: &str,
    least: Vec2,
    dismissible: bool,
) -> (Response, bool) {
    let height = least.y;
    let color = match tier {
        Tier::Primary => palette.on_accent,
        Tier::Chip { selected: true } => palette.window,
        Tier::Secondary | Tier::Chip { selected: false } => palette.text,
    };
    let galley = crate::bidi::layout_line(ui.painter(), label, font(tier), color);
    let pad = padding(tier);
    let icon_width = if icon.is_some() {
        TEXT_ICON + TEXT_ICON_GAP
    } else {
        0.0
    };
    let size = Vec2::new(
        (galley.size().x + icon_width + pad * 2.0).max(least.x),
        height,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let content = galley.size().x + icon_width;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(
            rect.center().x - content / 2.0 + TEXT_ICON / 2.0,
            rect.center().y,
        ),
        Vec2::splat(TEXT_ICON),
    );
    // Claimed after the button, so the cross sits on top and keeps its own click.
    let dismiss = (dismissible && icon.is_some()).then(|| {
        let dismiss = ui.interact(
            icon_rect.expand(4.0),
            response.id.with("dismiss"),
            Sense::click(),
        );
        dismiss.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                format!("Remove {label}"),
            )
        });
        dismiss
    });
    let dismissed = dismiss.as_ref().is_some_and(Response::clicked);
    let over_dismiss = dismiss
        .as_ref()
        .is_some_and(|dismiss| dismiss.hovered() || dismiss.has_focus());
    let corner = tokens::capsule(height);
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hovered = enabled && (response.hovered() || over_dismiss);
        let pressed = enabled && response.is_pointer_button_down_on();
        let fill = match tier {
            Tier::Primary if pressed => palette.accent.gamma_multiply(0.92),
            Tier::Primary if hovered => palette.accent_hover,
            Tier::Primary => palette.accent,
            Tier::Chip { selected: true } => palette.text,
            _ if pressed => palette.surface_active,
            _ if hovered => palette.surface_hover,
            _ => palette.surface,
        };
        let fill = if enabled {
            fill
        } else {
            fill.gamma_multiply(0.4)
        };
        let color = if enabled {
            color
        } else {
            color.gamma_multiply(0.4)
        };
        ui.painter().rect_filled(rect, corner, fill);
        // Content is centred when a least width leaves room around it.
        let mut x = rect.center().x - content / 2.0;
        if let Some(icon) = icon {
            let icon = if dismiss.is_some() && hovered {
                Icon::X
            } else {
                icon
            };
            icon.image(color, TEXT_ICON).paint_at(ui, icon_rect);
            x += icon_width;
        }
        let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, color);
    }
    focus_ring(ui, &response, corner);
    if let Some(dismiss) = dismiss {
        focus_ring(ui, &dismiss, CornerRadius::same(tokens::radius::ROUND));
    }
    (response, dismissed)
}
