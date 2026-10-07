//! A slowly moving gradient behind pages, made from the playing song's art.
//!
//! Each quarter of the cover gives one colour. The colours drift around their
//! own corner of the window while the song plays and hold still when it is
//! paused, and a new song's colours fade in over the old ones.

use std::time::Duration;

use egui::{Color32, Id, Rect, Rgba, Ui, pos2};

use crate::theme::Palette;

const COLUMNS: usize = 16;
const ROWS: usize = 10;
/// Roughly how long a new song's colours take to settle, in seconds.
const FADE_SECONDS: f32 = 0.6;
/// How often the gradient moves while a song plays. The motion is slow, so
/// this is far below the display's rate and costs little.
const FRAME: Duration = Duration::from_millis(50);
/// How far each colour wanders from its corner, as a share of the window.
const DRIFT: f32 = 0.22;
/// Each colour's turning speed in radians per second, and its starting angle.
const ORBITS: [(f64, f64); 4] = [(0.11, 0.0), (0.083, 1.7), (0.097, 3.1), (0.071, 4.4)];
const HOMES: [(f32, f32); 4] = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];

#[derive(Clone, Copy)]
struct State {
    colors: [Rgba; 4],
    phase: f64,
    last: f64,
}

/// Paints the gradient over `rect` for the art's quarter colours.
pub fn paint(ui: &Ui, palette: &Palette, rect: Rect, art: [[u8; 3]; 4], playing: bool) {
    let target = art.map(|rgb| Rgba::from(background_color(palette, rgb)));
    let now = ui.input(|input| input.time);
    let id = Id::new("art-background");
    let window = Rgba::from(palette.window);
    let state = ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_insert_with(id, || State {
            colors: [window; 4],
            phase: 0.0,
            last: now,
        });
        let elapsed = (now - state.last).clamp(0.0, 0.25);
        state.last = now;
        if playing {
            state.phase += elapsed;
        }
        let step = 1.0 - (-(elapsed as f32) / (FADE_SECONDS / 3.0)).exp();
        for (color, target) in state.colors.iter_mut().zip(target) {
            *color = *color * (1.0 - step) + target * step;
        }
        *state
    });
    let settled = state.colors.iter().zip(target).all(|(color, target)| {
        (0..4).all(|channel| (color[channel] - target[channel]).abs() < 0.002)
    });
    if !settled {
        ui.ctx().request_repaint();
    } else if playing {
        ui.ctx().request_repaint_after(FRAME);
    }
    ui.painter()
        .add(egui::Shape::mesh(mesh(rect, &state.colors, state.phase)));
}

/// A colour of the art lifted or lowered to sit behind the palette's text.
fn background_color(palette: &Palette, rgb: [u8; 3]) -> Color32 {
    // A light window shows colour more strongly, so it takes less of it.
    let amount = if palette.dark { 0.7 } else { 0.5 };
    super::blend(palette.window, palette.tint_from_art(rgb), amount)
}

fn centers(phase: f64) -> [(f32, f32); 4] {
    std::array::from_fn(|index| {
        let (speed, start) = ORBITS[index];
        let angle = phase * speed + start;
        let (home_x, home_y) = HOMES[index];
        (
            home_x + DRIFT * angle.cos() as f32,
            home_y + DRIFT * (angle * 1.3).sin() as f32,
        )
    })
}

fn color_at(x: f32, y: f32, centers: &[(f32, f32); 4], colors: &[Rgba; 4]) -> Color32 {
    let mut sum = Rgba::TRANSPARENT;
    let mut total = 0.0;
    for (&(cx, cy), &color) in centers.iter().zip(colors) {
        let distance = (x - cx).powi(2) + (y - cy).powi(2);
        let weight = (-distance / 0.08).exp() + 1e-4;
        sum = sum + color * weight;
        total += weight;
    }
    let mut color = Color32::from(sum * (1.0 / total));
    color[3] = 255;
    color
}

fn mesh(rect: Rect, colors: &[Rgba; 4], phase: f64) -> egui::Mesh {
    let centers = centers(phase);
    let mut mesh = egui::Mesh::default();
    for row in 0..=ROWS {
        for column in 0..=COLUMNS {
            let x = column as f32 / COLUMNS as f32;
            let y = row as f32 / ROWS as f32;
            mesh.colored_vertex(
                pos2(
                    rect.left() + x * rect.width(),
                    rect.top() + y * rect.height(),
                ),
                color_at(x, y, &centers, colors),
            );
        }
    }
    let stride = (COLUMNS + 1) as u32;
    for row in 0..ROWS as u32 {
        for column in 0..COLUMNS as u32 {
            let top_left = row * stride + column;
            let bottom_left = top_left + stride;
            mesh.add_triangle(top_left, top_left + 1, bottom_left + 1);
            mesh.add_triangle(top_left, bottom_left + 1, bottom_left);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba = Rgba::from_rgb(1.0, 0.0, 0.0);
    const GREEN: Rgba = Rgba::from_rgb(0.0, 1.0, 0.0);
    const BLUE: Rgba = Rgba::from_rgb(0.0, 0.0, 1.0);
    const WHITE: Rgba = Rgba::from_rgb(1.0, 1.0, 1.0);

    #[test]
    fn each_corner_takes_the_colour_of_its_quarter() {
        let colors = [RED, GREEN, BLUE, WHITE];
        let centers = HOMES;
        let top_left = Rgba::from(color_at(0.0, 0.0, &centers, &colors));
        let bottom_right = Rgba::from(color_at(1.0, 1.0, &centers, &colors));
        assert!(top_left.r() > top_left.g() && top_left.r() > top_left.b());
        assert!(bottom_right.r() > 0.5 && bottom_right.g() > 0.5 && bottom_right.b() > 0.5);
    }

    #[test]
    fn the_colours_stay_in_the_window_as_they_drift() {
        for step in 0..2000 {
            for (x, y) in centers(step as f64 * 0.5) {
                assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
            }
        }
    }

    #[test]
    fn the_mesh_covers_the_whole_page() {
        let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(810.0, 620.0));
        let mesh = mesh(rect, &[RED; 4], 0.0);
        assert_eq!(mesh.vertices.len(), (COLUMNS + 1) * (ROWS + 1));
        assert_eq!(mesh.indices.len(), COLUMNS * ROWS * 6);
        assert_eq!(mesh.calc_bounds(), rect);
    }
}
