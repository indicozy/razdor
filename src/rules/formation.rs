//! The card formation each side fights in (mechanics.md 2.1, original-mechanics/battle.md §6).
//!
//! Both shapes hold 12 units in three rows: front, back and reserve.
//! - Vanilla: 4 columns, 3 × 4.
//! - Community "wide row" (the default, as in the player's install): 6 columns, but the
//!   original blocks cells: the front row has 6, the back row 4 (columns 2–5, 1-based) and the
//!   reserve 2 (columns 3–4). On screen it is a 2 × 6 grid: the two reserve cells sit at the
//!   ends of the back row, where the original draws its tent icons (492940).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Row {
    /// Row 1: fights in melee, screens the back row.
    Front,
    /// Row 2: shooters and mages; +`Row2Def` defence against shots.
    Back,
    /// Row 3: cannot be targeted; its units can only move out (and friendly casters tend
    /// the reserve).
    Reserve,
}

impl Row {
    /// 1-based row number as the original counts it (front = 1).
    pub fn number(self) -> i32 {
        match self {
            Row::Front => 1,
            Row::Back => 2,
            Row::Reserve => 3,
        }
    }

    /// Front or back: the rows that fight.
    pub fn is_active(self) -> bool {
        self != Row::Reserve
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Slot {
    pub row: Row,
    pub col: u8,
}

impl Slot {
    pub const fn new(row: Row, col: u8) -> Self {
        Slot { row, col }
    }
}

/// Shape of a side's formation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Formation {
    pub cols: u8,
    /// A third, reserve row behind the back row.
    pub reserve: bool,
}

impl Default for Formation {
    fn default() -> Self {
        Formation::WIDE
    }
}

impl Formation {
    /// Community "wide row": front 6, back 4 (columns 1–4, 0-based), reserve 2 (columns 2–3).
    pub const WIDE: Formation = Formation { cols: 6, reserve: true };
    /// Vanilla: 3 × 4, the third row being the reserve.
    pub const VANILLA: Formation = Formation { cols: 4, reserve: true };

    /// The original's blocked-cell pattern (48395c) applies to the 6-column grid.
    fn wide(&self) -> bool {
        self.cols == 6 && self.reserve
    }

    pub fn rows(&self) -> &'static [Row] {
        if self.reserve {
            &[Row::Front, Row::Back, Row::Reserve]
        } else {
            &[Row::Front, Row::Back]
        }
    }

    /// Usable columns of `row` (0-based, end exclusive).
    pub fn col_range(&self, row: Row) -> std::ops::Range<u8> {
        match (self.wide(), row) {
            (true, Row::Back) => 1..5,
            (true, Row::Reserve) => 2..4,
            _ => 0..self.cols,
        }
    }

    pub fn slots(self) -> impl Iterator<Item = Slot> {
        self.rows().iter().flat_map(move |&row| self.col_range(row).map(move |col| Slot { row, col }))
    }

    pub fn contains(&self, s: Slot) -> bool {
        self.rows().contains(&s.row) && self.col_range(s.row).contains(&s.col)
    }

    /// Units a side can field (12 for both shapes).
    pub fn capacity(&self) -> usize {
        self.slots().count()
    }

    /// The vanilla 4 columns with a reserve: the original draws them on its 2 × 6 places too.
    fn short(&self) -> bool {
        self.cols == 4 && self.reserve
    }

    /// Grid lines on screen: 2 for both of the original's shapes (its 12 places, 492940),
    /// else one per row.
    pub fn display_lines(&self) -> usize {
        if self.wide() || self.short() {
            2
        } else {
            self.rows().len()
        }
    }

    /// Grid columns on screen: 6 for both of the original's shapes, else the columns.
    pub fn display_cols(&self) -> u8 {
        if self.wide() || self.short() {
            6
        } else {
            self.cols
        }
    }

    /// Where `s` is drawn: (line from the front, column). The original's places (492940):
    /// 6 columns put the reserve at the back row's ends; 4 columns put the front and back
    /// rows in the middle four places of their lines and the reserve at all four ends
    /// (reserve 1 front right, 2 back left, 3 back right, 4 front left, 1-based).
    pub fn display(&self, s: Slot) -> (usize, u8) {
        match (self.wide(), self.short(), s.row) {
            (true, _, Row::Reserve) => (1, if s.col <= 2 { 0 } else { self.cols - 1 }),
            (_, true, Row::Front) => (0, s.col + 1),
            (_, true, Row::Back) => (1, s.col + 1),
            (_, true, Row::Reserve) => match s.col {
                0 => (0, 5),
                1 => (1, 0),
                2 => (1, 5),
                _ => (0, 0),
            },
            _ => (self.rows().iter().position(|&r| r == s.row).unwrap_or(0), s.col),
        }
    }

    /// The cell drawn at (`line`, `col`), if any.
    pub fn at_display(&self, line: usize, col: u8) -> Option<Slot> {
        self.slots().find(|&s| self.display(s) == (line, col))
    }

    /// Columns from the centre outwards, as the original auto-places
    /// (3,2,4,1 for 4 columns and 4,3,5,2,6,1 for 6, 1-based).
    pub fn col_order(&self) -> Vec<u8> {
        let n = self.cols as i32;
        let mut out = Vec::with_capacity(n as usize);
        let mid = n / 2;
        out.push(mid);
        for d in 1..=n {
            for c in [mid - d, mid + d] {
                if (0..n).contains(&c) && out.len() < n as usize {
                    out.push(c);
                }
            }
        }
        out.into_iter().map(|c| c as u8).collect()
    }

    /// Where the original puts a unit added to an army (495ce0, 495fac): the first free cell
    /// of the reserve, then the back row, then the front row, columns in the preferred order,
    /// whatever the unit's role.
    pub fn new_unit_slot(&self, occupied: &[Slot]) -> Option<Slot> {
        self.first_free(occupied, &self.col_order())
    }

    /// Where the battle's write-back puts a unit left without a cell (4988c0): the same rows,
    /// reserve first, but the columns in plain order from the first, not the preferred one.
    pub fn after_battle_slot(&self, occupied: &[Slot]) -> Option<Slot> {
        self.first_free(occupied, &(0..self.cols).collect::<Vec<_>>())
    }

    fn first_free(&self, occupied: &[Slot], order: &[u8]) -> Option<Slot> {
        [Row::Reserve, Row::Back, Row::Front]
            .into_iter()
            .filter(|r| self.rows().contains(r))
            .flat_map(|row| order.iter().map(move |&col| Slot { row, col }))
            .find(|s| self.contains(*s) && !occupied.contains(s))
    }

    /// Puts every cell the formation does not have (a save or a row width from before) on a
    /// free cell of its own row, the cells it has kept where they are. The one helper for
    /// every list of units: the squad, a garrison, an army ([`Game::reseat_all`]).
    pub fn reseat<'a>(&self, cells: impl IntoIterator<Item = &'a mut Slot>) {
        let mut cells: Vec<&mut Slot> = cells.into_iter().collect();
        let mut taken: Vec<Slot> = cells.iter().filter(|s| self.contains(***s)).map(|s| **s).collect();
        for s in cells.iter_mut().filter(|s| !self.contains(***s)) {
            if let Some(free) = self.free_slot(&taken, s.row) {
                **s = free;
                taken.push(free);
            }
        }
    }

    /// First free cell: the preferred row first, then the other fighting row, then the reserve.
    pub fn free_slot(&self, occupied: &[Slot], preferred: Row) -> Option<Slot> {
        let mut rows = vec![preferred];
        rows.extend(self.rows().iter().copied().filter(|r| *r != preferred));
        let order = self.col_order();
        rows.into_iter()
            .filter(|r| self.rows().contains(r))
            .flat_map(|row| order.iter().map(move |&col| Slot { row, col }))
            .find(|s| self.contains(*s) && !occupied.contains(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_order_is_centre_out() {
        assert_eq!(Formation::WIDE.col_order(), vec![3, 2, 4, 1, 5, 0]);
        assert_eq!(Formation::VANILLA.col_order(), vec![2, 1, 3, 0]);
    }

    #[test]
    fn both_shapes_hold_twelve() {
        assert_eq!(Formation::WIDE.capacity(), 12);
        assert_eq!(Formation::VANILLA.capacity(), 12);
        assert_eq!(Formation::WIDE.slots().count(), 12);
        assert!(Formation::VANILLA.contains(Slot::new(Row::Reserve, 3)));
        assert!(!Formation::VANILLA.contains(Slot::new(Row::Front, 4)));
    }

    #[test]
    fn wide_row_is_front_6_back_4_reserve_2() {
        // 48395c: −1 in back-row columns 1 and 6 and reserve columns 1, 2, 5, 6 (1-based).
        let w = Formation::WIDE;
        let cols = |row| w.slots().filter(|s| s.row == row).map(|s| s.col).collect::<Vec<_>>();
        assert_eq!(cols(Row::Front), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(cols(Row::Back), vec![1, 2, 3, 4]);
        assert_eq!(cols(Row::Reserve), vec![2, 3]);
        assert!(!w.contains(Slot::new(Row::Back, 0)) && !w.contains(Slot::new(Row::Back, 5)));
        assert!(!w.contains(Slot::new(Row::Reserve, 1)) && !w.contains(Slot::new(Row::Reserve, 4)));
    }

    #[test]
    fn wide_row_draws_the_reserve_at_the_back_row_ends() {
        // 492940: the second display line is reserve 3, back 2–5, reserve 4 (1-based).
        let w = Formation::WIDE;
        assert_eq!(w.display_lines(), 2);
        let line: Vec<Option<Slot>> = (0..6).map(|c| w.at_display(1, c)).collect();
        assert_eq!(
            line,
            vec![
                Some(Slot::new(Row::Reserve, 2)),
                Some(Slot::new(Row::Back, 1)),
                Some(Slot::new(Row::Back, 2)),
                Some(Slot::new(Row::Back, 3)),
                Some(Slot::new(Row::Back, 4)),
                Some(Slot::new(Row::Reserve, 3)),
            ]
        );
        assert!((0..6).all(|c| w.at_display(0, c) == Some(Slot::new(Row::Front, c))));
    }

    #[test]
    fn four_columns_draw_the_reserve_at_both_lines_ends() {
        // 492940 with 4 columns: front places 1-4, back 7-10, reserve 5, 6, 11, 0.
        let v = Formation::VANILLA;
        assert_eq!((v.display_lines(), v.display_cols()), (2, 6));
        let line = |l| (0..6).map(|c| v.at_display(l, c)).collect::<Vec<_>>();
        let (f, b, r) = (|c| Some(Slot::new(Row::Front, c)), |c| Some(Slot::new(Row::Back, c)), |c| Some(Slot::new(Row::Reserve, c)));
        assert_eq!(line(0), vec![r(3), f(0), f(1), f(2), f(3), r(0)]);
        assert_eq!(line(1), vec![r(1), b(0), b(1), b(2), b(3), r(2)]);
    }

    #[test]
    fn a_new_unit_fills_the_reserve_then_the_back_then_the_front() {
        // 495ce0: reserve column 4, then 3 (1-based), then the back row 4, 3, 5, 2.
        let w = Formation::WIDE;
        let mut taken = Vec::new();
        let mut order = Vec::new();
        while let Some(s) = w.new_unit_slot(&taken) {
            taken.push(s);
            order.push(s);
        }
        assert_eq!(order.len(), 12);
        assert_eq!(&order[..6], &[Slot::new(Row::Reserve, 3), Slot::new(Row::Reserve, 2), Slot::new(Row::Back, 3), Slot::new(Row::Back, 2), Slot::new(Row::Back, 4), Slot::new(Row::Back, 1)]);
        assert_eq!(order[6], Slot::new(Row::Front, 3));
        let v = Formation::VANILLA;
        assert_eq!(v.new_unit_slot(&[]), Some(Slot::new(Row::Reserve, 2)), "4 columns: reserve column 3 first");
    }

    #[test]
    fn the_battle_write_back_fills_columns_in_plain_order() {
        // 4988c0: reserve, back, front, each from its first column (1-based 1, 2, 3, 4).
        let v = Formation::VANILLA;
        assert_eq!(v.after_battle_slot(&[]), Some(Slot::new(Row::Reserve, 0)));
        let full: Vec<Slot> = (0..4).map(|c| Slot::new(Row::Reserve, c)).collect();
        assert_eq!(v.after_battle_slot(&full), Some(Slot::new(Row::Back, 0)));
        // 6 columns: only the formation's cells, reserve columns 3 and 4 first.
        let w = Formation::WIDE;
        assert_eq!(w.after_battle_slot(&[]), Some(Slot::new(Row::Reserve, 2)));
        assert_eq!(w.after_battle_slot(&[Slot::new(Row::Reserve, 2), Slot::new(Row::Reserve, 3)]), Some(Slot::new(Row::Back, 1)));
    }

    #[test]
    fn reseat_moves_only_the_cells_the_formation_lacks() {
        // Front 6 is past the 6-wide grid; in the 4-wide shape Front 5 is past its row.
        let mut cells = [Slot::new(Row::Front, 6), Slot::new(Row::Back, 2), Slot::new(Row::Front, 5), Slot::new(Row::Front, 0)];
        Formation::WIDE.reseat(cells.iter_mut());
        assert_eq!(cells[1], Slot::new(Row::Back, 2), "a cell the formation has is kept");
        assert_eq!(cells[2], Slot::new(Row::Front, 5), "Front 5 is a cell of the 6-wide row");
        assert_eq!(cells[0], Slot::new(Row::Front, 3), "off the grid: the first free cell of its row, centre out");
        let mut narrow = [Slot::new(Row::Front, 5), Slot::new(Row::Front, 0)];
        Formation::VANILLA.reseat(narrow.iter_mut());
        assert_eq!(narrow, [Slot::new(Row::Front, 2), Slot::new(Row::Front, 0)], "4 wide: Front 5 goes to the centre, Front 0 stays");
    }

    #[test]
    fn free_slot_prefers_row_and_centre() {
        let f = Formation::WIDE;
        assert_eq!(f.free_slot(&[], Row::Back), Some(Slot::new(Row::Back, 3)));
        let full_front: Vec<_> = (0..6).map(|c| Slot::new(Row::Front, c)).collect();
        assert_eq!(f.free_slot(&full_front, Row::Front), Some(Slot::new(Row::Back, 3)));
        let active: Vec<_> = f.slots().filter(|s| s.row.is_active()).collect();
        assert_eq!(f.free_slot(&active, Row::Front), Some(Slot::new(Row::Reserve, 3)));
        let all: Vec<_> = f.slots().collect();
        assert_eq!(f.free_slot(&all, Row::Front), None);
        let v = Formation::VANILLA;
        let active: Vec<_> = v.slots().filter(|s| s.row.is_active()).collect();
        assert_eq!(v.free_slot(&active, Row::Front), Some(Slot::new(Row::Reserve, 2)));
    }
}
