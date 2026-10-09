//! Menus, popovers and other floating layers fade in over
//! [`tokens::motion::BASE`] and out over [`tokens::motion::BASE_OUT`].
//! Dialogs fade in over [`tokens::motion::CONTENT`] instead, growing to
//! their size from [`DIALOG_SCALE`] while their scrim fades in under them.
//!
//! egui draws a popup only while it is open and fades it in at its own
//! animation time, so the fade is applied to the layers once the frame's
//! interface has drawn: a layer that opened recently has its shapes faded
//! to the motion curve, and one that has just closed is drawn once more
//! from its last frame, fading out, where nothing can click it. Only those
//! few frames do any work.

use egui::epaint::ClippedShape;
use egui::layers::ShapeIdx;
use egui::{Context, Id, LayerId, Order, Shape};

use super::{motion, tokens};

/// A floating layer as it was last drawn, for fading it out once it closes.
#[derive(Clone)]
struct Drawn {
    layer: LayerId,
    shapes: Vec<ClippedShape>,
    opacity: f32,
}

/// A layer fading out since `since`, drawn from its last frame. It holds
/// the pictures that frame drew, which egui might otherwise let go: an
/// icon drawn at a size nothing else uses is freed two frames after its
/// last use.
#[derive(Clone)]
struct Closing {
    drawn: Drawn,
    since: f64,
    _textures: Vec<egui::TextureHandle>,
}

#[derive(Clone, Default)]
struct State {
    drawn: Vec<Drawn>,
    closing: Vec<Closing>,
}

fn state_id() -> Id {
    Id::new("popover-transitions")
}

/// How large a dialog starts as it fades in, as a share of its size.
const DIALOG_SCALE: f32 = 0.97;

/// Fades the frame's floating layers in and out. Call it once, after
/// everything has drawn. `steady` names layers that are always there, such
/// as the window's own controls, and never fade; `dialogs` names the modal
/// dialogs' layers.
pub fn fade(ctx: &Context, steady: &[Id], dialogs: &[Id]) {
    let mut state = ctx
        .data(|data| data.get_temp::<State>(state_id()))
        .unwrap_or_default();
    let now = ctx.input(|input| input.time);
    let layers: Vec<LayerId> = ctx.memory(|memory| {
        memory
            .layer_ids()
            .filter(|layer| layer.order == Order::Foreground && !steady.contains(&layer.id))
            .collect()
    });
    let mut drawn = Vec::new();
    for layer in layers {
        let shapes: Vec<ClippedShape> = ctx.graphics(|graphics| {
            graphics
                .get(layer)
                .map(|list| list.all_entries().cloned().collect())
                .unwrap_or_default()
        });
        if shapes.is_empty() {
            continue;
        }
        let dialog = dialogs.contains(&layer.id);
        let seconds = if dialog {
            tokens::motion::CONTENT
        } else {
            tokens::motion::BASE
        };
        let opacity = opening(ctx, layer.id, seconds);
        if dialog && opacity < 1.0 {
            grow(ctx, layer, egui::lerp(DIALOG_SCALE..=1.0, opacity));
        }
        let factor = opacity / egui_fade_in(ctx, layer.id);
        if factor < 1.0 {
            ctx.graphics_mut(|graphics| {
                if let Some(list) = graphics.get_mut(layer) {
                    let count = list.all_entries().len();
                    for index in 0..count {
                        list.mutate_shape(ShapeIdx(index), |clipped| {
                            fade_shape(&mut clipped.shape, factor);
                        });
                    }
                }
            });
        }
        drawn.push(Drawn {
            layer,
            shapes,
            opacity,
        });
    }
    // A layer drawn last frame and not in this one closed after it, so its
    // fade starts from that frame.
    let reduced = motion::reduced(ctx);
    let closed_at = now - f64::from(ctx.input(|input| input.stable_dt));
    for last in std::mem::take(&mut state.drawn) {
        if !reduced && !drawn.iter().any(|now| now.layer == last.layer) {
            let textures = hold_textures(ctx, &last.shapes);
            state.closing.push(Closing {
                drawn: last,
                since: closed_at,
                _textures: textures,
            });
        }
    }
    state.closing.retain(|closing| {
        let reopened = drawn.iter().any(|now| now.layer == closing.drawn.layer);
        let left = (1.0 - motion::since(ctx, closing.since, tokens::motion::BASE_OUT))
            * closing.drawn.opacity;
        if reopened || left <= 0.0 {
            return false;
        }
        paint_closing(ctx, &closing.drawn, left);
        true
    });
    state.drawn = drawn;
    ctx.data_mut(|data| {
        if state.drawn.is_empty() && state.closing.is_empty() {
            data.remove::<State>(state_id());
        } else {
            data.insert_temp(state_id(), state);
        }
    });
}

/// How far a layer that became visible recently has faded in over
/// `seconds`.
fn opening(ctx: &Context, id: Id, seconds: f32) -> f32 {
    match became_visible(ctx, id) {
        Some(since) => motion::since(ctx, since, seconds),
        None => 1.0,
    }
}

/// Scales a dialog's layer to `scale` of its size about its centre. The
/// scrim, which covers the whole window, stays as it is.
fn grow(ctx: &Context, layer: LayerId, scale: f32) {
    let window = ctx.content_rect();
    let covers_window = |shape: &Shape| shape.visual_bounding_rect().contains_rect(window);
    ctx.graphics_mut(|graphics| {
        let Some(list) = graphics.get_mut(layer) else {
            return;
        };
        let bounds = list
            .all_entries()
            .filter(|clipped| !covers_window(&clipped.shape))
            .fold(egui::Rect::NOTHING, |bounds, clipped| {
                bounds.union(clipped.shape.visual_bounding_rect())
            });
        if !bounds.is_positive() {
            return;
        }
        let centre = bounds.center().to_vec2();
        let transform = egui::emath::TSTransform::from_translation(centre)
            * egui::emath::TSTransform::from_scaling(scale)
            * egui::emath::TSTransform::from_translation(-centre);
        let count = list.all_entries().len();
        for index in 0..count {
            list.mutate_shape(ShapeIdx(index), |clipped| {
                if !covers_window(&clipped.shape) {
                    clipped.shape.transform(transform);
                }
            });
        }
    });
}

/// The opacity egui's own fade has given an area this frame, which the
/// motion curve replaces. It mirrors `egui::Area`'s fade-in.
fn egui_fade_in(ctx: &Context, id: Id) -> f32 {
    let animation_time = ctx.global_style().animation_time;
    let Some(since) = became_visible(ctx, id).filter(|_| animation_time > 0.0) else {
        return 1.0;
    };
    let age = ctx.input(|input| (input.time - since) as f32 + input.predicted_dt / 2.0);
    egui::emath::easing::quadratic_out(egui::remap_clamp(age, 0.0..=animation_time, 0.0..=1.0))
        .max(f32::EPSILON)
}

fn became_visible(ctx: &Context, id: Id) -> Option<f64> {
    egui::AreaState::load(ctx, id)?.last_became_visible_at
}

/// Draws a closed layer's last frame at `opacity`, on a layer of its own
/// that holds no widgets, so nothing in it answers the pointer.
fn paint_closing(ctx: &Context, drawn: &Drawn, opacity: f32) {
    let layer = LayerId::new(Order::Foreground, drawn.layer.id.with("closing"));
    ctx.graphics_mut(|graphics| {
        let list = graphics.entry(layer);
        for clipped in &drawn.shapes {
            let mut shape = clipped.shape.clone();
            fade_shape(&mut shape, opacity);
            list.add(clipped.clip_rect, shape);
        }
    });
}

/// A hold on every picture `shapes` draw, released when dropped.
fn hold_textures(ctx: &Context, shapes: &[ClippedShape]) -> Vec<egui::TextureHandle> {
    let manager = ctx.tex_manager();
    let mut held: Vec<egui::TextureHandle> = Vec::new();
    for clipped in shapes {
        let id = clipped.shape.texture_id();
        if id == egui::TextureId::default() || held.iter().any(|handle| handle.id() == id) {
            continue;
        }
        let mut textures = manager.write();
        if textures.meta(id).is_some() {
            textures.retain(id);
            held.push(egui::TextureHandle::new(manager.clone(), id));
        }
    }
    held
}

/// Multiplies a shape's opacity by `factor`.
fn fade_shape(shape: &mut Shape, factor: f32) {
    match shape {
        Shape::Vec(shapes) => {
            for shape in shapes {
                fade_shape(shape, factor);
            }
        }
        // Text fades through its own factor, which leaves the galley shared.
        Shape::Text(text) => text.opacity_factor *= factor,
        _ => egui::epaint::shape_transform::adjust_colors(shape, move |color| {
            if *color != egui::Color32::PLACEHOLDER {
                *color = color.gamma_multiply(factor);
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs one frame `dt` after the last, with a menu-like area shown
    /// while `open`, and returns the alpha of its fill and whether the
    /// frame asked for another.
    fn frame(ctx: &Context, time: &mut f64, dt: f32, open: bool) -> (Option<u8>, bool) {
        *time += f64::from(dt);
        let input = egui::RawInput {
            time: Some(*time),
            predicted_dt: dt,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            if open {
                egui::Area::new(Id::new("menu"))
                    .order(Order::Foreground)
                    .fixed_pos(egui::pos2(10.0, 10.0))
                    .show(ui.ctx(), |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(40.0, 20.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 0.0, egui::Color32::WHITE);
                    });
            }
            fade(ui.ctx(), &[], &[]);
        });
        output.textures_delta.clear();
        let alpha = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Rect(rect) if rect.fill != egui::Color32::TRANSPARENT => Some(rect.fill.a()),
                _ => None,
            });
        let repaint = output
            .viewport_output
            .values()
            .any(|viewport| viewport.repaint_delay.is_zero());
        (alpha, repaint)
    }

    #[test]
    fn a_menu_fades_in_then_out_and_then_rests() {
        let ctx = Context::default();
        let mut now = 0.0;
        let dt = 0.03;
        let mut opening = Vec::new();
        for _ in 0..8 {
            opening.push(frame(&ctx, &mut now, dt, true).0.unwrap_or(0));
        }
        // The first frame only measures the menu; then it rises to full.
        let shown: Vec<u8> = opening
            .into_iter()
            .skip_while(|alpha| *alpha == 0)
            .collect();
        assert!(shown[0] < 255, "{shown:?}");
        assert!(shown.windows(2).all(|pair| pair[0] <= pair[1]), "{shown:?}");
        assert_eq!(*shown.last().unwrap(), 255, "{shown:?}");
        // Faded at the motion curve, not egui's quicker one: 90 ms into
        // 180 it is not yet whole.
        assert!(shown.len() > 4, "{shown:?}");

        let (closing, repaint) = frame(&ctx, &mut now, dt, false);
        let closing = closing.expect("the closed menu fades out");
        assert!(closing > 0 && closing < 255);
        assert!(repaint);
        let mut last = closing;
        for _ in 0..3 {
            if let (Some(alpha), _) = frame(&ctx, &mut now, dt, false) {
                assert!(alpha <= last);
                last = alpha;
            }
        }
        assert_eq!(
            frame(&ctx, &mut now, dt, false).0,
            None,
            "gone after 120 ms"
        );
        let _ = frame(&ctx, &mut now, dt, false);
        assert!(
            !frame(&ctx, &mut now, dt, false).1,
            "and then nothing asks for frames"
        );
    }

    #[test]
    fn reduce_motion_shows_and_hides_a_menu_at_once() {
        let ctx = Context::default();
        let mut now = 0.0;
        let dt = 0.03;
        motion::set_reduced(&ctx, true);
        let mut alphas = Vec::new();
        for _ in 0..3 {
            alphas.push(frame(&ctx, &mut now, dt, true).0);
        }
        assert_eq!(alphas.last().copied().flatten(), Some(255), "{alphas:?}");
        assert_eq!(frame(&ctx, &mut now, dt, false).0, None);
    }
}
