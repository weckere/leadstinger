//! The grid's geometry (D-114..D-116, D-140, D-141): where each slot goes in Rows, Auto columns
//! and Fixed columns, where a dragged pad or files would land, and the board while
//! they're dragged. Pure layout, so it's tested without a window.

use crate::rows::{self, Grid, Place};
use eframe::egui::{pos2, vec2, Pos2, Rect};
use serde_json::{json, Value};

/// The space between rows and between tiles in a line.
pub const GAP: f32 = 10.0;
/// The narrowest tiles get in Auto columns (D-115); Fixed columns keeps the wider
/// minimums it always had.
pub const ROW_MIN_WIDTH: f32 = 96.0;
pub const PANEL_ROW_MIN_WIDTH: f32 = 120.0;
/// The smallest pads get in Rows, in both looks, before the board scrolls sideways
/// (D-170, D-176, D-217): about half the old 120 px. A pad this narrow starts its title
/// under the key cap and runs it on down the pad, a size smaller when a word wouldn't
/// fit; the key cap and the time still read.
pub const PAD_MIN_WIDTH: f32 = 64.0;

/// A cell of the grid: a slot (a tile, a blank, a ghost) or the add slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    /// The row it's in and its slot there.
    pub row: usize,
    pub at: usize,
    pub rect: Rect,
    /// The add slot, after the last row's slots.
    pub add: bool,
}

/// A row's extent on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub top: f32,
    pub bottom: f32,
    /// The height of its tiles (they're all the same).
    pub tile_h: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub cells: Vec<Cell>,
    pub bands: Vec<Band>,
    /// The left edge and width the grid lays out in (wider than the board when its
    /// widest row doesn't fit at the smallest pads).
    pub left: f32,
    pub width: f32,
    pub bottom: f32,
}

/// The widths and heights tiles may have.
#[derive(Clone, Copy)]
pub struct Sizing {
    pub min_w: f32,
    pub height: fn(f32) -> f32,
    /// The shortest rows get when they share the board's height (D-140).
    pub min_h: f32,
}

/// The width of each of `k` tiles sharing `avail`.
fn share(avail: f32, k: usize) -> f32 {
    ((avail - GAP * (k as f32 - 1.0)) / k as f32).floor()
}

/// How many tiles at least `min_w` wide fit across `avail`.
pub fn fit(avail: f32, min_w: f32) -> usize {
    ((avail + GAP) / (min_w + GAP)).floor().max(1.0) as usize
}

/// Lay out rows of `slots` (slot counts, top to bottom) from `origin`, `avail` wide. With
/// `add_slot`, the add slot is the next cell after the last row's slots, by the fill
/// rule. A row is never broken onto a second line (D-170): every slot in the set is one
/// width, the row size's (`columns` across `avail`), or narrower so the widest row fits,
/// never under `min_w`; past that the layout is wider than `avail` and the board scrolls
/// sideways. With `fill` (the board's height), the rows share it, down to `min_h`, then
/// the board scrolls (D-140); an add slot on a line of its own keeps `min_h`. Without
/// it, every slot is as tall as one of the row size's width (D-117).
pub fn rows(origin: Pos2, slots: &[usize], add_slot: bool, columns: usize, avail: f32, s: Sizing, fill: Option<f32>) -> Layout {
    let standard = share(avail, columns.max(1)).max(s.min_w);
    let mut rows = slots.to_vec();
    let add_row = match rows.last() {
        Some(&k) if add_slot && k < columns.max(1) => Some(rows.len() - 1),
        _ if add_slot => {
            rows.push(0);
            Some(rows.len() - 1)
        }
        _ => None,
    };
    // An add slot alone on its line doesn't take a share of the height.
    let add_alone = add_row.is_some() && rows.last() == Some(&0);
    let cells_of = |r: usize, k: usize| k + usize::from(add_row == Some(r));
    let widest = rows.iter().enumerate().map(|(r, &k)| cells_of(r, k)).max().unwrap_or(1);
    let w = standard.min(share(avail, widest.max(1))).max(s.min_w);
    let h = match fill {
        Some(fill) => {
            let shared = rows.len() - usize::from(add_alone);
            let spare = fill - shared.saturating_sub(1) as f32 * GAP - if add_alone { GAP + s.min_h } else { 0.0 };
            (spare / shared.max(1) as f32).floor().max(s.min_h)
        }
        None => (s.height)(standard),
    };
    let mut out = Layout { left: origin.x, width: avail.max(widest as f32 * (w + GAP) - GAP), ..Default::default() };
    let mut y = origin.y;
    for (r, &k) in rows.iter().enumerate() {
        let line_h = if add_alone && r == rows.len() - 1 && fill.is_some() { s.min_h } else { h };
        for c in 0..cells_of(r, k) {
            let x = origin.x + c as f32 * (w + GAP);
            out.cells.push(Cell { row: r, at: c, rect: Rect::from_min_size(pos2(x, y), vec2(w, line_h)), add: c >= k });
        }
        out.bands.push(Band { top: y, bottom: y + line_h, tile_h: line_h });
        y += line_h + GAP;
    }
    out.bottom = (y - GAP).max(origin.y);
    out
}

/// The tile counts of a grid of `n` cells in lines of `columns`.
pub fn uniform(n: usize, columns: usize) -> Vec<usize> {
    let c = columns.max(1);
    (0..n.div_ceil(c)).map(|r| c.min(n - r * c)).collect()
}

/// Auto columns (D-114): the fewest columns that show `n` cells without scrolling in
/// `avail` × `height`, or as many as fit at `min_w` when nothing does.
pub fn auto_columns(n: usize, avail: f32, height: f32, s: Sizing) -> usize {
    let cap = fit(avail, s.min_w);
    (1..=cap)
        .find(|&c| {
            let lines = n.div_ceil(c) as f32;
            lines * (s.height)(share(avail, c)) + (lines - 1.0).max(0.0) * GAP <= height
        })
        .unwrap_or(cap)
}

/// The top and bottom part of a row's slots that count as "a new row here".
fn edge(tile_h: f32) -> f32 {
    (tile_h * 0.2).min(14.0)
}

/// Where a pad or files dropped at `p` go (D-141), with the grid as it is now: a gap
/// before, between or after the pads of a row (`InRow`), a blank slot (`Slot`), or a
/// phantom row (`NewRow`) between rows, below the last, or past a row's left or right
/// edge. A pad is never the target. `rows` is the grid's row count (the add slot may
/// have a row of its own below them); `blank` says whether a slot is blank.
pub fn target_at(l: &Layout, rows: usize, blank: impl Fn(usize, usize) -> bool, p: Pos2) -> Place {
    let Some(first) = l.bands.first() else { return Place::NewRow(0) };
    if p.y < first.top + edge(first.tile_h) {
        return Place::NewRow(0);
    }
    for (r, b) in l.bands.iter().enumerate() {
        if r >= rows {
            break;
        }
        let next = l.bands.get(r + 1);
        let below = next.map_or(f32::INFINITY, |n| n.top + edge(n.tile_h));
        // The rows fill the board, so the last row's bottom edge is wider: it's where
        // "below the last row" is.
        let bottom = if r + 1 == rows { (b.tile_h * 0.25).min(24.0) } else { edge(b.tile_h) };
        if p.y > b.bottom - bottom && p.y < below {
            return Place::NewRow(r + 1);
        }
        if p.y <= b.bottom - bottom {
            if p.x < l.left {
                return Place::NewRow(r);
            }
            if p.x > l.left + l.width {
                return Place::NewRow(r + 1);
            }
            return in_row(l, r, &blank, p);
        }
    }
    Place::NewRow(rows)
}

/// A drop inside row `r`: a gap beside the pad under it, the blank slot, or an empty
/// slot past the row's end (the add slot's too), as the slot under the pointer
/// (D-190).
fn in_row(l: &Layout, r: usize, blank: &impl Fn(usize, usize) -> bool, p: Pos2) -> Place {
    let cells: Vec<&Cell> = l.cells.iter().filter(|c| c.row == r && !c.add).collect();
    let end = cells.last().map_or(0, |c| c.at + 1);
    let pitch = l.cells.first().map_or(0.0, |c| c.rect.width() + GAP);
    if pitch > 0.0 && p.x >= l.left + end as f32 * pitch {
        // The empty slot under the pointer, with the gap after it.
        return Place::Slot { row: r, at: ((p.x - l.left) / pitch).floor() as usize };
    }
    for c in &cells {
        let rect = c.rect;
        if blank(r, c.at) {
            if p.x < rect.left() {
                return Place::InRow { row: r, at: c.at };
            }
            if p.x <= rect.right() {
                return Place::Slot { row: r, at: c.at };
            }
        } else if p.x < rect.center().x {
            return Place::InRow { row: r, at: c.at };
        }
    }
    Place::InRow { row: r, at: end }
}

/// Where a dragged pad goes: the place for `tile.move`, and whether a blank stays in
/// its old slot (`leaveBlank`, D-202).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub place: Place,
    pub leave_blank: bool,
}

/// A drop for the pad now in slot `at` of `row` (a row of `slots` slots), or `None`
/// when it would leave the pad where it is. A gap between pads of its own row closes
/// the row up (`SoundSet::move_tile` numbers slots with the pad already out). A blank
/// of its own row, an empty slot past the row's end, or the row's end itself (past the
/// last pad's middle: a slot of its own after it, even when the row fills the board)
/// leaves a blank where the pad was, so nothing else moves (D-190, D-202).
pub fn move_place(drop: Place, row: usize, at: usize, slots: usize) -> Option<Move> {
    let close_up = |place| Move { place, leave_blank: false };
    let leave_blank = |s: usize| (s != at).then_some(Move { place: Place::Slot { row, at: s }, leave_blank: true });
    match drop {
        Place::InRow { row: r, at: gap } if r == row && gap >= slots => leave_blank(slots),
        Place::InRow { row: r, at: gap } if r == row => {
            let to = if gap > at { gap - 1 } else { gap };
            (to != at).then_some(close_up(Place::InRow { row, at: to }))
        }
        Place::Slot { row: r, at: s } if r == row => leave_blank(s),
        // Alone in its row, a new row just above or below it is the same place.
        Place::NewRow(r) if slots == 1 && (r == row || r == row + 1) => None,
        p => Some(close_up(p)),
    }
}

/// Where several pads dragged together go (D-219), the pads `items` of `grid` dropped
/// at `drop` (as [`target_at`] gives it, on the board as it is): the place for
/// `tile.move` with `tiles`, numbered with them all taken out, or `None` when they'd
/// end up where they are. They go as one run in the set's order and their rows close
/// up behind them (no blanks stay, unlike one pad, D-202): a gap or a blank keeps its
/// place among the pads that stay; an empty slot past a row's end keeps its column, so
/// the run starts under the pointer (D-190).
pub fn run_place<T: PartialEq + Clone>(grid: &Grid<T>, items: &[T], drop: Place) -> Option<Place> {
    let ours = |row: usize, at: usize| grid.get(row).map_or(0, |r| r.iter().take(at).flatten().filter(|x| items.contains(x)).count());
    let len = |row: usize| grid.get(row).map_or(0, Vec::len);
    let place = match drop {
        Place::InRow { row, at } => Place::InRow { row, at: at - ours(row, at) },
        Place::Slot { row, at } if at < len(row) => Place::Slot { row, at: at - ours(row, at) },
        p => p,
    };
    let mut after = grid.clone();
    for i in items {
        rows::take(&mut after, i);
    }
    rows::put(&mut after, place, items.iter().cloned().map(Some).collect(), 1);
    rows::prune(&mut after);
    let mut before = grid.clone();
    rows::prune(&mut before);
    (after != before).then_some(place)
}

/// Where the pads `items` dragged together go: [`move_place`] for one pad,
/// [`run_place`] for several.
pub fn drag_move<T: PartialEq + Clone>(grid: &Grid<T>, items: &[T], drop: Place) -> Option<Move> {
    match items {
        [one] => {
            let (row, at) = rows::find(grid, one)?;
            move_place(drop, row, at, grid[row].len())
        }
        _ => run_place(grid, items, drop).map(|place| Move { place, leave_blank: false }),
    }
}

/// What a slot of the drag preview shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    /// The tile at this flat index.
    Pad(usize),
    /// Where a dragged pad, or one of the dropped files, goes.
    Ghost,
}

/// The board while the pads `from` (flat, in set order: one, or the selection dragged
/// together, D-219) or `files` files are dragged to `place` (as [`target_at`] gives it;
/// `None` off the grid): the result of the drop, with a ghost for each pad or file
/// dropped, and the dragged pads' rows kept even if they empty, so the rows under the
/// pointer stay put until the drop.
pub fn preview(base: &Grid<usize>, from: &[usize], files: usize, place: Option<Place>, columns: usize) -> Grid<Show> {
    let mut g: Grid<Show> = base.iter().map(|r| r.iter().map(|s| s.map(Show::Pad)).collect()).collect();
    let Some(place) = place else { return g };
    let items: Vec<Show> = from.iter().map(|&f| Show::Pad(f)).filter(|p| rows::find(&g, p).is_some()).collect();
    if !items.is_empty() {
        match drag_move(&g, &items, place) {
            None => {
                for p in &items {
                    if let Some((row, at)) = rows::find(&g, p) {
                        g[row][at] = Some(Show::Ghost);
                    }
                }
            }
            Some(m) => {
                for p in &items {
                    match rows::find(&g, p) {
                        Some((row, at)) if m.leave_blank => g[row][at] = None,
                        _ => {
                            rows::take(&mut g, p);
                        }
                    }
                }
                rows::put(&mut g, m.place, vec![Some(Show::Ghost); items.len()], columns);
            }
        }
    } else if files > 0 {
        rows::put(&mut g, place, vec![Some(Show::Ghost); files], columns);
    }
    g
}

/// tile.add's and tile.move's parameters for a place (1-based, D-116, D-143).
pub fn place_json(place: Place) -> Value {
    match place {
        Place::Append => json!({}),
        Place::Flat(i) => json!({"at": i + 1}),
        Place::InRow { row, at } => json!({"row": row + 1, "at": at.saturating_add(1)}),
        Place::Slot { row, at } => json!({"row": row + 1, "slot": at.saturating_add(1)}),
        Place::NewRow(row) => json!({"row": row + 1, "newRow": true}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console(w: f32) -> f32 {
        (w / 1.5).clamp(58.0, 170.0)
    }
    const S: Sizing = Sizing { min_w: ROW_MIN_WIDTH, height: console, min_h: 72.0 };

    fn widths(l: &Layout, row: usize) -> Vec<f32> {
        l.cells.iter().filter(|c| c.row == row).map(|c| c.rect.width()).collect()
    }

    #[test]
    fn rows_of_the_row_size_look_like_the_old_columns() {
        // 12 tiles in rows of 5 with the add slot, 1000 px: every cell 192 px, as before.
        let l = rows(Pos2::ZERO, &[5, 5, 2], true, 5, 1000.0, S, None);
        assert!(l.cells.iter().all(|c| c.rect.width() == 192.0), "{:?}", l.cells);
        assert_eq!(l.cells.len(), 13);
        let add = l.cells.last().unwrap();
        assert_eq!((add.add, add.row, add.at), (true, 2, 2), "the add slot is the next cell");
        assert_eq!(l.bands[1].top, 128.0 + GAP);
        // A full last row: the add slot starts a new line, still column-sized.
        let l = rows(Pos2::ZERO, &[5], true, 5, 1000.0, S, None);
        assert_eq!((l.cells[5].row, l.cells[5].rect.width()), (1, 192.0));
    }

    #[test]
    fn every_pad_shrinks_together_so_the_widest_row_fits() {
        // D-170: 8 in a row narrows every slot of the set to 116 px, not just that row's.
        let l = rows(Pos2::ZERO, &[3, 8], false, 5, 1000.0, S, None);
        assert_eq!(widths(&l, 0), vec![116.0; 3], "a short row shrinks with it");
        assert_eq!(widths(&l, 1), vec![116.0; 8], "8 share the width");
        assert_eq!(l.width, 1000.0, "it fits: nothing scrolls");
        assert_eq!((l.bands[0].tile_h, l.bands[1].tile_h), (128.0, 128.0), "narrower tiles keep the height (D-117)");
        // Every row's slots line up in columns.
        let x = |r: usize| l.cells.iter().filter(|c| c.row == r).map(|c| c.rect.left()).collect::<Vec<_>>();
        assert_eq!(x(0), x(1)[..3]);
        // Row size 3: big pads until a row holds more, then the same shrink.
        let l = rows(Pos2::ZERO, &[3, 3], false, 3, 1000.0, S, None);
        assert_eq!(widths(&l, 0), vec![326.0; 3]);
        assert_eq!(l.bands[0].tile_h, 170.0, "height stops at the limit");
        let l = rows(Pos2::ZERO, &[3, 8], false, 3, 1000.0, S, None);
        assert_eq!(widths(&l, 0), vec![116.0; 3]);
        // A short last row with the add slot doesn't make anything narrower.
        let l = rows(Pos2::ZERO, &[5, 4], true, 5, 1000.0, S, None);
        assert!(l.cells.iter().all(|c| c.rect.width() == 192.0));
    }

    #[test]
    fn rows_share_the_boards_height_down_to_a_minimum() {
        // Two rows in 600 px: 295 px each.
        let l = rows(Pos2::ZERO, &[5, 3], false, 5, 1000.0, S, Some(600.0));
        assert_eq!((l.bands[0].tile_h, l.bands[1].tile_h, l.bottom), (295.0, 295.0, 600.0));
        // Six rows: shorter ones; twenty: the minimum, and the board scrolls.
        let l = rows(Pos2::ZERO, &[5; 6], false, 5, 1000.0, S, Some(600.0));
        assert_eq!(l.bands[0].tile_h, 91.0);
        let l = rows(Pos2::ZERO, &[5; 20], false, 5, 1000.0, S, Some(600.0));
        assert_eq!(l.bands[0].tile_h, 72.0);
        assert!(l.bottom > 600.0);
        // The add slot on a line of its own stays short and leaves the rows the rest.
        let l = rows(Pos2::ZERO, &[5], true, 5, 1000.0, S, Some(600.0));
        assert_eq!((l.bands[0].tile_h, l.bands[1].tile_h, l.bottom), (518.0, 72.0, 600.0));
        // A row too wide for the board is still one row, with one share.
        let l = rows(Pos2::ZERO, &[8], false, 5, 470.0, S, Some(600.0));
        assert_eq!((l.bands.len(), l.bands[0].tile_h), (1, 600.0));
    }

    #[test]
    fn a_row_too_full_for_the_minimum_never_wraps_and_the_board_scrolls_sideways() {
        // 720 px window: about 470 px of board, room for 6 pads of 64 px. 8 stay in one
        // row at the minimum, and the layout is wider than the board.
        let r = Sizing { min_w: PAD_MIN_WIDTH, ..S };
        let l = rows(Pos2::ZERO, &[3, 8], false, 5, 470.0, r, None);
        assert!(l.cells.iter().filter(|c| c.row == 1).all(|c| c.rect.top() == l.bands[1].top), "one line");
        assert!(l.cells.iter().all(|c| c.rect.width() == PAD_MIN_WIDTH), "every pad at the minimum");
        assert_eq!(l.width, 8.0 * (PAD_MIN_WIDTH + GAP) - GAP);
        assert_eq!(l.cells.last().unwrap().rect.right(), l.width);
        // The row size alone can be too wide: 7 of 64 px don't fit 470 px either.
        let l = rows(Pos2::ZERO, &[7], false, 7, 470.0, r, None);
        assert_eq!((l.cells.len(), l.cells[6].rect.top(), l.width), (7, 0.0, 7.0 * (PAD_MIN_WIDTH + GAP) - GAP));
        // Six still share it, narrower than the old 120 px minimum (D-217).
        let l = rows(Pos2::ZERO, &[6], false, 6, 470.0, r, None);
        assert_eq!((l.cells[0].rect.width(), l.width), (70.0, 470.0));
        // Just above the minimum, everything still fits: 3 at 150 px.
        let l = rows(Pos2::ZERO, &[3, 3], false, 3, 470.0, r, None);
        assert_eq!((l.cells[0].rect.width(), l.width), (150.0, 470.0));
        // An empty row (the dragged pad's, in a preview) still has its band.
        let l = rows(Pos2::ZERO, &[3, 0, 2], false, 5, 1000.0, S, None);
        assert_eq!((l.bands.len(), l.cells.len()), (3, 5));
    }

    #[test]
    fn auto_columns_grow_until_everything_fits() {
        assert_eq!(auto_columns(4, 1000.0, 800.0, S), 1, "four 170 px rows fit");
        assert_eq!(auto_columns(13, 1000.0, 600.0, S), 5, "four columns are 675 px tall");
        let many = auto_columns(200, 1000.0, 600.0, S);
        assert_eq!(many, fit(1000.0, ROW_MIN_WIDTH), "then it scrolls at the minimum width");
        assert_eq!(uniform(7, 3), vec![3, 3, 1]);
    }

    #[test]
    fn drop_targets_are_gaps_blanks_and_phantom_rows() {
        // [a _ c] over [d e f g h i j k], 1000 px: slot 1 of row 0 is blank.
        let l = rows(Pos2::ZERO, &[3, 8], false, 5, 1000.0, S, None);
        let blank = |r: usize, at: usize| (r, at) == (0, 1);
        let (b0, b1) = (l.bands[0], l.bands[1]);
        let c = |i: usize| l.cells[i].rect;
        let at = |p: Pos2| target_at(&l, 2, blank, p);
        // Left half of a pad: the gap before it; right half: the gap after it.
        assert_eq!(at(pos2(c(0).left() + 10.0, c(0).center().y)), Place::InRow { row: 0, at: 0 });
        assert_eq!(at(pos2(c(0).right() - 10.0, c(0).center().y)), Place::InRow { row: 0, at: 1 });
        // Anywhere on the blank: the blank; the gap just before it is still a gap.
        assert_eq!(at(c(1).center()), Place::Slot { row: 0, at: 1 });
        assert_eq!(at(pos2(c(1).right() - 2.0, c(1).center().y)), Place::Slot { row: 0, at: 1 });
        assert_eq!(at(pos2(c(1).left() - 3.0, c(1).center().y)), Place::InRow { row: 0, at: 1 });
        // Just past the short row's last slot: the gap after it. Further, over an empty
        // slot past its end (the columns line up), that slot (D-190).
        let y0 = c(0).center().y;
        assert_eq!(at(pos2(c(2).right() + 3.0, y0)), Place::InRow { row: 0, at: 3 });
        assert_eq!(at(pos2(c(2).right() + GAP + 2.0, y0)), Place::Slot { row: 0, at: 3 });
        assert_eq!(at(pos2(900.0, y0)), Place::Slot { row: 0, at: 7 });
        // The gap between the rows, and the tiles' edges beside it: a phantom row there.
        assert_eq!(at(pos2(300.0, b0.bottom + 5.0)), Place::NewRow(1));
        assert_eq!(at(pos2(300.0, b0.bottom - 3.0)), Place::NewRow(1));
        assert_eq!(at(pos2(300.0, b1.top + 3.0)), Place::NewRow(1));
        assert_eq!(at(pos2(300.0, 2.0)), Place::NewRow(0));
        assert_eq!(at(pos2(300.0, b1.bottom + 40.0)), Place::NewRow(2));
        // Past a row's left or right edge: a phantom row above or below it.
        assert_eq!(at(pos2(-4.0, b1.top + 50.0)), Place::NewRow(1));
        assert_eq!(at(pos2(1004.0, b1.top + 50.0)), Place::NewRow(2));
        // A pad's middle is never a target for files either.
        assert_eq!(at(c(4).center()), Place::InRow { row: 1, at: 2 });
        // A row wider than the board: its slots past the board's edge are targets, and
        // past its own right edge is a phantom row.
        let w = rows(Pos2::ZERO, &[12], false, 5, 470.0, Sizing { min_w: PAD_MIN_WIDTH, ..S }, None);
        let eighth = w.cells[7].rect;
        assert!(eighth.left() > 470.0);
        assert_eq!(target_at(&w, 1, |_, _| false, pos2(eighth.left() + 5.0, eighth.center().y)), Place::InRow { row: 0, at: 7 });
        assert_eq!(target_at(&w, 1, |_, _| false, pos2(eighth.right() - 5.0, eighth.center().y)), Place::InRow { row: 0, at: 8 });
        assert_eq!(target_at(&w, 1, |_, _| false, pos2(w.width + 4.0, eighth.center().y)), Place::NewRow(1));
        // The add slot's own line, below a full row: a new last row.
        let w = rows(Pos2::ZERO, &[5], true, 5, 1000.0, S, None);
        assert_eq!(target_at(&w, 1, |_, _| false, w.cells[5].rect.center()), Place::NewRow(1));
        // The add slot beside a short last row is the empty slot it stands in.
        let w = rows(Pos2::ZERO, &[5, 2], true, 5, 1000.0, S, None);
        let add = w.cells.last().unwrap();
        assert!(add.add);
        assert_eq!(target_at(&w, 2, |_, _| false, pos2(add.rect.left() + 5.0, add.rect.center().y)), Place::Slot { row: 1, at: 2 });
        assert_eq!(target_at(&w, 2, |_, _| false, pos2(add.rect.right() - 5.0, add.rect.center().y)), Place::Slot { row: 1, at: 2 });
    }

    #[test]
    fn moves_that_go_nowhere_are_dropped() {
        let close_up = |place| Some(Move { place, leave_blank: false });
        let leave_blank = |row, at| Some(Move { place: Place::Slot { row, at }, leave_blank: true });
        // The pad in slot 2 of a 4-slot row 1.
        assert_eq!(move_place(Place::InRow { row: 1, at: 2 }, 1, 2, 4), None, "before itself");
        assert_eq!(move_place(Place::InRow { row: 1, at: 3 }, 1, 2, 4), None, "after itself");
        assert_eq!(move_place(Place::InRow { row: 1, at: 0 }, 1, 2, 4), close_up(Place::InRow { row: 1, at: 0 }));
        // The row's end (past the last pad's middle): a new slot after it, and a blank
        // where it was, not a swap with the last pad (D-202).
        assert_eq!(move_place(Place::InRow { row: 1, at: 4 }, 1, 2, 4), leave_blank(1, 4));
        assert_eq!(move_place(Place::InRow { row: 1, at: 4 }, 1, 3, 4), leave_blank(1, 4), "the last pad, one slot right");
        // A blank of its row, or an empty slot past the row's end: that slot, and a
        // blank where it was (D-190, D-202).
        assert_eq!(move_place(Place::Slot { row: 1, at: 3 }, 1, 2, 4), leave_blank(1, 3), "a blank after it");
        assert_eq!(move_place(Place::Slot { row: 1, at: 4 }, 1, 3, 4), leave_blank(1, 4), "its own row's last pad");
        assert_eq!(move_place(Place::Slot { row: 1, at: 6 }, 1, 0, 4), leave_blank(1, 6));
        // From another row: as before, its old row closes up.
        assert_eq!(move_place(Place::InRow { row: 0, at: 3 }, 1, 0, 4), close_up(Place::InRow { row: 0, at: 3 }));
        assert_eq!(move_place(Place::Slot { row: 0, at: 5 }, 1, 0, 4), close_up(Place::Slot { row: 0, at: 5 }));
        assert_eq!(move_place(Place::NewRow(2), 1, 0, 1), None);
        assert_eq!(move_place(Place::NewRow(2), 1, 0, 2), close_up(Place::NewRow(2)));
        assert_eq!(place_json(Place::NewRow(0)), json!({"row": 1, "newRow": true}));
        assert_eq!(place_json(Place::Slot { row: 0, at: 1 }), json!({"row": 1, "slot": 2}));
    }

    #[test]
    fn a_full_rows_end_is_a_target_for_its_own_pads() {
        // D-202: every row is 4 wide and the pads fill the board, so there's no empty
        // slot drawn after a row's last pad. Its right half is the row's end.
        let l = rows(Pos2::ZERO, &[4, 4], false, 4, 1000.0, S, None);
        let d = l.cells[3].rect;
        assert!(d.right() > 1000.0 - GAP, "the row fills the board: {d:?}");
        let end = target_at(&l, 2, |_, _| false, pos2(d.right() - 5.0, d.center().y));
        assert_eq!(end, Place::InRow { row: 0, at: 4 });
        // C there: [A B _ D C]; D itself: [A B C _ D]. Between C and D: a swap.
        assert_eq!(move_place(end, 0, 2, 4).map(|m| m.place), Some(Place::Slot { row: 0, at: 4 }));
        assert_eq!(move_place(end, 0, 3, 4).map(|m| m.place), Some(Place::Slot { row: 0, at: 4 }));
        let between = target_at(&l, 2, |_, _| false, pos2(d.left() + 5.0, d.center().y));
        assert_eq!(move_place(between, 0, 1, 4), Some(Move { place: Place::InRow { row: 0, at: 2 }, leave_blank: false }));
        // A pad of the row below dropped there joins the row's end, as before.
        assert_eq!(move_place(end, 1, 0, 4), Some(Move { place: Place::InRow { row: 0, at: 4 }, leave_blank: false }));
        // The same in a row shorter than the widest: the last pad's right half, the gap
        // after it and the empty slot drawn there all give the slot after it.
        let l = rows(Pos2::ZERO, &[3, 5], false, 5, 1000.0, S, None);
        let c = l.cells[2].rect;
        for x in [c.right() - 5.0, c.right() + 3.0, c.right() + GAP + 5.0] {
            let p = target_at(&l, 2, |_, _| false, pos2(x, c.center().y));
            assert_eq!(move_place(p, 0, 1, 3), Some(Move { place: Place::Slot { row: 0, at: 3 }, leave_blank: true }), "at x = {x}");
        }
    }

    #[test]
    fn the_preview_is_the_drop_with_ghosts() {
        use Show::{Ghost, Pad};
        // [0 1 2] [3] [_ 4]
        let base: Grid<usize> = vec![vec![Some(0), Some(1), Some(2)], vec![Some(3)], vec![None, Some(4)]];
        // A pad into a gap of another row: its row closes up, the gap opens.
        let g = preview(&base, &[0], 0, Some(Place::InRow { row: 2, at: 2 }), 5);
        assert_eq!(g, vec![vec![Some(Pad(1)), Some(Pad(2))], vec![Some(Pad(3))], vec![None, Some(Pad(4)), Some(Ghost)]]);
        // Into the blank: nothing else moves there.
        let g = preview(&base, &[1], 0, Some(Place::Slot { row: 2, at: 0 }), 5);
        assert_eq!(g[2], vec![Some(Ghost), Some(Pad(4))]);
        // The dragged pad's row stays while it's dragged out, even when it empties.
        let g = preview(&base, &[3], 0, Some(Place::NewRow(3)), 5);
        assert_eq!((g[1].len(), g[3].clone()), (0, vec![Some(Ghost)]));
        // Where it is, or off the grid: a ghost in its own slot / the board as it is.
        let g = preview(&base, &[1], 0, Some(Place::InRow { row: 0, at: 2 }), 5);
        assert_eq!(g[0], vec![Some(Pad(0)), Some(Ghost), Some(Pad(2))]);
        assert_eq!(preview(&base, &[1], 0, None, 5)[0][1], Some(Pad(1)));
        // A row's last pad dragged into the empty slot after it: a blank where it was.
        let g = preview(&base, &[2], 0, Some(Place::Slot { row: 0, at: 3 }), 5);
        assert_eq!(g[0], vec![Some(Pad(0)), Some(Pad(1)), None, Some(Ghost)]);
        // Its row's end, for a pad before the last: a new slot, a blank where it was
        // (D-202); the last pad past its own middle: one slot right.
        let g = preview(&base, &[1], 0, Some(Place::InRow { row: 0, at: 3 }), 5);
        assert_eq!(g[0], vec![Some(Pad(0)), None, Some(Pad(2)), Some(Ghost)]);
        let g = preview(&base, &[2], 0, Some(Place::InRow { row: 0, at: 3 }), 5);
        assert_eq!(g[0], vec![Some(Pad(0)), Some(Pad(1)), None, Some(Ghost)]);
        // A blank of its own row: the pad and the blank change places.
        let g = preview(&base, &[4], 0, Some(Place::Slot { row: 2, at: 0 }), 5);
        assert_eq!(g[2], vec![Some(Ghost), None]);
        // Between two pads of its row: still a close-up move.
        let g = preview(&base, &[0], 0, Some(Place::InRow { row: 0, at: 2 }), 5);
        assert_eq!(g[0], vec![Some(Pad(1)), Some(Ghost), Some(Pad(2))]);
        // Three files after pad 0: three ghosts, the row grows.
        let g = preview(&base, &[], 3, Some(Place::InRow { row: 0, at: 1 }), 5);
        assert_eq!(g[0], vec![Some(Pad(0)), Some(Ghost), Some(Ghost), Some(Ghost), Some(Pad(1)), Some(Pad(2))]);
    }

    #[test]
    fn several_pads_dragged_together_land_as_one_run() {
        use Show::{Ghost, Pad};
        // [0 1 2 3] [4] [_ 5]
        let base: Grid<usize> = vec![vec![Some(0), Some(1), Some(2), Some(3)], vec![Some(4)], vec![None, Some(5)]];
        // A gap is counted among the pads that stay: before 0 is the row's start.
        assert_eq!(run_place(&base, &[1, 4], Place::InRow { row: 0, at: 0 }), Some(Place::InRow { row: 0, at: 0 }));
        // After 3 (gap 4), with 1 and 2 out of the row: its end, slot 2 of [0 3].
        assert_eq!(run_place(&base, &[1, 2], Place::InRow { row: 0, at: 4 }), Some(Place::InRow { row: 0, at: 2 }));
        // Any gap inside or beside the run itself: nothing moves.
        for at in 1..=3 {
            assert_eq!(run_place(&base, &[1, 2], Place::InRow { row: 0, at }), None, "gap {at}");
        }
        // The blank keeps its place: the first fills it, the rest push along.
        assert_eq!(run_place(&base, &[0, 4], Place::Slot { row: 2, at: 0 }), Some(Place::Slot { row: 2, at: 0 }));
        // An empty slot past a row's end keeps its column, so the run starts there.
        assert_eq!(run_place(&base, &[2, 3], Place::Slot { row: 0, at: 5 }), Some(Place::Slot { row: 0, at: 5 }));
        // 4 and 5, the only pads of rows 1 and 2, as a new top row: a real move.
        assert_eq!(run_place(&base, &[4, 5], Place::NewRow(0)), Some(Place::NewRow(0)));
        // One pad keeps D-202's rule; several don't leave blanks.
        assert_eq!(drag_move(&base, &[1], Place::InRow { row: 0, at: 4 }), Some(Move { place: Place::Slot { row: 0, at: 4 }, leave_blank: true }));
        assert_eq!(drag_move(&base, &[0, 1], Place::InRow { row: 0, at: 4 }), Some(Move { place: Place::InRow { row: 0, at: 2 }, leave_blank: false }));

        // The preview: as many ghosts as pads, the pads they leave closed up.
        let g = preview(&base, &[1, 4], 0, Some(Place::InRow { row: 2, at: 2 }), 5);
        assert_eq!(g, vec![vec![Some(Pad(0)), Some(Pad(2)), Some(Pad(3))], vec![], vec![None, Some(Pad(5)), Some(Ghost), Some(Ghost)]]);
        // Past the row's end: blanks up to the pointer's column, then the run.
        let g = preview(&base, &[2, 3], 0, Some(Place::Slot { row: 0, at: 5 }), 5);
        assert_eq!(g[0], vec![Some(Pad(0)), Some(Pad(1)), None, None, None, Some(Ghost), Some(Ghost)]);
        // Where they are: a ghost in each one's own slot.
        let g = preview(&base, &[1, 2], 0, Some(Place::InRow { row: 0, at: 2 }), 5);
        assert_eq!(g[0], vec![Some(Pad(0)), Some(Ghost), Some(Ghost), Some(Pad(3))]);
    }
}
