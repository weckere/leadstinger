//! The slot grid (D-111, D-113, D-140). A set keeps its tiles in one list in reading order
//! (rows left to right, top to bottom, skipping blank slots), so the cue list, OSC and
//! MIDI numbering and hotkey reassignment never see rows or blanks. `SoundSet::rows` says
//! how many tiles (pads) each row holds and `SoundSet::blanks` where its blank slots are;
//! together they give the grid's shape, and the tiles fill its pad slots in order. Tiles
//! beyond the stored rows (appended by anything that pushes onto the list) fill the last
//! row up to the set's `columns` slots, then start new rows of `columns`, so empty `rows`
//! is exactly the old N-column wrap.

use crate::model::{SoundSet, Tile};

/// A grid of slots, row by row: `Some` a pad, `None` a blank slot.
pub type Grid<T> = Vec<Vec<Option<T>>>;

/// The furthest slot a place past a row's end opens blanks up to; beyond it the place
/// is the row's end.
const MAX_SLOT: usize = 256;

/// Where tiles (or blanks) go in a set's grid. Rows and slots are 0-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// After the last slot, by the fill rule (the last row up to `columns`, then new rows).
    Append,
    /// At this place in the flat (reading) order, just before the pad there and in its
    /// row (the last row's end past the last pad).
    Flat(usize),
    /// Into the gap before slot `at` of `row` (its end for `at` past its slots); the
    /// slots from there on move along. A row past the last starts a new last row.
    InRow { row: usize, at: usize },
    /// Into slot `at` of `row`: a blank slot is filled and nothing moves; past the row's
    /// end, blank slots fill the space up to it (D-190); otherwise as `InRow`.
    Slot { row: usize, at: usize },
    /// As a new row here, before the row that has this number now (the row count for a
    /// new last row).
    NewRow(usize),
}

/// The grid that `rows` (pads per row) and `blanks` ([row, slot] of each blank) give
/// `n` tiles, as flat indices, with the fill rule for tiles beyond them. A row whose
/// pads run out goes, with its blanks.
pub fn derive(rows: &[u32], blanks: &[[u32; 2]], n: usize, columns: u32) -> Grid<usize> {
    let cols = columns.max(1) as usize;
    let mut out: Grid<usize> = Vec::new();
    let mut next = 0;
    for (r, &k) in rows.iter().enumerate() {
        let k = (k as usize).min(n - next);
        if k == 0 {
            continue;
        }
        let mut here: Vec<usize> = blanks.iter().filter(|b| b[0] as usize == r).map(|b| b[1] as usize).collect();
        here.sort_unstable();
        here.dedup();
        let mut row = Vec::new();
        let (mut pads, mut b) = (0, 0);
        while pads < k {
            if here.get(b) == Some(&row.len()) {
                row.push(None);
                b += 1;
            } else {
                row.push(Some(next));
                next += 1;
                pads += 1;
            }
        }
        // Blanks after the row's last pad stay at its end.
        row.extend(std::iter::repeat_n(None, here.len() - b));
        out.push(row);
    }
    let rest: Vec<Option<usize>> = (next..n).map(Some).collect();
    put(&mut out, Place::Append, rest, cols);
    out
}

/// Put `items` (pads, or `None` for blanks) at `place`, in a grid of rows of `columns`
/// for `Append`.
pub fn put<T>(grid: &mut Grid<T>, place: Place, items: Vec<Option<T>>, columns: usize) {
    if items.is_empty() {
        return;
    }
    let cols = columns.max(1);
    match place {
        Place::Append => {
            let mut items = items.into_iter().peekable();
            if let Some(last) = grid.last_mut() {
                while last.len() < cols {
                    match items.next() {
                        Some(i) => last.push(i),
                        None => return,
                    }
                }
            }
            while items.peek().is_some() {
                grid.push(items.by_ref().take(cols).collect());
            }
        }
        Place::Flat(i) => {
            let mut seen = 0;
            for (r, row) in grid.iter().enumerate() {
                for (s, slot) in row.iter().enumerate() {
                    if slot.is_some() {
                        if seen == i {
                            return put(grid, Place::InRow { row: r, at: s }, items, cols);
                        }
                        seen += 1;
                    }
                }
            }
            let end = grid.len().saturating_sub(1);
            put(grid, Place::InRow { row: end, at: usize::MAX }, items, cols);
        }
        Place::InRow { row, at } if row < grid.len() => {
            let at = at.min(grid[row].len());
            grid[row].splice(at..at, items);
        }
        Place::Slot { row, at } if grid.get(row).and_then(|r| r.get(at)).is_some_and(|s| s.is_none()) => {
            let mut items = items.into_iter();
            grid[row][at] = items.next().flatten();
            grid[row].splice(at + 1..at + 1, items);
        }
        // Past the row's end: blanks up to the slot, then the items there (D-190).
        Place::Slot { row, at } if at < MAX_SLOT && grid.get(row).is_some_and(|r| at > r.len()) => {
            grid[row].resize_with(at, || None);
            grid[row].extend(items);
        }
        Place::Slot { row, at } => put(grid, Place::InRow { row, at }, items, cols),
        Place::InRow { .. } => grid.push(items),
        Place::NewRow(row) => {
            let row = row.min(grid.len());
            grid.insert(row, items);
        }
    }
}

/// Where `item` is: its row and slot.
pub fn find<T: PartialEq>(grid: &Grid<T>, item: &T) -> Option<(usize, usize)> {
    grid.iter().enumerate().find_map(|(r, row)| row.iter().position(|s| s.as_ref() == Some(item)).map(|s| (r, s)))
}

/// Take `item`'s slot out of its row (the slots after it move up one). Its row stays,
/// even with no pads left, so row numbers keep their meaning until [`prune`].
pub fn take<T: PartialEq>(grid: &mut Grid<T>, item: &T) -> Option<(usize, usize)> {
    let (r, s) = find(grid, item)?;
    grid[r].remove(s);
    Some((r, s))
}

/// Remove the rows with no pads left (their blanks go with them).
pub fn prune<T>(grid: &mut Grid<T>) {
    grid.retain(|row| row.iter().any(Option::is_some));
}

/// The pads in the rectangle of slots whose corners are `a` and `b` (row, slot), in
/// reading order (D-169): every row between them, the slots between their columns. Blank
/// slots, and slots a short row doesn't reach, are skipped.
pub fn rectangle<T: Clone>(grid: &Grid<T>, a: (usize, usize), b: (usize, usize)) -> Vec<T> {
    let ((r0, r1), (s0, s1)) = ((a.0.min(b.0), a.0.max(b.0)), (a.1.min(b.1), a.1.max(b.1)));
    grid.iter().take(r1 + 1).skip(r0).flat_map(|row| row.iter().take(s1 + 1).skip(s0).flatten().cloned()).collect()
}

/// How many slots each row has.
pub fn slot_counts<T>(grid: &Grid<T>) -> Vec<usize> {
    grid.iter().map(Vec::len).collect()
}

impl SoundSet {
    /// The grid as flat tile indices, `None` for blank slots. There's never a row
    /// without a pad.
    pub fn slots(&self) -> Grid<usize> {
        derive(&self.rows, &self.blanks, self.tiles.len(), self.columns)
    }

    /// How many tiles each row holds, top to bottom.
    pub fn row_lengths(&self) -> Vec<usize> {
        self.slots().iter().map(|r| r.iter().flatten().count()).collect()
    }

    /// The row and the slot in it (0-based, counting blanks) of the tile at flat `index`.
    pub fn row_of(&self, index: usize) -> (usize, usize) {
        let g = self.slots();
        find(&g, &index).unwrap_or_else(|| (g.len().saturating_sub(1), g.last().map_or(0, Vec::len)))
    }

    /// The flat indices of the tiles in the rectangle with corners at the tiles at flat
    /// `a` and `b` (Shift-click, D-169): in the set's slot grid, or with `columns` in lines
    /// of that many (the Auto and Fixed columns layouts, and the cue list as one column).
    pub fn rectangle(&self, a: usize, b: usize, columns: Option<usize>) -> Vec<usize> {
        let g = match columns {
            Some(c) => derive(&[], &[], self.tiles.len(), c.max(1) as u32),
            None => self.slots(),
        };
        match (find(&g, &a), find(&g, &b)) {
            (Some(a), Some(b)) => rectangle(&g, a, b),
            _ => Vec::new(),
        }
    }

    /// Lay the tiles out as `grid` says, where its indices name `pool`'s tiles (any it
    /// leaves out follow at the end). Stores the shape.
    fn arrange(&mut self, mut grid: Grid<usize>, mut pool: Vec<Option<Tile>>) {
        prune(&mut grid);
        let mut tiles = Vec::with_capacity(pool.len());
        let mut rows = Vec::with_capacity(grid.len());
        let mut blanks = Vec::new();
        for (r, row) in grid.iter_mut().enumerate() {
            let mut pads = 0;
            for (s, slot) in row.iter_mut().enumerate() {
                match slot.and_then(|i| pool.get_mut(i)?.take()) {
                    Some(t) => {
                        *slot = Some(tiles.len());
                        tiles.push(t);
                        pads += 1;
                    }
                    None => {
                        *slot = None;
                        blanks.push([r as u32, s as u32]);
                    }
                }
            }
            rows.push(pads);
        }
        tiles.extend(pool.into_iter().flatten());
        self.tiles = tiles;
        self.rows = rows;
        self.blanks = blanks;
    }

    /// Give the set this shape: `grid`'s pads are taken as the tiles in reading order
    /// (their indices don't matter), and tiles beyond them follow the fill rule.
    pub fn set_shape<T>(&mut self, grid: Grid<T>) {
        let n = self.tiles.len();
        let mut next = 0;
        let mut g: Grid<usize> = Vec::new();
        for row in grid {
            g.push(row.into_iter().map(|x| x.map(|_| next).inspect(|_| next += 1).filter(|&i| i < n)).collect());
        }
        put(&mut g, Place::Append, (next.min(n)..n).map(Some).collect(), self.columns as usize);
        let pool = std::mem::take(&mut self.tiles).into_iter().map(Some).collect();
        self.arrange(g, pool);
    }

    /// Put `new` tiles at `place`; returns the flat index of the first.
    pub fn insert_tiles(&mut self, place: Place, new: Vec<Tile>) -> usize {
        let n = self.tiles.len();
        let k = new.len();
        if k == 0 {
            return n;
        }
        let first = new[0].id;
        let mut grid = self.slots();
        put(&mut grid, place, (n..n + k).map(Some).collect(), self.columns as usize);
        let pool = std::mem::take(&mut self.tiles).into_iter().chain(new).map(Some).collect();
        self.arrange(grid, pool);
        self.tiles.iter().position(|t| t.id == first).unwrap_or(n)
    }

    /// Take out the tile at flat `index`; its row closes up, and a row it leaves
    /// without pads goes.
    pub fn remove_tile_at(&mut self, index: usize) -> Tile {
        let mut grid = self.slots();
        take(&mut grid, &index);
        let tile = self.tiles[index].clone();
        let mut pool: Vec<Option<Tile>> = std::mem::take(&mut self.tiles).into_iter().map(Some).collect();
        pool[index] = None;
        self.arrange(grid, pool);
        tile
    }

    /// Move the tile at flat `from` to `place`, given as for the grid with the tile
    /// already taken out of its slot (rows keep their numbers; `at` is where it ends up
    /// in that row). Returns its new flat index.
    pub fn move_tile(&mut self, from: usize, place: Place) -> usize {
        self.move_tiles(&[from], place, false).first().copied().unwrap_or(from)
    }

    /// As [`move_tile`](Self::move_tile), but a blank stays in the tile's old slot, so
    /// every slot keeps its number and nothing else moves (D-202).
    pub fn move_tile_leaving_blank(&mut self, from: usize, place: Place) -> usize {
        self.move_tiles(&[from], place, true).first().copied().unwrap_or(from)
    }

    /// Move the tiles at flat `from` together to `place`, as one run in the set's order
    /// (D-219). `place` is given as for the grid with them all taken out of their slots
    /// (rows keep their numbers); with `leave_blank`, a blank stays in each one's old
    /// slot instead. Returns their new flat indices, in the set's order.
    pub fn move_tiles(&mut self, from: &[usize], place: Place, leave_blank: bool) -> Vec<usize> {
        let mut from: Vec<usize> = from.iter().copied().filter(|&i| i < self.tiles.len()).collect();
        from.sort_unstable();
        from.dedup();
        let ids: Vec<_> = from.iter().map(|&i| self.tiles[i].id).collect();
        let mut grid = self.slots();
        for i in &from {
            match find(&grid, i) {
                Some((r, s)) if leave_blank => grid[r][s] = None,
                _ => {
                    take(&mut grid, i);
                }
            }
        }
        put(&mut grid, place, from.iter().map(|&i| Some(i)).collect(), self.columns as usize);
        let pool = std::mem::take(&mut self.tiles).into_iter().map(Some).collect();
        self.arrange(grid, pool);
        ids.iter().zip(&from).map(|(id, &f)| self.tiles.iter().position(|t| t.id == *id).unwrap_or(f)).collect()
    }

    /// Insert `count` blank slots into the gap at `place` (`InRow`, or `Slot` before a
    /// slot). False when there's no such row: a row can't be all blanks.
    pub fn insert_blanks(&mut self, place: Place, count: usize) -> bool {
        let mut grid = self.slots();
        let row = match place {
            Place::InRow { row, .. } | Place::Slot { row, .. } => row,
            _ => return false,
        };
        if row >= grid.len() || count == 0 {
            return false;
        }
        let at = match place {
            Place::InRow { at, .. } | Place::Slot { at, .. } => at,
            _ => unreachable!(),
        };
        put(&mut grid, Place::InRow { row, at }, vec![None; count], 1);
        let pool = std::mem::take(&mut self.tiles).into_iter().map(Some).collect();
        self.arrange(grid, pool);
        true
    }

    /// Remove the blank slot `at` of `row` (the slots after it move up one). False when
    /// that slot isn't blank.
    pub fn remove_blank(&mut self, row: usize, at: usize) -> bool {
        let mut grid = self.slots();
        if grid.get(row).and_then(|r| r.get(at)) != Some(&None) {
            return false;
        }
        grid[row].remove(at);
        let pool = std::mem::take(&mut self.tiles).into_iter().map(Some).collect();
        self.arrange(grid, pool);
        true
    }

    /// Set the rows' lengths outright, with no blanks (they must add up to no more than
    /// the tiles; any left over follow the fill rule).
    pub fn set_rows(&mut self, rows: &[usize]) {
        self.rows = rows.iter().map(|&r| r as u32).collect();
        self.blanks.clear();
    }

    /// Even the rows out to `per_row` tiles each, with no blanks; `per_row` becomes the
    /// set's row size.
    pub fn even_rows(&mut self, per_row: u32) {
        self.columns = per_row.clamp(1, 12);
        self.rows.clear();
        self.blanks.clear();
        let rows = self.row_lengths();
        self.set_rows(&rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(rows: &[u32], n: usize, columns: u32) -> SoundSet {
        let tiles = (0..n).map(|i| Tile { title: format!("{i}"), ..Default::default() }).collect();
        SoundSet { rows: rows.to_vec(), tiles, columns, ..SoundSet::named("Rows") }
    }

    /// The grid by title, "_" for a blank.
    fn titles(s: &SoundSet) -> Vec<Vec<String>> {
        s.slots().iter().map(|r| r.iter().map(|x| x.map_or("_".into(), |i| s.tiles[i].title.clone())).collect()).collect()
    }

    fn t(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect()
    }

    fn new(name: &str) -> Vec<Tile> {
        vec![Tile { title: name.into(), ..Default::default() }]
    }

    #[test]
    fn empty_rows_are_the_old_column_wrap_and_appends_fill_the_last_row() {
        let lens = |rows: &[u32], n, c| derive(rows, &[], n, c).iter().map(Vec::len).collect::<Vec<_>>();
        assert_eq!(lens(&[], 12, 5), vec![5, 5, 2]);
        assert_eq!(lens(&[], 0, 5), Vec::<usize>::new());
        assert_eq!(lens(&[3, 8], 11, 5), vec![3, 8], "stored rows as they are");
        assert_eq!(lens(&[3, 8], 13, 5), vec![3, 8, 2], "a full last row: appends start a new one");
        assert_eq!(lens(&[3], 6, 5), vec![5, 1], "a short last row fills up to the row size first");
        assert_eq!(lens(&[3, 0, 8], 5, 5), vec![3, 2], "zeros skipped, clipped to the tiles");
        assert_eq!(lens(&[], 3, 0), vec![1, 1, 1], "a zero row size counts as one");
        assert_eq!(derive(&[2], &[], 2, 5), vec![vec![Some(0), Some(1)]]);
    }

    #[test]
    fn blanks_sit_between_the_pads_and_never_reorder_them() {
        // [0 _ 1] [_ 2 3 _]
        let g = derive(&[2, 2], &[[0, 1], [1, 0], [1, 3]], 4, 5);
        assert_eq!(g, vec![vec![Some(0), None, Some(1)], vec![None, Some(2), Some(3), None]]);
        // Blanks count towards the row size when appending.
        let g = derive(&[2], &[[0, 1], [0, 2]], 4, 5);
        assert_eq!(g, vec![vec![Some(0), None, None, Some(1), Some(2)], vec![Some(3)]]);
        // A blank past the row's pads stays at its end; a row whose pads ran out goes.
        assert_eq!(derive(&[1], &[[0, 7]], 1, 5), vec![vec![Some(0), None]]);
        assert_eq!(derive(&[1, 1], &[[1, 0]], 1, 5), vec![vec![Some(0)]]);
    }

    #[test]
    fn inserting_into_gaps_blanks_and_new_rows() {
        let mut s = set(&[2, 3], 5, 5);
        assert_eq!(s.insert_tiles(Place::InRow { row: 0, at: 1 }, new("a")), 1);
        assert_eq!(titles(&s), t(&[&["0", "a", "1"], &["2", "3", "4"]]));
        assert_eq!(s.insert_tiles(Place::NewRow(1), new("b")), 3);
        assert_eq!(titles(&s), t(&[&["0", "a", "1"], &["b"], &["2", "3", "4"]]));
        assert_eq!(s.insert_tiles(Place::NewRow(9), new("c")), 7, "past the last row: a new last row");
        assert_eq!(s.insert_tiles(Place::InRow { row: 9, at: 0 }, new("d")), 8);
        assert_eq!(titles(&s), t(&[&["0", "a", "1"], &["b"], &["2", "3", "4"], &["c"], &["d"]]));
        // A flat place goes before the pad there, in its row; at the end, the last row.
        assert_eq!(s.insert_tiles(Place::Flat(3), new("e")), 3);
        assert_eq!(s.insert_tiles(Place::Flat(99), new("f")), 10);
        assert_eq!(titles(&s), t(&[&["0", "a", "1"], &["e", "b"], &["2", "3", "4"], &["c"], &["d", "f"]]));
        // Appending follows the fill rule.
        s.insert_tiles(Place::Append, new("g"));
        assert_eq!(titles(&s).last().cloned(), Some(vec!["d".into(), "f".into(), "g".into()]));
        // Into an empty set.
        let mut e = set(&[], 0, 4);
        e.insert_tiles(Place::NewRow(0), new("x"));
        assert_eq!(titles(&e), t(&[&["x"]]));
        // A blank slot fills and nothing moves; several fill it and push along after it.
        let mut s = set(&[3], 3, 5);
        assert!(s.insert_blanks(Place::InRow { row: 0, at: 1 }, 2));
        assert_eq!(titles(&s), t(&[&["0", "_", "_", "1", "2"]]));
        assert_eq!(s.insert_tiles(Place::Slot { row: 0, at: 2 }, new("a")), 1);
        assert_eq!(titles(&s), t(&[&["0", "_", "a", "1", "2"]]));
        let two = vec![Tile { title: "b".into(), ..Default::default() }, Tile { title: "c".into(), ..Default::default() }];
        assert_eq!(s.insert_tiles(Place::Slot { row: 0, at: 1 }, two), 1);
        assert_eq!(titles(&s), t(&[&["0", "b", "c", "a", "1", "2"]]));
        assert_eq!(s.tiles.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["0", "b", "c", "a", "1", "2"], "reading order");
        // A slot holding a pad: inserted before it.
        s.insert_tiles(Place::Slot { row: 0, at: 0 }, new("z"));
        assert_eq!(titles(&s)[0][0], "z");
    }

    #[test]
    fn removing_a_rows_last_tile_removes_the_row_and_its_blanks() {
        let mut s = set(&[2, 1, 3], 6, 5);
        assert_eq!(s.remove_tile_at(2).title, "2");
        assert_eq!(titles(&s), t(&[&["0", "1"], &["3", "4", "5"]]));
        s.remove_tile_at(0);
        assert_eq!(s.rows, vec![1, 3]);
        let mut s = set(&[1, 2], 3, 5);
        s.insert_blanks(Place::InRow { row: 0, at: 0 }, 1);
        s.insert_blanks(Place::InRow { row: 1, at: 1 }, 1);
        assert_eq!(titles(&s), t(&[&["_", "0"], &["1", "_", "2"]]));
        assert_eq!(s.blanks, vec![[0, 0], [1, 1]]);
        s.remove_tile_at(0);
        assert_eq!(titles(&s), t(&[&["1", "_", "2"]]));
        s.remove_tile_at(0);
        assert_eq!(titles(&s), t(&[&["_", "2"]]), "the row closes up");
        assert!(!s.remove_blank(0, 1), "not a blank");
        assert!(s.remove_blank(0, 0));
        assert_eq!((titles(&s), s.blanks.clone()), (t(&[&["2"]]), vec![]));
        assert!(!s.insert_blanks(Place::NewRow(0), 1), "a row can't be all blanks");
    }

    #[test]
    fn moving_tiles_between_rows() {
        // Rows as shown before the move: [0 1 2] [3] [4 5].
        let fresh = || set(&[3, 1, 2], 6, 5);
        let mut s = fresh();
        assert_eq!(s.move_tile(0, Place::InRow { row: 2, at: 1 }), 4);
        assert_eq!(titles(&s), t(&[&["1", "2"], &["3"], &["4", "0", "5"]]));
        // Leaving a row empty removes it; later row numbers are as they were before.
        let mut s = fresh();
        s.move_tile(3, Place::InRow { row: 2, at: 0 });
        assert_eq!(titles(&s), t(&[&["0", "1", "2"], &["3", "4", "5"]]));
        // Within its own row: `at` is where it ends up.
        let mut s = fresh();
        assert_eq!(s.move_tile(0, Place::InRow { row: 0, at: 2 }), 2);
        assert_eq!(titles(&s), t(&[&["1", "2", "0"], &["3"], &["4", "5"]]));
        // A new row between two others, or at the top.
        let mut s = fresh();
        s.move_tile(5, Place::NewRow(1));
        assert_eq!(titles(&s), t(&[&["0", "1", "2"], &["5"], &["3"], &["4"]]));
        let mut s = fresh();
        s.move_tile(3, Place::NewRow(0));
        assert_eq!(titles(&s), t(&[&["3"], &["0", "1", "2"], &["4", "5"]]));
        // A tile alone in its row made a new row just above or below itself: no change.
        for r in [1, 2] {
            let mut s = fresh();
            s.move_tile(3, Place::NewRow(r));
            assert_eq!(titles(&s), titles(&fresh()), "new row {r}");
        }
        // The old flat `to`: lands there, in the row of the tile it displaces.
        let mut s = fresh();
        assert_eq!(s.move_tile(2, Place::Flat(3)), 3);
        assert_eq!(titles(&s), t(&[&["0", "1"], &["3"], &["2", "4", "5"]]));
        let mut s = fresh();
        assert_eq!(s.move_tile(0, Place::Flat(99)), 5);
        assert_eq!(titles(&s), t(&[&["1", "2"], &["3"], &["4", "5", "0"]]));
        // The only tile in the set.
        let mut one = set(&[1], 1, 5);
        assert_eq!(one.move_tile(0, Place::NewRow(3)), 0);
        assert_eq!(titles(&one), t(&[&["0"]]));
    }

    #[test]
    fn a_move_can_leave_a_blank_where_the_tile_was() {
        // [0 1 2 3] [4]: slots keep the numbers they have now (D-202).
        let fresh = || set(&[4, 1], 5, 4);
        let mut s = fresh();
        assert_eq!(s.move_tile_leaving_blank(2, Place::Slot { row: 0, at: 4 }), 3);
        assert_eq!(titles(&s), t(&[&["0", "1", "_", "3", "2"], &["4"]]), "past the row's last tile");
        let mut s = fresh();
        s.move_tile_leaving_blank(3, Place::Slot { row: 0, at: 4 });
        assert_eq!(titles(&s), t(&[&["0", "1", "2", "_", "3"], &["4"]]), "the last tile one slot right");
        let mut s = fresh();
        s.move_tile_leaving_blank(0, Place::Slot { row: 0, at: 6 });
        assert_eq!(titles(&s), t(&[&["_", "1", "2", "3", "_", "_", "0"], &["4"]]), "further: blanks before it");
        // Into a blank of its row: the two change places.
        let mut s = fresh();
        s.move_tile_leaving_blank(3, Place::Slot { row: 0, at: 4 });
        s.move_tile_leaving_blank(1, Place::Slot { row: 0, at: 3 });
        assert_eq!(titles(&s), t(&[&["0", "_", "2", "1", "3"], &["4"]]));
        // Out of a row it was alone in: the blank goes with the row.
        let mut s = fresh();
        s.move_tile_leaving_blank(4, Place::InRow { row: 0, at: 0 });
        assert_eq!(titles(&s), t(&[&["4", "0", "1", "2", "3"]]));
    }

    #[test]
    fn several_tiles_move_as_one_run_in_set_order() {
        // [0 1 2] [3] [4 5], with 4 and 1 (given out of order) moved together.
        let fresh = || set(&[3, 1, 2], 6, 5);
        let mut s = fresh();
        assert_eq!(s.move_tiles(&[4, 1], Place::InRow { row: 0, at: 0 }, false), vec![0, 1]);
        assert_eq!(titles(&s), t(&[&["1", "4", "0", "2"], &["3"], &["5"]]));
        // A row they leave without tiles goes; rows are numbered as before the move.
        let mut s = fresh();
        assert_eq!(s.move_tiles(&[3, 5], Place::InRow { row: 0, at: 1 }, false), vec![1, 2]);
        assert_eq!(titles(&s), t(&[&["0", "3", "5", "1", "2"], &["4"]]));
        // A new row of their own, and the flat `to`: where the first ends up.
        let mut s = fresh();
        s.move_tiles(&[0, 2], Place::NewRow(3), false);
        assert_eq!(titles(&s), t(&[&["1"], &["3"], &["4", "5"], &["0", "2"]]));
        let mut s = fresh();
        assert_eq!(s.move_tiles(&[0, 1], Place::Flat(2), false), vec![2, 3]);
        assert_eq!(titles(&s), t(&[&["2"], &["3"], &["0", "1", "4", "5"]]));
        // Leaving blanks: their old slots stay, nothing else moves.
        let mut s = fresh();
        s.move_tiles(&[0, 1], Place::Slot { row: 2, at: 2 }, true);
        assert_eq!(titles(&s), t(&[&["_", "_", "2"], &["3"], &["4", "5", "0", "1"]]));
        // Into a blank: the first fills it, the rest push along after it.
        let mut s = fresh();
        s.insert_blanks(Place::InRow { row: 1, at: 0 }, 1);
        s.move_tiles(&[4, 5], Place::Slot { row: 1, at: 0 }, false);
        assert_eq!(titles(&s), t(&[&["0", "1", "2"], &["4", "5", "3"]]));
    }

    #[test]
    fn moving_into_and_past_blanks() {
        // [0 _ 1] [2 3]
        let fresh = || {
            let mut s = set(&[2, 2], 4, 5);
            s.insert_blanks(Place::InRow { row: 0, at: 1 }, 1);
            s
        };
        // Into the blank: it fills, and only the tile's old row closes up.
        let mut s = fresh();
        assert_eq!(s.move_tile(3, Place::Slot { row: 0, at: 1 }), 1);
        assert_eq!(titles(&s), t(&[&["0", "3", "1"], &["2"]]));
        // Its own row's blank, after it: numbered with the tile already out.
        let mut s = fresh();
        s.move_tile(0, Place::Slot { row: 0, at: 0 });
        assert_eq!(titles(&s), t(&[&["0", "1"], &["2", "3"]]));
        // Swapping places with a blank (⌘→): a gap insert past it.
        let mut s = fresh();
        s.move_tile(0, Place::InRow { row: 0, at: 1 });
        assert_eq!(titles(&s), t(&[&["_", "0", "1"], &["2", "3"]]));
        // A blank is left alone when a tile is moved out of its row's other slots.
        let mut s = fresh();
        s.move_tile(1, Place::NewRow(2));
        assert_eq!(titles(&s), t(&[&["0", "_"], &["2", "3"], &["1"]]));
        // The row of a tile alone with blanks goes when it leaves.
        let mut s = fresh();
        s.move_tile(1, Place::NewRow(2));
        s.move_tile(0, Place::InRow { row: 1, at: 9 });
        assert_eq!(titles(&s), t(&[&["2", "3", "0"], &["1"]]));
    }

    #[test]
    fn a_slot_past_a_rows_end_opens_blanks_up_to_it() {
        // D-190: [0 1 2] [3]; the row's own last pad into the empty slot after it.
        let fresh = || set(&[3, 1], 4, 5);
        let mut s = fresh();
        assert_eq!(s.move_tile(2, Place::Slot { row: 0, at: 3 }), 2);
        assert_eq!(titles(&s), t(&[&["0", "1", "_", "2"], &["3"]]), "a blank where it was");
        assert_eq!(s.blanks, vec![[0, 2]]);
        // Further right: more blanks; the row's first pad: the row closes up first.
        let mut s = fresh();
        s.move_tile(0, Place::Slot { row: 0, at: 4 });
        assert_eq!(titles(&s), t(&[&["1", "2", "_", "_", "0"], &["3"]]));
        // From another row, the slot just past the end is the row's end, as before.
        let mut s = fresh();
        s.move_tile(3, Place::Slot { row: 0, at: 3 });
        assert_eq!(titles(&s), t(&[&["0", "1", "2", "3"]]));
        let mut s = fresh();
        s.insert_tiles(Place::Slot { row: 1, at: 2 }, new("a"));
        assert_eq!(titles(&s), t(&[&["0", "1", "2"], &["3", "_", "a"]]));
        // Absurdly far (the CLI's --slot): the row's end.
        let mut s = fresh();
        s.move_tile(3, Place::Slot { row: 0, at: usize::MAX });
        assert_eq!(titles(&s), t(&[&["0", "1", "2", "3"]]));
    }

    #[test]
    fn shift_click_selects_the_rectangle_between_two_pads() {
        // [0 1 2 3 4] / [5 _ 6] / [7 8 9 10 11 12 13]
        let mut s = set(&[5, 2, 7], 14, 5);
        s.blanks = vec![[1, 1]];
        let g = s.slots();
        assert_eq!(rectangle(&g, (0, 1), (2, 3)), vec![1, 2, 3, 6, 8, 9, 10], "rows x slots; the blank and row 1's missing slot 3 are skipped");
        assert_eq!(rectangle(&g, (2, 3), (0, 1)), rectangle(&g, (0, 1), (2, 3)), "either corner first");
        assert_eq!(rectangle(&g, (0, 4), (0, 4)), vec![4], "one slot");
        assert_eq!(rectangle(&g, (0, 4), (2, 6)), vec![4, 11, 12, 13], "past a short row's end");
        // By the tiles' flat indices: tile 2 to tile 9 is slots (0, 2) to (2, 2).
        assert_eq!(s.rectangle(2, 9, None), vec![2, 6, 9]);
        assert_eq!(s.rectangle(9, 2, None), vec![2, 6, 9]);
        // In lines of 4 (Auto/Fixed columns): [0 1 2 3] [4 5 6 7] [8 9 10 11]: 2 to 9 is
        // columns 1-2 of the first three lines.
        assert_eq!(s.rectangle(2, 9, Some(4)), vec![1, 2, 5, 6, 9, 10]);
        // One column (the cue list) is the run between them.
        assert_eq!(s.rectangle(9, 2, Some(1)), (2..=9).collect::<Vec<_>>());
        assert!(s.rectangle(2, 99, None).is_empty());
    }

    #[test]
    fn evening_rows_out_clears_the_blanks() {
        let mut s = set(&[3, 8], 11, 5);
        s.insert_blanks(Place::InRow { row: 0, at: 0 }, 1);
        s.even_rows(4);
        assert_eq!((s.columns, s.rows.clone(), s.blanks.clone()), (4, vec![4, 4, 3], vec![]));
        s.set_rows(&[1, 2]);
        assert_eq!(s.row_lengths(), vec![1, 4, 4, 2], "left over: the last row fills up to 4, then rows of 4");
    }
}
