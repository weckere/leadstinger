//! The waveform editor's icon buttons (D-276): small glyphs drawn with egui shapes, as
//! the pad options' are, so they follow the theme's button colours and need no font.
//! Each button keeps the name it had as text, for screen readers and the tests.

use super::*;
use std::f32::consts::TAU;

/// What a button's glyph shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// A marker's flag with a plus: add a marker at the cursor.
    MarkerAdd,
    /// The cursor's dashed line with the trim's opening bracket on it: start here.
    StartHere,
    /// The same with the closing bracket: end here.
    EndHere,
    /// Brackets around a shaded stretch: trim to the selection.
    TrimSelection,
    /// A loop over a shaded stretch: vamp the selection.
    VampSelection,
    /// A play triangle between brackets: play the selection.
    PlaySelection,
    /// A square: stop.
    Stop,
    /// A shaded stretch crossed out: clear the selection.
    ClearSelection,
    /// A playhead with a word block starting on it: start the word at the playhead.
    StartAtPlayhead,
    /// Two blocks pressed together: join the word and the next.
    JoinNext,
    /// A turning arrow: put the line back.
    Reset,
    /// Three even dashed blocks: split the line evenly again.
    SplitAgain,
    /// A word block with its start, its end or the whole of it marked: what the arrows move.
    GripStart,
    GripEnd,
    GripWord,
    /// A little waveform turning into two lines of text: transcribe the sound.
    Transcribe,
    /// Two word blocks whose starts sit on the sound-start ticks below: time the words.
    AlignWords,
    /// A subtitle line's bar over its words, with a plus: add a line.
    AddLine,
    /// The same with a minus: delete the line.
    DeleteLine,
}

/// An icon button: `glyph` on a small button, named `name` for screen readers; `selected`
/// draws it pressed (a toggle).
pub fn button(ui: &mut Ui, glyph: Glyph, name: &str, enabled: bool, selected: bool) -> egui::Response {
    let resp = ui.add_enabled(enabled, theme::small(egui::Button::new("").min_size(vec2(32.0, theme::TARGET)).selected(selected)));
    let color = if !enabled {
        ui.visuals().widgets.noninteractive.fg_stroke.color.gamma_multiply(0.6)
    } else {
        ui.style().interact_selectable(&resp, selected).fg_stroke.color
    };
    paint(ui.painter(), resp.rect.center(), 16.0, glyph, color);
    theme::name(resp, name)
}

/// Draw `glyph` in a `size` square around `c`.
pub fn paint(p: &egui::Painter, c: Pos2, size: f32, glyph: Glyph, color: Color32) {
    let s = size / 2.0;
    let w = (size * 0.09).max(1.3);
    let stroke = Stroke::new(w, color);
    let faint = theme::with_alpha(color, 0.35);
    let pt = |x: f32, y: f32| c + vec2(x * s, y * s);
    // A trim bracket at `x` (in half-sizes), opening right or left.
    let bracket = |x: f32, opens_right: bool| {
        let d = if opens_right { 0.35 } else { -0.35 };
        p.add(egui::Shape::line(vec![pt(x + d, -0.8), pt(x, -0.8), pt(x, 0.8), pt(x + d, 0.8)], stroke));
    };
    let block = |x0: f32, x1: f32, fill: Color32| p.rect_filled(Rect::from_min_max(pt(x0, -0.45), pt(x1, 0.45)), 1.5, fill);
    let dashed = |x: f32| {
        p.extend(egui::Shape::dashed_line(&[pt(x, -0.9), pt(x, 0.9)], Stroke::new(w * 0.8, color), 2.0, 2.0));
    };
    match glyph {
        Glyph::MarkerAdd => {
            p.line_segment([pt(-0.55, -0.85), pt(-0.55, 0.85)], stroke);
            p.add(egui::Shape::convex_polygon(vec![pt(-0.55, -0.85), pt(0.25, -0.5), pt(-0.55, -0.15)], color, Stroke::NONE));
            p.line_segment([pt(0.25, 0.45), pt(0.95, 0.45)], stroke);
            p.line_segment([pt(0.6, 0.1), pt(0.6, 0.8)], stroke);
        }
        Glyph::StartHere => {
            dashed(-0.15);
            bracket(-0.15, true);
            block(0.35, 0.95, faint);
        }
        Glyph::EndHere => {
            dashed(0.15);
            bracket(0.15, false);
            block(-0.95, -0.35, faint);
        }
        Glyph::TrimSelection => {
            block(-0.45, 0.45, faint);
            bracket(-0.75, true);
            bracket(0.75, false);
        }
        Glyph::VampSelection => {
            block(-0.9, 0.9, faint);
            super::pad_options::paint_icon(p, c, size * 0.95, super::pad_options::PadOption::Loop, color);
        }
        Glyph::PlaySelection => {
            bracket(-0.8, true);
            bracket(0.8, false);
            p.add(egui::Shape::convex_polygon(vec![pt(-0.3, -0.5), pt(0.45, 0.0), pt(-0.3, 0.5)], color, Stroke::NONE));
        }
        Glyph::Stop => {
            p.rect_filled(Rect::from_center_size(c, vec2(s * 1.1, s * 1.1)), 1.5, color);
        }
        Glyph::ClearSelection => {
            block(-0.9, 0.9, faint);
            p.line_segment([pt(-0.5, -0.5), pt(0.5, 0.5)], stroke);
            p.line_segment([pt(-0.5, 0.5), pt(0.5, -0.5)], stroke);
        }
        Glyph::StartAtPlayhead => {
            block(-0.25, 0.95, faint);
            p.line_segment([pt(-0.25, -0.45), pt(-0.25, 0.45)], stroke);
            p.line_segment([pt(-0.55, -0.95), pt(-0.55, 0.95)], Stroke::new(w * 1.2, color));
            p.add(egui::Shape::convex_polygon(vec![pt(-0.85, -0.95), pt(-0.25, -0.95), pt(-0.55, -0.6)], color, Stroke::NONE));
        }
        Glyph::JoinNext => {
            block(-0.95, -0.45, faint);
            block(0.45, 0.95, faint);
            p.add(egui::Shape::convex_polygon(vec![pt(-0.4, -0.35), pt(-0.05, 0.0), pt(-0.4, 0.35)], color, Stroke::NONE));
            p.add(egui::Shape::convex_polygon(vec![pt(0.4, -0.35), pt(0.05, 0.0), pt(0.4, 0.35)], color, Stroke::NONE));
        }
        Glyph::Reset => {
            // Most of a circle, anticlockwise, with its arrowhead at the top left.
            let r = s * 0.7;
            let (from, to) = (0.05 * TAU, -0.7 * TAU);
            let at = |a: f32| c + r * vec2(a.cos(), a.sin());
            let points: Vec<Pos2> = (0..=24).map(|i| at(from + (to - from) * i as f32 / 24.0)).collect();
            p.add(egui::Shape::line(points, stroke));
            let tip = at(to);
            let along = -vec2(-to.sin(), to.cos());
            let out = vec2(to.cos(), to.sin());
            let head = s * 0.4;
            p.add(egui::Shape::convex_polygon(vec![tip + along * head * 0.9, tip + out * head * 0.75, tip - out * head * 0.75], color, Stroke::NONE));
        }
        Glyph::SplitAgain => {
            for x in [-0.95, -0.3, 0.35] {
                let r = Rect::from_min_max(pt(x, -0.45), pt(x + 0.55, 0.45));
                let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                p.extend(egui::Shape::dashed_line(&corners, Stroke::new(w * 0.8, color), 2.0, 1.5));
            }
        }
        Glyph::Transcribe => {
            for (x, h) in [(-0.85, 0.35), (-0.6, 0.75), (-0.35, 0.5)] {
                p.line_segment([pt(x, -h), pt(x, h)], stroke);
            }
            p.add(egui::Shape::convex_polygon(vec![pt(-0.15, -0.3), pt(0.15, 0.0), pt(-0.15, 0.3)], color, Stroke::NONE));
            p.line_segment([pt(0.3, -0.35), pt(0.95, -0.35)], stroke);
            p.line_segment([pt(0.3, 0.35), pt(0.75, 0.35)], stroke);
        }
        Glyph::AlignWords => {
            let top = |x0: f32, x1: f32| p.rect_filled(Rect::from_min_max(pt(x0, -0.75), pt(x1, -0.05)), 1.5, faint);
            top(-0.85, -0.1);
            top(0.15, 0.85);
            for x in [-0.85, 0.15] {
                p.line_segment([pt(x, -0.75), pt(x, 0.9)], stroke);
            }
            p.line_segment([pt(-0.95, 0.9), pt(0.95, 0.9)], Stroke::new(w * 0.7, faint));
        }
        Glyph::AddLine | Glyph::DeleteLine => {
            // The lane's shapes: a line's bar over two of its words, as the lane draws them.
            p.rect_stroke(Rect::from_min_max(pt(-0.95, -0.85), pt(0.35, -0.4)), 1.5, Stroke::new(w * 0.8, color), egui::StrokeKind::Inside);
            p.rect_filled(Rect::from_min_max(pt(-0.95, -0.2), pt(-0.35, 0.35)), 1.5, faint);
            p.rect_filled(Rect::from_min_max(pt(-0.25, -0.2), pt(0.35, 0.35)), 1.5, faint);
            let (cx, cy, r) = (0.6, 0.55, 0.38);
            p.line_segment([pt(cx - r, cy), pt(cx + r, cy)], Stroke::new(w * 1.3, color));
            if glyph == Glyph::AddLine {
                p.line_segment([pt(cx, cy - r), pt(cx, cy + r)], Stroke::new(w * 1.3, color));
            }
        }
        Glyph::GripStart | Glyph::GripEnd | Glyph::GripWord => {
            let r = Rect::from_min_max(pt(-0.7, -0.5), pt(0.7, 0.5));
            p.rect_stroke(r, 1.5, Stroke::new(w * 0.7, faint), egui::StrokeKind::Inside);
            let bold = Stroke::new(w * 1.8, color);
            match glyph {
                Glyph::GripStart => {
                    p.line_segment([pt(-0.7, -0.95), pt(-0.7, 0.95)], bold);
                }
                Glyph::GripEnd => {
                    p.line_segment([pt(0.7, -0.95), pt(0.7, 0.95)], bold);
                }
                _ => {
                    p.rect_filled(r, 1.5, faint);
                    p.line_segment([pt(-0.4, 0.0), pt(0.4, 0.0)], stroke);
                    for (x, d) in [(-0.4, 1.0), (0.4, -1.0)] {
                        p.add(egui::Shape::convex_polygon(vec![pt(x - 0.2 * d, 0.0), pt(x + 0.1 * d, -0.25), pt(x + 0.1 * d, 0.25)], color, Stroke::NONE));
                    }
                }
            }
        }
    }
}
