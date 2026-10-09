//! How the interface moves between states: every transition runs for one
//! of the [`tokens::motion`] durations along [`tokens::motion::ease`], and
//! Reduce motion makes every duration zero.
//!
//! Everything goes through egui's `animate_*_with_time`, which asks for the
//! next frame only while a value is still moving, so a settled window does
//! no work.

use egui::{Color32, Context, Id};

use super::tokens;

fn reduced_id() -> Id {
    Id::new("reduce-motion")
}

/// Turns Reduce motion on or off for the frames that follow. egui's own
/// fades (tooltips, collapsing headers, scrolling) follow it too.
pub fn set_reduced(ctx: &Context, reduced: bool) {
    if self::reduced(ctx) != reduced {
        ctx.data_mut(|data| data.insert_temp(reduced_id(), reduced));
    }
    let animation_time = if reduced { 0.0 } else { tokens::motion::FAST };
    if ctx.global_style().animation_time != animation_time {
        ctx.global_style_mut(|style| style.animation_time = animation_time);
    }
}

/// Whether Reduce motion is on.
pub fn reduced(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp(reduced_id()).unwrap_or(false))
}

/// How long a transition of `seconds` lasts: no time at all with Reduce
/// motion on.
pub fn time(ctx: &Context, seconds: f32) -> f32 {
    if reduced(ctx) { 0.0 } else { seconds }
}

/// Eases from 0 toward 1 while `on`, and back while not, over `seconds`.
/// The first call for an `id` starts where it is told to be, so a control
/// drawn for the first time does not animate in.
pub fn toward(ctx: &Context, id: Id, on: bool, seconds: f32) -> f32 {
    ctx.animate_bool_with_time_and_easing(id, on, time(ctx, seconds), tokens::motion::ease)
}

/// Hover feedback: [`toward`] at [`tokens::motion::FAST`].
pub fn hover(ctx: &Context, id: Id, on: bool) -> f32 {
    toward(ctx, id, on, tokens::motion::FAST)
}

/// Lays `fill` under a row, card or tile as the pointer arrives, and lifts
/// it as the pointer leaves, at [`tokens::motion::FAST`].
pub fn hover_fill(
    ui: &egui::Ui,
    id: Id,
    hovered: bool,
    rect: egui::Rect,
    corner: impl Into<egui::CornerRadius>,
    fill: Color32,
) {
    let shown = hover(ui.ctx(), id.with("hover-fill"), hovered);
    if shown > 0.0 {
        ui.painter()
            .rect_filled(rect, corner, fill.gamma_multiply(shown));
    }
}

/// The colour `t` of the way from `from` to `to`.
pub fn mix(from: Color32, to: Color32, t: f32) -> Color32 {
    if t <= 0.0 {
        from
    } else if t >= 1.0 {
        to
    } else {
        from.lerp_to_gamma(to, t)
    }
}

/// How far a transition that began at `since` (in egui's clock) has come
/// after `seconds`, eased, asking for the next frame until it arrives.
pub fn since(ctx: &Context, since: f64, seconds: f32) -> f32 {
    let seconds = time(ctx, seconds);
    if seconds <= 0.0 {
        return 1.0;
    }
    let elapsed = ctx.input(|input| input.time - since) as f32;
    let progress = (elapsed / seconds).clamp(0.0, 1.0);
    if progress < 1.0 {
        ctx.request_repaint();
    }
    tokens::motion::ease(progress)
}

/// The macOS accessibility preference to reduce motion, which Spotifast
/// follows until the person chooses in Settings.
#[cfg(target_os = "macos")]
pub fn system_prefers_reduced() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// Other desktops: motion stays on until the person turns it off.
#[cfg(not(target_os = "macos"))]
pub fn system_prefers_reduced() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs one frame `dt` seconds after the last, returning what `f` gave
    /// and whether the frame asked for another.
    fn frame<R>(
        ctx: &Context,
        time: &mut f64,
        dt: f32,
        mut f: impl FnMut(&Context) -> R,
    ) -> (R, bool) {
        *time += f64::from(dt);
        let input = egui::RawInput {
            time: Some(*time),
            predicted_dt: dt,
            ..Default::default()
        };
        let mut result = None;
        let mut output = ctx.run_ui(input, |ui| result = Some(f(ui.ctx())));
        output.textures_delta.clear();
        let repaint = output
            .viewport_output
            .values()
            .any(|viewport| viewport.repaint_delay.is_zero());
        (result.expect("the frame ran"), repaint)
    }

    #[test]
    fn a_transition_eases_over_its_duration_and_then_rests() {
        let ctx = Context::default();
        let id = Id::new("t");
        let mut now = 0.0;
        let dt = 1.0 / 60.0;
        let (start, _) = frame(&ctx, &mut now, dt, |ctx| toward(ctx, id, false, 0.12));
        assert_eq!(start, 0.0, "the first call starts where it is told");
        let mut values = Vec::new();
        for _ in 0..12 {
            values.push(frame(&ctx, &mut now, dt, |ctx| toward(ctx, id, true, 0.12)));
        }
        assert!(values.windows(2).all(|pair| pair[0].0 <= pair[1].0));
        assert!(
            values[0].0 > 0.0 && values[0].1,
            "moving, and asking for frames"
        );
        // Ease-out: the first steps cover more than an even share.
        assert!(values[0].0 > dt / 0.12);
        let (last, repaint) = *values.last().unwrap();
        assert_eq!(last, 1.0);
        assert!(!repaint, "a settled value asks for nothing");
    }

    #[test]
    fn reduce_motion_makes_every_transition_instant() {
        let ctx = Context::default();
        let id = Id::new("t");
        let mut now = 0.0;
        let dt = 1.0 / 60.0;
        let _ = frame(&ctx, &mut now, dt, |ctx| {
            set_reduced(ctx, true);
            toward(ctx, id, false, tokens::motion::PANEL)
        });
        assert!(reduced(&ctx));
        assert_eq!(ctx.global_style().animation_time, 0.0);
        assert_eq!(time(&ctx, tokens::motion::CONTENT), 0.0);
        let (value, _) = frame(&ctx, &mut now, dt, |ctx| {
            toward(ctx, id, true, tokens::motion::PANEL)
        });
        assert_eq!(value, 1.0);
        // egui paints once more after a request, then rests.
        let _ = frame(&ctx, &mut now, dt, |ctx| {
            toward(ctx, id, true, tokens::motion::PANEL)
        });
        let (_, repaint) = frame(&ctx, &mut now, dt, |ctx| {
            toward(ctx, id, true, tokens::motion::PANEL)
        });
        assert!(!repaint, "nothing moves, so nothing asks for frames");
        let started = now;
        let (value, _) = frame(&ctx, &mut now, dt, |ctx| {
            since(ctx, started, tokens::motion::PAGE)
        });
        assert_eq!(value, 1.0);

        let _ = frame(&ctx, &mut now, dt, |ctx| set_reduced(ctx, false));
        assert!(!reduced(&ctx));
        assert_eq!(ctx.global_style().animation_time, tokens::motion::FAST);
        assert_eq!(time(&ctx, tokens::motion::CONTENT), tokens::motion::CONTENT);
    }

    #[test]
    fn progress_since_a_moment_eases_to_one() {
        let ctx = Context::default();
        let mut now = 0.0;
        let dt = 0.05;
        let (_, _) = frame(&ctx, &mut now, dt, |_| ());
        let started = now;
        let (halfway, repaint) = frame(&ctx, &mut now, dt, |ctx| since(ctx, started, 0.1));
        assert!(halfway > 0.5 && halfway < 1.0, "{halfway}");
        assert!(repaint);
        let (done, _) = frame(&ctx, &mut now, dt, |ctx| since(ctx, started, 0.1));
        assert_eq!(done, 1.0);
        // egui paints once more after a request, then rests.
        let _ = frame(&ctx, &mut now, dt, |ctx| since(ctx, started, 0.1));
        let (_, repaint) = frame(&ctx, &mut now, dt, |ctx| since(ctx, started, 0.1));
        assert!(!repaint);
    }

    #[test]
    fn colours_mix_from_one_end_to_the_other() {
        let (a, b) = (Color32::from_rgb(0, 0, 0), Color32::from_rgb(200, 100, 50));
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        let half = mix(a, b, 0.5);
        assert!(half.r() > 0 && half.r() < 200);
    }
}
