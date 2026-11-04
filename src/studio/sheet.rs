//! The Units and Items spreadsheets of the Данные workspace (UX5): one row per record with its key
//! stats, each the package's override or else the base's value, and its suggested price (the game's
//! formula, `records::Priced`). The rows are read here; the sheet's edits go through `records`, one undo
//! step each. The sort and the filter are the sheet's view of the rows.

use std::cmp::Ordering;

use crate::dt::data::ArtefactType;
use crate::pack::load::Package;

use super::records::{self as rec, Kind, Priced};

/// The sheet's columns. The first one (`Id`) is the frozen column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Id,
    Base,
    /// Units: the experience for the first level-up (the base's value, the format holds no override).
    Xp,
    Hits,
    Attack,
    Defence,
    /// Units: the manoeuvres per turn (the speed the battle core reads).
    Speed,
    /// Items: the artefact's type (the base's).
    Type,
    Cost,
    /// The formula's price (for a unit its tactical cost gives one, for an item its banded price).
    Suggested,
}

/// The columns of a sheet, in order.
pub fn columns(kind: Kind) -> &'static [Column] {
    match kind {
        Kind::Unit => &[Column::Id, Column::Base, Column::Xp, Column::Hits, Column::Attack, Column::Defence, Column::Speed, Column::Cost, Column::Suggested],
        Kind::Item => &[Column::Id, Column::Base, Column::Type, Column::Cost, Column::Suggested],
    }
}

/// The record field a column edits in place (a number the format holds); `None` for the other columns.
pub fn field_of(col: Column) -> Option<&'static str> {
    match col {
        Column::Hits => Some("hits"),
        Column::Attack => Some("attack_blow"),
        Column::Defence => Some("defence_blow"),
        Column::Speed => Some("manevres"),
        Column::Cost => Some("cost"),
        _ => None,
    }
}

/// A cell's value: a number, a text, or nothing to show (no install, or the formula gives none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Num(i64),
    Text(String),
    Empty,
}

impl Cell {
    /// The cell as the sheet shows and edits it.
    pub fn text(&self) -> String {
        match self {
            Cell::Num(n) => n.to_string(),
            Cell::Text(t) => t.clone(),
            Cell::Empty => String::new(),
        }
    }
}

/// One record of the sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub base: String,
    pub file: String,
    pub line: u32,
    /// One cell per column of `columns(kind)`, in order.
    pub cells: Vec<Cell>,
    /// Whether the package overrides the cell (per column): the accent marks an override.
    pub overridden: Vec<bool>,
}

/// The sheet's rows for `kind`, in the package's order. `priced` is the game's prices of the package (`None`
/// without the install: the base's values and the suggested prices are then empty).
pub fn rows(pkg: &Package, mut priced: Option<&mut Priced>, kind: Kind) -> Vec<Row> {
    let cols = columns(kind);
    rec::rows(pkg, kind)
        .into_iter()
        .map(|r| {
            let mut cells = Vec::with_capacity(cols.len());
            let mut overridden = Vec::with_capacity(cols.len());
            for col in cols {
                let (cell, over) = match col {
                    Column::Id => (Cell::Text(r.id.clone()), false),
                    Column::Base => (Cell::Text(r.base.clone()), false),
                    Column::Type => {
                        let t = priced.as_deref().and_then(|p| p.item_def(&r.id)).map(|a| type_name(a.kind));
                        (t.map_or(Cell::Empty, |t| Cell::Text(t.to_string())), false)
                    }
                    Column::Xp => {
                        let xp = priced.as_deref().and_then(|p| p.unit_def(&r.id)).map(|d| d.start_experience as i64);
                        (xp.map_or(Cell::Empty, Cell::Num), false)
                    }
                    Column::Suggested => (suggested(kind, &r.id, priced.as_deref_mut()), false),
                    other => {
                        let key = field_of(*other).unwrap_or("cost");
                        let ov = rec::override_of(pkg, kind, &r.id, key);
                        let base = priced.as_deref().and_then(|p| rec::base_value(pkg, p, kind, &r.id, key));
                        (ov.or(base).map_or(Cell::Empty, Cell::Num), ov.is_some())
                    }
                };
                cells.push(cell);
                overridden.push(over);
            }
            Row { id: r.id, name: r.name, base: r.base, file: r.file, line: r.line, cells, overridden }
        })
        .collect()
}

/// The suggested price of a record: the unit's gold by the game's formula (none when the tactical cost is
/// below 1 or the gold is over 65,535), the item's banded price (none when the formula leaves it alone).
fn suggested(kind: Kind, id: &str, priced: Option<&mut Priced>) -> Cell {
    let Some(p) = priced else { return Cell::Empty };
    match kind {
        Kind::Unit => match p.unit_price(id) {
            Ok(q) if q.tactical >= 1 => q.price.map_or(Cell::Empty, Cell::Num),
            _ => Cell::Empty,
        },
        Kind::Item => p.item_price(id).map_or(Cell::Empty, |q| Cell::Num(q.banded)),
    }
}

/// An artefact type as the sheet names it (the game's own name for the type).
pub fn type_name(t: ArtefactType) -> String {
    format!("{t:?}")
}

/// Sorts the rows by column `col`: ascending or descending. An empty cell sorts last in both directions;
/// equal cells keep the id order.
pub fn sort(rows: &mut [Row], col: usize, desc: bool) {
    rows.sort_by(|a, b| {
        let (x, y) = (&a.cells[col], &b.cells[col]);
        let ord = match (x, y) {
            (Cell::Empty, Cell::Empty) => return a.id.cmp(&b.id),
            (Cell::Empty, _) => return Ordering::Greater,
            (_, Cell::Empty) => return Ordering::Less,
            (Cell::Num(p), Cell::Num(q)) => p.cmp(q),
            (Cell::Text(p), Cell::Text(q)) => p.to_lowercase().cmp(&q.to_lowercase()),
            (p, q) => p.text().cmp(&q.text()),
        };
        let ord = if desc { ord.reverse() } else { ord };
        ord.then_with(|| a.id.cmp(&b.id))
    });
}

/// Whether a row shows for a filter: its id, name or base contains the filter's text (case-insensitive).
/// An empty filter shows every row.
pub fn matches(row: &Row, filter: &str) -> bool {
    let q = filter.trim().to_lowercase();
    q.is_empty() || [&row.id, &row.name, &row.base].iter().any(|s| s.to_lowercase().contains(&q))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, cells: Vec<Cell>) -> Row {
        Row { id: id.into(), name: format!("name of {id}"), base: "dt:x".into(), file: "units/a.toml".into(), line: 1, overridden: vec![false; cells.len()], cells }
    }

    #[test]
    fn a_numeric_sort_is_by_value_with_empty_cells_last_in_both_directions() {
        let mut rows = vec![row("c", vec![Cell::Num(30)]), row("a", vec![Cell::Empty]), row("b", vec![Cell::Num(10)]), row("d", vec![Cell::Num(10)])];
        sort(&mut rows, 0, false);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["b", "d", "c", "a"], "ascending: by value, the tie by id, the empty last");
        sort(&mut rows, 0, true);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["c", "b", "d", "a"], "descending: the empty still last");
    }

    #[test]
    fn a_text_sort_ignores_case() {
        let mut rows = vec![row("x", vec![Cell::Text("beta".into())]), row("y", vec![Cell::Text("Alpha".into())])];
        sort(&mut rows, 0, false);
        assert_eq!(rows[0].id, "y");
    }

    #[test]
    fn the_filter_matches_id_name_or_base_and_an_empty_filter_shows_all() {
        let r = row("ash_knight", vec![Cell::Num(1)]);
        assert!(matches(&r, ""));
        assert!(matches(&r, "KNIGHT"), "case-insensitive on the id");
        assert!(matches(&r, "name of"), "the name too");
        assert!(matches(&r, "dt:x"), "and the base");
        assert!(!matches(&r, "wolf"));
    }

    #[test]
    fn the_columns_put_the_id_first_and_the_suggested_price_last() {
        assert_eq!(columns(Kind::Unit).first(), Some(&Column::Id));
        assert_eq!(columns(Kind::Unit).last(), Some(&Column::Suggested));
        assert_eq!(columns(Kind::Item).last(), Some(&Column::Suggested));
        assert_eq!(field_of(Column::Speed), Some("manevres"));
        assert_eq!(field_of(Column::Suggested), None, "the suggested price is written by its own action");
    }
}
