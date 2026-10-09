//! Soft orbs of colour drifting behind pages, made from album art: a
//! hovered card's cover (see [`super::card_hover`]), else the page's own
//! cover, else the playing song's.
//!
//! Each quarter of the cover gives one orb its colour, and the cover's
//! average a fifth, made vivid and drawn a little toward each other so they
//! stay in one family. They glow most at the top of the page and give way
//! to the plain window below, where the page's words sit, so the art colours
//! the room without competing with the page. A new cover's colours fade in
//! over the old ones.
//!
//! The orbs follow the music playing here (see [`crate::vis_energy`]): the
//! louder and quicker it is, the larger and brighter they glow and the
//! faster they wander, and each beat makes them swell softly. Music playing
//! on another device gives them a gentle drift of their own, and a paused
//! song leaves them still.

use std::time::Duration;

use egui::{Color32, Id, Mesh, Pos2, Rect, Rgba, Shape, Ui, ecolor::Hsva, pos2};

use crate::theme::Palette;
use crate::vis_energy::Vibe;

/// The number of orbs: one per quarter of the cover and one for its
/// average.
const ORBS: usize = 5;
/// Rings and segments in each orb's mesh: enough for a smooth fall-off.
const RINGS: usize = 10;
const SEGMENTS: usize = 40;
/// Roughly how long a new song's colours take to settle, in seconds.
const FADE_SECONDS: f32 = 0.6;
/// How often the orbs move while music plays here, smooth enough to follow
/// a beat yet far below the display's rate.
const MUSIC_FRAME: Duration = Duration::from_millis(33);
/// How often they move while music plays on another device, when their
/// motion is slow and never sudden.
const DRIFT_FRAME: Duration = Duration::from_millis(80);
/// The most an orb covers the window at any point, however loud the music,
/// so the page's words always read over it.
pub(super) const MAX_OPACITY: f32 = 0.7;
/// How much of its colour each orb gives up to the cover's average.
const HARMONY: f32 = 0.25;
/// Where each orb wanders around, as a share of the page, and its radius
/// as a share of the page's longer side.
const HOMES: [(f32, f32); ORBS] = [
    (0.18, 0.2),
    (0.82, 0.16),
    (0.28, 0.72),
    (0.8, 0.68),
    (0.52, 0.4),
];
const RADII: [f32; ORBS] = [0.42, 0.38, 0.4, 0.36, 0.3];
/// How far each orb wanders from its home, across and down.
const REACH: (f32, f32) = (0.16, 0.12);
/// Each orb's turning speeds across and down in radians per second at a
/// calm pace, and its starting angles.
const PATHS: [(f64, f64, f64, f64); ORBS] = [
    (0.071, 0.059, 0.0, 1.3),
    (0.053, 0.067, 1.7, 0.4),
    (0.062, 0.043, 3.1, 2.2),
    (0.047, 0.077, 4.4, 5.0),
    (0.083, 0.051, 2.6, 3.7),
];

/// What moves the orbs this frame.
#[derive(Clone, Copy, Debug)]
pub enum Motion<'a> {
    /// Nothing plays: they hold still, letting a last beat settle.
    Still,
    /// Music plays on another device, so there is nothing to listen to.
    Drift,
    /// Music plays here; the newest samples of the visualisers' sound.
    Music(&'a [f32]),
}

#[derive(Clone, Copy)]
struct State {
    colors: [Rgba; ORBS],
    phase: f64,
    last: f64,
    vibe: Vibe,
}

/// The orbs over `rect` for the art's quarter colours. Whichever art they
/// are given, the colours fade from what they showed before.
pub fn shape(
    ui: &Ui,
    palette: &Palette,
    rect: Rect,
    art: [[u8; 3]; 4],
    motion: Motion<'_>,
) -> Shape {
    let target = orb_colors(art, palette.dark);
    let now = ui.input(|input| input.time);
    let id = Id::new("art-background");
    let window = Rgba::from(palette.window);
    let state = ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_insert_with(id, || State {
            colors: [window; ORBS],
            phase: 0.0,
            last: now,
            vibe: Vibe::default(),
        });
        let elapsed = (now - state.last).clamp(0.0, 0.25);
        state.last = now;
        let samples = match motion {
            Motion::Music(samples) => samples,
            Motion::Still | Motion::Drift => &[],
        };
        state.vibe.step(samples, elapsed as f32);
        state.phase += elapsed * f64::from(pace(motion, &state.vibe));
        let step = 1.0 - (-(elapsed as f32) / (FADE_SECONDS / 3.0)).exp();
        for (color, target) in state.colors.iter_mut().zip(target) {
            *color = *color * (1.0 - step) + target * step;
        }
        *state
    });
    let settled = state.colors.iter().zip(target).all(|(color, target)| {
        (0..4).all(|channel| (color[channel] - target[channel]).abs() < 0.002)
    });
    match next_frame(settled, state.vibe.calm(), motion) {
        Some(Duration::ZERO) => ui.ctx().request_repaint(),
        Some(delay) => ui.ctx().request_repaint_after(delay),
        None => {}
    }
    // A light window shows colour more strongly, so it takes less of it.
    let strength = if palette.dark { 0.6 } else { 0.62 };
    Shape::mesh(mesh(
        rect,
        &state.colors,
        state.phase,
        &state.vibe,
        strength,
    ))
}

/// When the orbs next need drawing: at once while colours fade, at a
/// steady pace while they move, and not at all once they rest.
fn next_frame(settled: bool, calm: bool, motion: Motion<'_>) -> Option<Duration> {
    if !settled {
        return Some(Duration::ZERO);
    }
    match motion {
        Motion::Music(_) => Some(MUSIC_FRAME),
        Motion::Drift => Some(DRIFT_FRAME),
        // A pause lets the last swell die down before the orbs rest.
        Motion::Still if !calm => Some(MUSIC_FRAME),
        Motion::Still => None,
    }
}

/// How fast the orbs wander, relative to a calm song: slow for quiet music,
/// up to about three times as fast for loud, quick music.
fn pace(motion: Motion<'_>, vibe: &Vibe) -> f32 {
    match motion {
        Motion::Still => 0.0,
        Motion::Drift => 1.0,
        Motion::Music(_) => 0.6 + 1.4 * vibe.energy() + 0.8 * vibe.pace(),
    }
}

/// The orbs' colours: the cover's quarters and its average, made vivid and
/// drawn part of the way toward their average.
fn orb_colors(art: [[u8; 3]; 4], dark: bool) -> [Rgba; ORBS] {
    let quarters = art.map(|rgb| vivid(rgb, dark));
    let average = quarters
        .iter()
        .fold(Rgba::TRANSPARENT, |sum, &color| sum + color)
        * 0.25;
    std::array::from_fn(|index| {
        let color = quarters.get(index).copied().unwrap_or(average);
        color * (1.0 - HARMONY) + average * HARMONY
    })
}

/// `rgb` made rich and bright: saturated unless it is close to grey, and
/// at one brightness, a little lower over a dark window where it glows
/// more.
fn vivid(rgb: [u8; 3], dark: bool) -> Rgba {
    let mut color = Hsva::from(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
    if color.s > 0.08 {
        color.s = (color.s * 1.4).clamp(0.55, 0.95);
    }
    color.v = if dark { 0.8 } else { 0.9 };
    color.a = 1.0;
    Rgba::from(color)
}

/// Every colour an orb can glow in, sampled: [`vivid`] around the hue
/// circle at the least and the most saturation it gives, and grey, so what
/// lies over the orbs can be checked against all of them.
#[cfg(test)]
pub(super) fn orb_colour_range(dark: bool) -> Vec<Color32> {
    let mut colours = vec![Color32::from(vivid([128, 128, 128], dark))];
    for hue in (0..360).step_by(5) {
        for saturation in [0.4, 1.0] {
            let rgb = Color32::from(Hsva::new(hue as f32 / 360.0, saturation, 1.0, 1.0));
            colours.push(Color32::from(vivid([rgb.r(), rgb.g(), rgb.b()], dark)));
        }
    }
    colours
}

/// How much colour shows at height `y` of the page, from 1 at the top to a
/// faint wash at the bottom, so the words below stay clear.
fn falloff(y: f32) -> f32 {
    let y = y.clamp(0.0, 1.0);
    0.16 + 0.84 * (1.0 - y).powf(1.4)
}

/// An orb's opacity at `distance` from its centre, as a share of its
/// radius: a bell that reaches nothing at the edge.
fn glow(distance: f32) -> f32 {
    const SPREAD: f32 = 3.2;
    let edge = (-SPREAD).exp();
    (((-SPREAD * distance * distance).exp() - edge) / (1.0 - edge)).max(0.0)
}

/// Where each orb's centre is at `phase`, as a share of the page.
fn centers(phase: f64) -> [(f32, f32); ORBS] {
    std::array::from_fn(|index| {
        let (across, down, start_x, start_y) = PATHS[index];
        let (home_x, home_y) = HOMES[index];
        let (reach_x, reach_y) = REACH;
        let x =
            (phase * across + start_x).sin() * 0.8 + (phase * across * 2.3 + start_y).sin() * 0.2;
        let y = (phase * down + start_y).sin() * 0.8 + (phase * down * 1.7 + start_x).cos() * 0.2;
        (home_x + reach_x * x as f32, home_y + reach_y * y as f32)
    })
}

fn mesh(rect: Rect, colors: &[Rgba; ORBS], phase: f64, vibe: &Vibe, strength: f32) -> Mesh {
    let grow = 1.0 + 0.1 * vibe.energy() + 0.07 * vibe.pulse();
    let brighten = 0.85 + 0.2 * vibe.energy() + 0.15 * vibe.pulse();
    let size = rect.width().max(rect.height());
    let mut mesh = Mesh::default();
    for ((&(x, y), &color), radius) in centers(phase).iter().zip(colors).zip(RADII) {
        let color = Color32::from(color);
        let center = pos2(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        );
        orb(
            &mut mesh,
            center,
            radius * size * grow,
            |point, distance| {
                let height = (point.y - rect.top()) / rect.height().max(1.0);
                // Faded in the display's own space, as egui blends, so a
                // faint edge stays the orb's colour instead of washing out.
                color.gamma_multiply(
                    (strength * brighten * glow(distance) * falloff(height)).min(MAX_OPACITY),
                )
            },
        );
    }
    mesh
}

/// Adds a disc to `mesh` around `center`, coloured at each point by
/// `paint(point, distance)`, where distance runs from 0 at the centre to 1
/// at the edge.
fn orb(mesh: &mut Mesh, center: Pos2, radius: f32, paint: impl Fn(Pos2, f32) -> Color32) {
    let first = mesh.vertices.len() as u32;
    mesh.colored_vertex(center, paint(center, 0.0));
    for ring in 1..=RINGS {
        let distance = ring as f32 / RINGS as f32;
        for segment in 0..SEGMENTS {
            let angle = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let point = center + radius * distance * egui::vec2(angle.cos(), angle.sin());
            mesh.colored_vertex(point, paint(point, distance));
        }
    }
    let at = |ring: usize, segment: usize| {
        first + 1 + ((ring - 1) * SEGMENTS + segment % SEGMENTS) as u32
    };
    for segment in 0..SEGMENTS {
        mesh.add_triangle(first, at(1, segment), at(1, segment + 1));
    }
    for ring in 1..RINGS {
        for segment in 0..SEGMENTS {
            let (inner, inner_next) = (at(ring, segment), at(ring, segment + 1));
            let (outer, outer_next) = (at(ring + 1, segment), at(ring + 1, segment + 1));
            mesh.add_triangle(inner, outer, outer_next);
            mesh.add_triangle(inner, outer_next, inner_next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 3] = [200, 40, 40];
    const GREEN: [u8; 3] = [40, 160, 60];
    const BLUE: [u8; 3] = [40, 60, 200];
    const GREY: [u8; 3] = [128, 128, 128];

    #[test]
    fn each_orb_takes_the_colour_of_its_quarter() {
        let [red, green, blue, ..] = orb_colors([RED, GREEN, BLUE, GREY], true);
        assert!(red.r() > red.g() && red.r() > red.b());
        assert!(green.g() > green.r() && green.g() > green.b());
        assert!(blue.b() > blue.r() && blue.b() > blue.g());
    }

    #[test]
    fn orb_colours_move_toward_one_family() {
        let [red, green, ..] = orb_colors([RED, GREEN, BLUE, GREY], true);
        assert!(red.g() > 0.0 && green.r() > 0.0);
        assert!(red.r() > red.g(), "each keeps its own character");
    }

    #[test]
    fn colours_are_made_vivid_but_grey_stays_grey() {
        let dull = [150, 120, 110];
        let before = Hsva::from(Color32::from_rgb(150, 120, 110));
        let after = Hsva::from(Color32::from(vivid(dull, true)));
        assert!(
            after.s >= 0.55 && after.s > before.s,
            "{} {}",
            before.s,
            after.s
        );
        let grey = Hsva::from(Color32::from(vivid(GREY, false)));
        assert!(grey.s < 0.05);
    }

    #[test]
    fn the_colour_fades_toward_the_bottom_of_the_page() {
        assert_eq!(falloff(0.0), 1.0);
        assert!(falloff(1.0) < 0.2);
        assert!(
            (0..10).all(|step| falloff(step as f32 / 10.0) > falloff((step + 1) as f32 / 10.0))
        );
    }

    #[test]
    fn an_orb_is_brightest_in_the_middle_and_gone_at_its_edge() {
        assert_eq!(glow(0.0), 1.0);
        assert_eq!(glow(1.0), 0.0);
        assert!((0..10).all(|step| glow(step as f32 / 10.0) > glow((step + 1) as f32 / 10.0)));
    }

    #[test]
    fn the_orbs_stay_over_the_page_as_they_drift() {
        for step in 0..4000 {
            for (x, y) in centers(step as f64 * 0.5) {
                assert!((-0.05..=1.05).contains(&x) && (-0.05..=1.05).contains(&y));
            }
        }
    }

    #[test]
    fn the_orbs_drift_apart_from_each_other() {
        let start = centers(0.0);
        let later = centers(40.0);
        assert!(start.iter().zip(later).all(|(a, b)| a != &b), "all move");
    }

    #[test]
    fn the_orbs_are_soft_meshes_of_modest_size() {
        let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(810.0, 620.0));
        let colors = orb_colors([RED, GREEN, BLUE, GREY], true);
        let mesh = mesh(rect, &colors, 0.0, &Vibe::default(), 0.5);
        let per_orb = 1 + RINGS * SEGMENTS;
        assert_eq!(mesh.vertices.len(), ORBS * per_orb);
        assert_eq!(
            mesh.indices.len(),
            ORBS * (SEGMENTS + (RINGS - 1) * SEGMENTS * 2) * 3
        );
        assert!(mesh.is_valid());
        // Every orb's rim is fully transparent, so no edge shows.
        for orb in mesh.vertices.chunks(per_orb) {
            assert!(orb[per_orb - SEGMENTS..].iter().all(|v| v.color.a() == 0));
            assert!(orb[0].color.a() > 0);
        }
    }

    #[test]
    fn the_orbs_never_cover_the_page_completely() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1280.0, 800.0));
        let colors = orb_colors([RED, GREEN, BLUE, GREY], true);
        let mut vibe = Vibe::default();
        // As loud and on the beat as music gets.
        for _ in 0..60 {
            vibe.step(&[0.9; 512], 1.0 / 30.0);
        }
        let mesh = mesh(rect, &colors, 0.0, &vibe, 0.5);
        assert!(mesh.vertices.iter().all(|v| v.color.a() <= 179));
    }

    #[test]
    fn louder_music_makes_bigger_brighter_orbs() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1280.0, 800.0));
        let colors = orb_colors([RED, GREEN, BLUE, GREY], true);
        let quiet = mesh(rect, &colors, 0.0, &Vibe::default(), 0.3);
        let mut vibe = Vibe::default();
        for _ in 0..60 {
            vibe.step(&[0.5; 512], 1.0 / 30.0);
        }
        let loud = mesh(rect, &colors, 0.0, &vibe, 0.3);
        assert!(loud.calc_bounds().area() > quiet.calc_bounds().area());
        assert!(loud.vertices[0].color.a() > quiet.vertices[0].color.a());
    }

    #[test]
    fn music_here_moves_the_orbs_faster_than_a_calm_drift() {
        let mut vibe = Vibe::default();
        assert_eq!(pace(Motion::Still, &vibe), 0.0);
        assert_eq!(pace(Motion::Drift, &vibe), 1.0);
        let quiet = pace(Motion::Music(&[]), &vibe);
        for _ in 0..60 {
            vibe.step(&[0.5; 512], 1.0 / 30.0);
        }
        let loud = pace(Motion::Music(&[]), &vibe);
        assert!(quiet < 1.0 && loud > 1.5, "{quiet} {loud}");
    }

    #[test]
    fn nothing_is_drawn_again_once_paused_and_settled() {
        assert_eq!(next_frame(true, true, Motion::Still), None);
        assert_eq!(
            next_frame(true, false, Motion::Still),
            Some(MUSIC_FRAME),
            "a last beat settles first"
        );
        assert_eq!(next_frame(false, true, Motion::Still), Some(Duration::ZERO));
        assert_eq!(next_frame(true, true, Motion::Drift), Some(DRIFT_FRAME));
        assert_eq!(
            next_frame(true, true, Motion::Music(&[])),
            Some(MUSIC_FRAME)
        );
    }

    #[test]
    fn a_paused_song_asks_for_no_more_frames() {
        let ctx = egui::Context::default();
        let palette = Palette::dark();
        let delays = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = std::sync::Arc::clone(&delays);
        ctx.set_request_repaint_callback(move |info| seen.lock().unwrap().push(info.delay));
        let frame = |time: f64, motion: Motion<'_>| {
            let input = egui::RawInput {
                time: Some(time),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let rect = ui.max_rect();
                let _ = shape(ui, &palette, rect, [RED, GREEN, BLUE, GREY], motion);
            });
            output.textures_delta.clear();
        };
        // Long enough for the colours and any swell to settle.
        let mut time = 0.0;
        for _ in 0..100 {
            time += 0.25;
            frame(time, Motion::Still);
        }
        delays.lock().unwrap().clear();
        frame(time + 0.25, Motion::Still);
        assert!(delays.lock().unwrap().is_empty(), "{:?}", delays.lock());
        frame(time + 0.5, Motion::Drift);
        assert!(!delays.lock().unwrap().is_empty());
    }
}
