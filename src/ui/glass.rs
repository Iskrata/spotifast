//! Liquid glass: while the moving album art background shows, the sidebar,
//! the side panels, the player bar and the top bar's search float over it
//! as translucent surfaces with gutters between them, so the art's colours
//! glow through and around them. Menus, popovers and dialogs are glass too,
//! nearly opaque, as they hold the most text.
//!
//! The glass is a tint, not a blur: egui draws no backdrop filter. Its
//! opacity keeps text readable over the brightest orbs (see the tests).
//! With the art background off, every surface is solid as before.

use egui::epaint::{PathShape, PathStroke, Shadow};
use egui::{Color32, Context, CornerRadius, Frame, Id, Margin, Rect, Shape, Stroke, StrokeKind};

use super::tokens;
use crate::theme::Palette;

fn on_id() -> Id {
    Id::new("liquid-glass")
}

/// Turns glass on or off for the frames that follow.
pub fn set(ctx: &Context, on: bool) {
    if self::on(ctx) != on {
        ctx.data_mut(|data| data.insert_temp(on_id(), on));
    }
}

/// Whether the surfaces are glass this frame.
pub fn on(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp(on_id()).unwrap_or(false))
}

/// The frame of a floating panel filled with `solid` when glass is off:
/// the sidebar, a side panel or the player bar. As glass, it is filled
/// with the panel glass, rounded and inset by the gutter; a tint the solid
/// fill carried (the player bar's) gives way to the art showing through.
/// Its shadow and edges come from [`Floating`].
pub fn panel_frame(ctx: &Context, palette: &Palette, solid: Color32) -> Frame {
    if !on(ctx) {
        return Frame::new().fill(solid);
    }
    Frame::new()
        .fill(palette.glass_panel())
        .corner_radius(CornerRadius::same(tokens::radius::FLOATING))
        .outer_margin(Margin::same(tokens::glass::GUTTER))
}

/// The panel fill as it looks over the plain window, made opaque, for
/// covering a panel's own contents where they must not show, as under the
/// sidebar's floating artwork. The plain panel colour while glass is off.
pub fn panel_solid(ctx: &Context, palette: &Palette) -> Color32 {
    if on(ctx) {
        over(palette.glass_panel(), palette.window)
    } else {
        palette.panel
    }
}

/// `top` laid over `bottom` as egui blends them, in the display's own
/// space.
pub(crate) fn over(top: Color32, bottom: Color32) -> Color32 {
    let behind = 1.0 - f32::from(top.a()) / 255.0;
    let channel = |top: u8, bottom: u8| {
        (f32::from(top) + f32::from(bottom) * behind)
            .round()
            .min(255.0) as u8
    };
    let alpha = (f32::from(top.a()) + f32::from(bottom.a()) * behind).round() as u8;
    Color32::from_rgba_premultiplied(
        channel(top.r(), bottom.r()),
        channel(top.g(), bottom.g()),
        channel(top.b(), bottom.b()),
        alpha,
    )
}

/// A popover's frame as glass: `solid`, the frame it has when glass is
/// off, with the popover glass as its fill, a faint edge and rounder
/// corners.
pub fn popover_frame(ctx: &Context, palette: &Palette, solid: Frame, radius: u8) -> Frame {
    if !on(ctx) {
        return solid;
    }
    solid
        .fill(palette.glass_popover())
        .stroke(Stroke::new(1.0, palette.glass_border()))
        .corner_radius(CornerRadius::same(radius))
}

/// A floating glass surface's shadow and edges, around a panel drawn with
/// [`panel_frame`]. Begun before the panel draws, so the shadow lies under
/// it, and finished after, so the edges lie over its contents' margins.
/// Nothing at all while glass is off.
pub struct Floating {
    shadow: Option<egui::layers::ShapeIdx>,
}

impl Floating {
    pub fn begin(ui: &egui::Ui) -> Self {
        Self {
            shadow: on(ui.ctx()).then(|| ui.painter().add(Shape::Noop)),
        }
    }

    /// Paints around the panel whose outer rect, gutter included, is
    /// `outer`.
    pub fn finish(self, ui: &egui::Ui, palette: &Palette, outer: Rect) {
        let Some(slot) = self.shadow else {
            return;
        };
        let rect = outer.shrink(f32::from(tokens::glass::GUTTER));
        if !rect.is_positive() {
            return;
        }
        let corner = CornerRadius::same(tokens::radius::FLOATING);
        ui.painter()
            .set(slot, shadow(palette).as_shape(rect, corner));
        ui.painter().add(edge(palette, rect, corner));
    }
}

/// The soft shadow a floating surface casts on the art behind it.
fn shadow(palette: &Palette) -> Shadow {
    let [.., alpha] = palette.shadow.to_srgba_unmultiplied();
    Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(alpha / 2),
    }
}

/// A glass surface's outline: the highlight along its top edge, fading
/// over [`tokens::glass::HIGHLIGHT_DEPTH`] into the faint border around
/// the rest.
pub fn edge(palette: &Palette, rect: Rect, corner: CornerRadius) -> Shape {
    let mut points = Vec::new();
    egui::epaint::tessellator::path::rounded_rectangle(&mut points, rect, corner.into());
    let (light, border) = (palette.glass_highlight(), palette.glass_border());
    let top = rect.top();
    let stroke = PathStroke::new_uv(1.0, move |_, point| {
        let depth = ((point.y - top) / tokens::glass::HIGHLIGHT_DEPTH).clamp(0.0, 1.0);
        light.lerp_to_gamma(border, depth)
    })
    .with_kind(StrokeKind::Inside);
    Shape::Path(PathShape::closed_line(points, stroke))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glass_follows_what_it_is_set_to() {
        let ctx = Context::default();
        assert!(!on(&ctx), "solid until turned on");
        set(&ctx, true);
        assert!(on(&ctx));
        set(&ctx, false);
        assert!(!on(&ctx));
    }

    #[test]
    fn solid_surfaces_are_left_as_they_were() {
        let ctx = Context::default();
        let palette = Palette::dark();
        let frame = panel_frame(&ctx, &palette, palette.panel);
        assert_eq!(frame, Frame::new().fill(palette.panel));
        let menu = Frame::new().fill(palette.overlay);
        assert_eq!(popover_frame(&ctx, &palette, menu, 10), menu);
    }

    #[test]
    fn glass_panels_float_apart_and_let_the_art_through() {
        let ctx = Context::default();
        set(&ctx, true);
        for palette in [Palette::dark(), Palette::light()] {
            let frame = panel_frame(&ctx, &palette, palette.panel);
            assert_eq!(frame.fill, palette.glass_panel());
            assert!(frame.fill.a() < 255);
            assert_eq!(frame.outer_margin, Margin::same(tokens::glass::GUTTER));
            let menu = popover_frame(&ctx, &palette, Frame::new().fill(palette.overlay), 10);
            assert_eq!(menu.fill, palette.glass_popover());
            assert!(menu.fill.a() > frame.fill.a(), "popovers are more opaque");
        }
    }

    #[test]
    fn the_edge_is_lit_along_the_top_and_faint_below() {
        let palette = Palette::dark();
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 400.0));
        let Shape::Path(path) = edge(&palette, rect, CornerRadius::same(20)) else {
            panic!("a path");
        };
        let egui::epaint::ColorMode::UV(color) = &path.stroke.color else {
            panic!("a gradient");
        };
        assert_eq!(
            color(rect, egui::pos2(100.0, 0.0)),
            palette.glass_highlight()
        );
        assert_eq!(color(rect, egui::pos2(0.0, 300.0)), palette.glass_border());
    }

    /// WCAG's relative luminance of an opaque colour.
    fn luminance(color: Color32) -> f32 {
        let linear = |channel: u8| {
            let c = f32::from(channel) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
    }

    /// WCAG's contrast ratio between two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// What can lie behind glass: the plain window, and the window under
    /// each colour an orb can glow in, at the most an orb covers it.
    fn backdrops(palette: &Palette) -> Vec<Color32> {
        let orbs = super::super::art_background::orb_colour_range(palette.dark);
        std::iter::once(palette.window)
            .chain(orbs.into_iter().map(|orb| {
                over(
                    orb.gamma_multiply(super::super::art_background::MAX_OPACITY),
                    palette.window,
                )
            }))
            .collect()
    }

    /// Every glass surface's fill in `palette`, by name: the floating
    /// panels and the search field resting, and popovers and the focused
    /// search field.
    fn surfaces(palette: &Palette) -> [(&'static str, Color32); 2] {
        [
            ("panel", palette.glass_panel()),
            ("popover", palette.glass_popover()),
        ]
    }

    /// Body text, in the text and secondary colours, reads at a WCAG
    /// contrast of at least 4.5 on every glass surface over the plain
    /// window and over every colour the orbs can glow in, in both themes.
    #[test]
    fn body_text_stays_readable_on_glass_over_any_orb() {
        for palette in [Palette::dark(), Palette::light()] {
            let backdrops = backdrops(&palette);
            for (surface, fill) in surfaces(&palette) {
                for (name, text) in [("text", palette.text), ("secondary", palette.secondary)] {
                    let least = backdrops
                        .iter()
                        .map(|&backdrop| contrast(text, over(fill, backdrop)))
                        .fold(f32::INFINITY, f32::min);
                    assert!(
                        least >= 4.5,
                        "{name} on {surface} glass, dark {}: {least:.2}",
                        palette.dark
                    );
                }
            }
        }
    }
}
