//! The «Дерево развития» window (the owner's approved design, 2026-11-17): a
//! read-only window over the army screen (the promotion pane's expand button). It opens on the unit's line: a vertical tree with its root at the bottom. The line starts at the
//! highest ancestor-or-self whose subtree has at most `LINE_COLS` leaves; the ancestors above it are a row
//! of small cards under it. A class wider than that (a trunk class) shows its options as rows of cards,
//! each collapsed option a card with a «+N» of the classes under it. «Всё дерево» opens the whole tree as
//! a radial web (zoom, pan, the focus's path gold, the next steps green); a tap there opens that class's
//! line. The stats card of the tapped class sits beside the tree (left on desktop and sideways phones, a
//! band on top of an upright phone). Nothing here promotes: the pane keeps its own promotion click.
//!
//! The graph is the install's `NextUnitN` links, so a mod's tree shows as it is. The layout functions
//! (`graph`, `leaves`, `line_layout`, `trunk_rows`, `line_model`, `radial`) hold no drawing and are unit
//! tested on made-up graphs at the end of this file.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::f32::consts::{PI, TAU};
use std::rc::Rc;

use macroquad::input::MouseButton;
use macroquad::prelude::*;
use razdor::i18n::tr;
use razdor::local;
use razdor::rules::battle::Team;
use razdor::rules::content::{Content, Stat, UnitId};
use razdor::rules::formation::{Row, Slot};
use razdor::rules::game::Game;
use razdor::rules::units::Unit;

use super::assets::Assets;
use super::chrome;
use super::display;
use super::dt_font;
use super::gestures::SwipeKind;
use super::input;
use super::window_layout;
use super::items_view::class_wage;
use super::unit_sheet;
use super::widgets;

/// A line holds at most this many leaves (columns); a class with more is a trunk class.
pub const LINE_COLS: u32 = 7;
/// The classes a walk visits at most (a guard against a huge mod).
const CAP: usize = 400;
/// The trunk view's rows at most (a guard; Evolution's trunk is 6 deep).
const TRUNK_ROWS: usize = 12;

// ---- Pure layout: the graph, the line, the trunk rows, the radial web --------------------------

/// The upgrade graph: each class's options (in data order, each once) and its parents (sorted, so the
/// first is the first-parent edge that the line and the crumbs follow).
#[derive(Debug, Default, Clone)]
pub struct Graph {
    kids: HashMap<u32, Vec<u32>>,
    parents: HashMap<u32, Vec<u32>>,
}

impl Graph {
    /// The graph of `(class, option)` edges, in data order. A repeated edge counts once.
    pub fn from_edges(edges: impl IntoIterator<Item = (u32, u32)>) -> Graph {
        let mut g = Graph::default();
        for (from, to) in edges {
            let k = g.kids.entry(from).or_default();
            if !k.contains(&to) {
                k.push(to);
            }
            let p = g.parents.entry(to).or_default();
            if !p.contains(&from) {
                p.push(from);
            }
        }
        for v in g.parents.values_mut() {
            v.sort_unstable();
        }
        g
    }

    /// The options of `id`, in data order.
    pub fn kids(&self, id: u32) -> &[u32] {
        self.kids.get(&id).map_or(&[], Vec::as_slice)
    }

    /// The parents of `id`, sorted (the first is its first-parent edge).
    fn parents_of(&self, id: u32) -> &[u32] {
        self.parents.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Every class with an option or a parent, sorted, each once.
    pub fn nodes(&self) -> Vec<u32> {
        let mut nodes: Vec<u32> = self.kids.keys().chain(self.parents.keys()).copied().collect();
        nodes.sort_unstable();
        nodes.dedup();
        nodes
    }
}

/// The install's upgrade graph: every `NextUnitN` link whose target is a unit of the install.
pub fn graph(c: &Content) -> Graph {
    Graph::from_edges(c.units.iter().flat_map(|u| {
        u.upgrades.iter().filter_map(|up| up.target).filter(|&t| c.try_unit(UnitId(t)).is_some()).map(move |t| (u.id, t))
    }))
}

/// The classes reachable from `id` (itself included), at most `CAP`.
fn reach(g: &Graph, id: u32) -> HashSet<u32> {
    let mut seen = HashSet::from([id]);
    let mut q = VecDeque::from([id]);
    while let Some(p) = q.pop_front() {
        if seen.len() >= CAP {
            break;
        }
        for &k in g.kids(p) {
            if seen.insert(k) {
                q.push_back(k);
            }
        }
    }
    seen
}

/// The leaf count of every class's subtree: a leaf counts 1; a class whose options only loop back
/// (a cycle with no way out) counts 1.
pub fn leaves(g: &Graph) -> HashMap<u32, u32> {
    g.nodes()
        .into_iter()
        .map(|n| {
            let leaf = reach(g, n).into_iter().filter(|&x| g.kids(x).is_empty()).count() as u32;
            (n, leaf.max(1))
        })
        .collect()
}

/// The first-parent chain from the root down to `id` (root first, `id` last). Cycle-safe.
pub fn chain_to(g: &Graph, id: u32) -> Vec<u32> {
    let mut chain = vec![id];
    let mut cur = id;
    while let Some(&p) = g.parents_of(cur).first() {
        if chain.contains(&p) {
            break;
        }
        chain.push(p);
        cur = p;
    }
    chain.reverse();
    chain
}

/// The class a line starts at: the first class on `id`'s chain (root first) whose leaves fit a line.
/// None when `id` is a trunk class (its own subtree is wider than a line; so are its ancestors').
pub fn line_start(g: &Graph, lv: &HashMap<u32, u32>, id: u32) -> Option<u32> {
    chain_to(g, id).into_iter().find(|c| lv.get(c).copied().unwrap_or(1) <= LINE_COLS)
}

/// One class of a line's tidy tree: its depth below the line's top, its column (leaves get whole
/// columns, a parent sits centred over its options), the index of the card it hangs from.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub id: u32,
    pub depth: usize,
    pub col: f32,
    pub parent: Option<usize>,
}

struct Tidy {
    out: Vec<Placed>,
    seen: HashSet<u32>,
    cols: u32,
}

/// The tidy tree of the subtree of `top` (each class once, at its first parent in the walk) and its
/// number of leaf columns.
pub fn line_layout(g: &Graph, top: u32) -> (Vec<Placed>, u32) {
    let mut t = Tidy { out: Vec::new(), seen: HashSet::new(), cols: 0 };
    tidy_rec(g, top, 0, None, &mut t);
    (t.out, t.cols)
}

fn tidy_rec(g: &Graph, id: u32, depth: usize, parent: Option<usize>, t: &mut Tidy) -> usize {
    let me = t.out.len();
    t.out.push(Placed { id, depth, col: 0.0, parent });
    t.seen.insert(id);
    let mut kid_cols: Vec<f32> = Vec::new();
    for &k in g.kids(id) {
        if t.out.len() >= CAP {
            break;
        }
        if t.seen.contains(&k) {
            continue;
        }
        let ci = tidy_rec(g, k, depth + 1, Some(me), t);
        kid_cols.push(t.out[ci].col);
    }
    let lo = kid_cols.iter().copied().reduce(f32::min);
    let hi = kid_cols.iter().copied().reduce(f32::max);
    t.out[me].col = match (lo, hi) {
        (Some(lo), Some(hi)) => (lo + hi) / 2.0,
        _ => {
            let c = t.cols as f32;
            t.cols += 1;
            c
        }
    };
    me
}

/// One card of a trunk row: its class, its «+N» (the classes under it; 0 = none), and whether its
/// options are the next row (only the first trunk option is).
#[derive(Debug, Clone, PartialEq)]
pub struct TrunkItem {
    pub id: u32,
    pub badge: u32,
    pub expanded: bool,
}

/// The trunk view from `anchor`: row 0 is the anchor, each next row the options of the row's expanded
/// card. An option that fits a line is one card with its «+N»; the first trunk option is expanded
/// into the next row, the other trunk options are collapsed to a «+N» too.
pub fn trunk_rows(g: &Graph, lv: &HashMap<u32, u32>, anchor: u32) -> Vec<Vec<TrunkItem>> {
    let mut rows = vec![vec![TrunkItem { id: anchor, badge: 0, expanded: true }]];
    let mut cur = anchor;
    while rows.len() < TRUNK_ROWS {
        let ks = g.kids(cur);
        if ks.is_empty() {
            break;
        }
        let mut row = Vec::new();
        let mut next = None;
        for &k in ks {
            let trunk = lv.get(&k).copied().unwrap_or(1) > LINE_COLS;
            if trunk && next.is_none() {
                next = Some(k);
                row.push(TrunkItem { id: k, badge: 0, expanded: true });
            } else {
                let under = (reach(g, k).len() as u32).saturating_sub(1);
                row.push(TrunkItem { id: k, badge: under, expanded: false });
            }
        }
        rows.push(row);
        match next {
            Some(k) => cur = k,
            None => break,
        }
    }
    rows
}

/// One card of the line view. `row` counts up from the bottom (the line's top is row 0; a trunk's
/// rows are numbered the same way; the crumbs are row 0 of their own). `col` is the card's column
/// among `cols` columns of its row. `parent` is the card it hangs from (the one below it).
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub id: u32,
    pub badge: u32,
    pub crumb: bool,
    pub row: usize,
    pub col: f32,
    pub cols: u32,
    pub parent: Option<usize>,
}

/// The cards of the line view from `anchor`: the crumbs (one row, root first), then the line (a tidy
/// tree of its subtree) or, for a trunk class, the trunk rows. The first card of the line hangs from the
/// last crumb.
pub fn line_model(g: &Graph, lv: &HashMap<u32, u32>, anchor: u32) -> Vec<Card> {
    let chain = chain_to(g, anchor);
    let (top, crumbs) = match line_start(g, lv, anchor) {
        Some(t) => {
            let at = chain.iter().position(|&x| x == t).unwrap_or(0);
            (Some(t), chain[..at].to_vec())
        }
        None => (None, chain[..chain.len() - 1].to_vec()),
    };
    let ncrumbs = crumbs.len() as u32;
    let mut cards: Vec<Card> = crumbs
        .iter()
        .enumerate()
        .map(|(i, &id)| Card { id, badge: 0, crumb: true, row: 0, col: i as f32, cols: ncrumbs, parent: None })
        .collect();
    let below = if crumbs.is_empty() { None } else { Some(crumbs.len() - 1) };
    match top {
        Some(t) => {
            let (lays, cols) = line_layout(g, t);
            let off = cards.len();
            for lay in lays {
                cards.push(Card {
                    id: lay.id,
                    badge: 0,
                    crumb: false,
                    row: lay.depth,
                    col: lay.col,
                    cols,
                    parent: lay.parent.map(|p| p + off).or(below),
                });
            }
        }
        None => {
            let mut prev = below;
            for (r, row) in trunk_rows(g, lv, anchor).iter().enumerate() {
                let n = row.len() as u32;
                let mut next = None;
                for (i, it) in row.iter().enumerate() {
                    if it.expanded {
                        next = Some(cards.len());
                    }
                    cards.push(Card { id: it.id, badge: it.badge, crumb: false, row: r, col: i as f32, cols: n, parent: prev });
                }
                prev = next;
            }
        }
    }
    cards
}

/// The radial web of a whole tree: rings around a hub (world units, the hub at 0,0). Each class is placed
/// once, on its first-parent edge; the other edges are only drawn (`extra`).
#[derive(Debug, Default, Clone)]
pub struct Radial {
    /// Placed classes, in placement order.
    pub order: Vec<u32>,
    pub pos: HashMap<u32, Vec2>,
    pub ring: HashMap<u32, usize>,
    /// Each placed class's first parent (its tree edge); roots have none.
    pub parent: HashMap<u32, u32>,
    /// Edges between placed classes that are not tree edges.
    pub extra: Vec<(u32, u32)>,
    pub roots: Vec<u32>,
    /// Radius per ring (index = ring; index 0 is the hub).
    pub rings: Vec<f32>,
    pub rmax: f32,
}

/// A class's centre-to-centre spacing on its ring, in world units.
const SPACING: f32 = 1.5;
/// The hub's least ring radius, and the least growth of the rings outward.
const MIN_R1: f32 = 2.0;
const RING_STEP: f32 = 1.9;

/// The angle a class and its subtree need, in radians: its own slot on its ring, or the sum of its
/// children's when that is more. Memoised in `out`.
fn need_of(n: u32, tk: &HashMap<u32, Vec<u32>>, rad: &HashMap<u32, f32>, out: &mut HashMap<u32, f32>) -> f32 {
    if let Some(&v) = out.get(&n) {
        return v;
    }
    let own = SPACING / rad[&n];
    let v = match tk.get(&n) {
        Some(ch) if !ch.is_empty() => {
            let s: f32 = ch.iter().map(|&c| need_of(c, tk, rad, out)).sum();
            own.max(s)
        }
        _ => own,
    };
    out.insert(n, v);
    v
}

/// Gives `n` the middle angle of [a, b] and its children parts of it by their needs.
fn spread(n: u32, a: f32, b: f32, tk: &HashMap<u32, Vec<u32>>, need: &HashMap<u32, f32>, ang: &mut HashMap<u32, f32>) {
    ang.insert(n, (a + b) / 2.0);
    let Some(ch) = tk.get(&n) else { return };
    let total: f32 = ch.iter().map(|c| need[c]).sum();
    let mut at = a;
    for &c in ch {
        let span = (b - a) * need[&c] / total;
        spread(c, at, at + span, tk, need, ang);
        at += span;
    }
}

/// Spreads one ring's sorted angles (in [0, TAU)) so that no two are closer than `gap` radians.
fn pack(x: &mut [f32], gap: f32) {
    let n = x.len();
    if n < 2 || n as f32 * gap > TAU {
        return;
    }
    for _ in 0..200 {
        for i in 0..n - 1 {
            let d = x[i + 1] - x[i];
            if d < gap {
                let e = (gap - d) / 2.0;
                x[i] -= e;
                x[i + 1] += e;
            }
        }
        let d = x[0] + TAU - x[n - 1];
        if d < gap {
            let e = (gap - d) / 2.0;
            x[0] += e;
            x[n - 1] -= e;
        }
    }
}

/// The radial layout of the whole graph: breadth first from the roots (a cycle with no root starts
/// from its lowest class), rings even from ring 1 outward, each class's wedge by its subtree's need.
pub fn radial(g: &Graph) -> Radial {
    let nodes = g.nodes();
    let roots_all: Vec<u32> = nodes.iter().copied().filter(|n| g.parents_of(*n).is_empty() && !g.kids(*n).is_empty()).collect();
    let mut w = Radial::default();
    let mut tk: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut starts = roots_all.into_iter();
    loop {
        if w.order.len() >= CAP {
            break;
        }
        let s = match starts.next() {
            Some(s) => s,
            None => match nodes.iter().copied().find(|n| !w.ring.contains_key(n) && !g.kids(*n).is_empty()) {
                Some(s) => s,
                None => break,
            },
        };
        if w.ring.contains_key(&s) {
            continue;
        }
        w.roots.push(s);
        w.ring.insert(s, 1);
        w.order.push(s);
        let mut q = VecDeque::from([s]);
        while let Some(p) = q.pop_front() {
            for &ch in g.kids(p) {
                if w.order.len() >= CAP {
                    break;
                }
                if w.ring.contains_key(&ch) {
                    continue;
                }
                let r = w.ring[&p] + 1;
                w.ring.insert(ch, r);
                w.parent.insert(ch, p);
                tk.entry(p).or_default().push(ch);
                w.order.push(ch);
                q.push_back(ch);
            }
        }
    }
    let max_d = w.ring.values().copied().max().unwrap_or(0);
    let mut count = vec![0usize; max_d + 1];
    for n in &w.order {
        count[w.ring[n]] += 1;
    }
    let mut rings = vec![0.0; max_d + 1];
    if max_d >= 1 {
        let r1 = (count[1] as f32 * SPACING / TAU).max(MIN_R1);
        let mut step = RING_STEP;
        for (d, &c) in count.iter().enumerate().skip(2) {
            step = step.max((c as f32 * SPACING / TAU - r1) / (d - 1) as f32);
        }
        for (d, r) in rings.iter_mut().enumerate().skip(1) {
            *r = r1 + (d - 1) as f32 * step;
        }
    }
    let rad: HashMap<u32, f32> = w.order.iter().map(|&n| (n, rings[w.ring[&n]])).collect();
    let mut need: HashMap<u32, f32> = HashMap::new();
    for &r in &w.roots {
        need_of(r, &tk, &rad, &mut need);
    }
    let total: f32 = w.roots.iter().map(|r| need[r]).sum();
    let mut ang: HashMap<u32, f32> = HashMap::new();
    let mut at = -PI / 2.0;
    for &r in &w.roots {
        let span = TAU * need[&r] / total;
        spread(r, at, at + span, &tk, &need, &mut ang);
        at += span;
    }
    for (d, &ring_r) in rings.iter().enumerate().skip(1) {
        let mut items: Vec<u32> = w.order.iter().copied().filter(|n| w.ring[n] == d).collect();
        items.sort_by(|a, b| ang[a].rem_euclid(TAU).total_cmp(&ang[b].rem_euclid(TAU)));
        let mut x: Vec<f32> = items.iter().map(|n| ang[n].rem_euclid(TAU)).collect();
        pack(&mut x, SPACING / ring_r);
        for (n, a) in items.iter().zip(x) {
            ang.insert(*n, a);
        }
    }
    for &n in &w.order {
        let r = rings[w.ring[&n]];
        let t = ang[&n];
        w.pos.insert(n, Vec2::new(r * t.cos(), r * t.sin()));
    }
    w.rmax = rings.last().copied().unwrap_or(0.0);
    w.rings = rings;
    for &u in &w.order {
        for &v in g.kids(u) {
            if w.ring.contains_key(&v) && w.parent.get(&v) != Some(&u) {
                w.extra.push((u, v));
            }
        }
    }
    w
}

/// The focus's tree chain, the focus first, up to its root (empty if not placed).
fn path_to(w: &Radial, id: u32) -> Vec<u32> {
    let mut out = Vec::new();
    if !w.pos.contains_key(&id) {
        return out;
    }
    let mut cur = id;
    loop {
        out.push(cur);
        match w.parent.get(&cur) {
            Some(&p) => cur = p,
            None => break,
        }
    }
    out
}

/// A name in at most two lines of `w` px at `size`: it breaks after a space or a hyphen; a word too
/// wide for its line is cut with «…».
fn name_lines(name: &str, w: f32, size: f32) -> Vec<String> {
    let mut segs: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in name.chars() {
        cur.push(ch);
        if ch == ' ' || ch == '-' {
            segs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        segs.push(cur);
    }
    let mut first = String::new();
    let mut i = 0;
    while i < segs.len() {
        let cand = format!("{first}{}", segs[i].trim_end());
        if !first.is_empty() && widgets::measure(&cand, size).width > w {
            break;
        }
        first.push_str(&segs[i]);
        i += 1;
    }
    let mut out = vec![cut_to(first.trim_end(), w, size)];
    if i < segs.len() {
        let rest: String = segs[i..].concat();
        out.push(cut_to(rest.trim(), w, size));
    }
    out
}

/// `s` cut to fit `w` px at `size`, with «…» where it was cut.
fn cut_to(s: &str, w: f32, size: f32) -> String {
    if widgets::measure(s, size).width <= w {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while chars.pop().is_some() {
        let cut: String = chars.iter().collect();
        let t = format!("{}…", cut.trim_end());
        if widgets::measure(&t, size).width <= w {
            return t;
        }
    }
    "…".to_string()
}

// ---- The window's state -------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Open {
    /// The squad member whose tree it is (index 0 is the hero, who never promotes).
    member: usize,
}

/// The install's graph of the open window, built once at open (a window is open within one game).
struct Model {
    g: Graph,
    lv: HashMap<u32, u32>,
    radial: Radial,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Line,
    Web,
}

#[derive(Default)]
struct Pan {
    init: bool,
    center: Vec2,
    zoom: f32,
    target: Option<Vec2>,
    press: Option<Press>,
}

#[derive(Clone, Copy)]
struct Press {
    start: Vec2,
    last: Vec2,
    dragging: bool,
}

local! {
    /// The open window (None: closed) and the model it shows.
    static OPEN: Cell<Option<Open>> = const { Cell::new(None) };
    static MODEL: RefCell<Option<Rc<Model>>> = RefCell::new(None);
    /// The tapped class: the stats card's, the gold frame's and the line's focus.
    static FOCUS: Cell<u32> = const { Cell::new(0) };
    static MODE: Cell<Mode> = const { Cell::new(Mode::Line) };
    /// The line view's re-rooted top class (a tap on a «+N» card or a crumb); None: the focus's line.
    static VIEW_TOP: Cell<Option<u32>> = const { Cell::new(None) };
    /// The line view's scroll in px (the content's top moved up) and whether it is set for this open.
    static SCROLL: Cell<f32> = const { Cell::new(0.0) };
    static SCROLL_INIT: Cell<bool> = const { Cell::new(false) };
    /// The line view's mouse press (a drag scrolls, a release without one selects).
    static LINE_PRESS: Cell<Option<Press>> = const { Cell::new(None) };
    /// The web's view: centre and zoom, a pending ease, a press.
    static PAN: RefCell<Pan> = RefCell::new(Pan::default());
    /// The next web frame is set to the fit zoom (entered by «Всё дерево», or opened in the web state).
    static WEB_FIT: Cell<bool> = const { Cell::new(false) };
}

/// The screen under the window takes no pointer or key while the window is open (`widgets`'s input
/// block). Restores the block on drop.
pub struct InputHold {
    was: bool,
    held: bool,
}

/// Holds the army screen's input while the window is open. Take it at the top of the screen's frame and
/// drop it before [`overlay`], so the window itself takes the input.
pub fn hold_input() -> InputHold {
    let was = widgets::input_blocked();
    let held = OPEN.with(|o| o.get()).is_some();
    if held {
        widgets::set_input_blocked(true);
    }
    InputHold { was, held }
}

impl Drop for InputHold {
    fn drop(&mut self) {
        if self.held {
            widgets::set_input_blocked(self.was);
        }
    }
}

/// Opens the window for `class`, the tree of squad member `member`, on its line (`web`: the whole tree at
/// fit, as «Всё дерево» opens it).
pub fn open(c: &Content, member: usize, class: UnitId, web: bool) {
    let g = graph(c);
    let lv = leaves(&g);
    let rad = radial(&g);
    MODEL.with(|m| *m.borrow_mut() = Some(Rc::new(Model { g, lv, radial: rad })));
    OPEN.with(|o| o.set(Some(Open { member })));
    FOCUS.with(|f| f.set(class.0));
    MODE.with(|m| m.set(if web { Mode::Web } else { Mode::Line }));
    WEB_FIT.with(|w| w.set(web));
    VIEW_TOP.with(|v| v.set(None));
    SCROLL_INIT.with(|s| s.set(false));
    LINE_PRESS.with(|p| p.set(None));
    PAN.with(|p| *p.borrow_mut() = Pan::default());
}

/// Closes the window.
pub fn close() {
    OPEN.with(|o| o.set(None));
    MODEL.with(|m| *m.borrow_mut() = None);
}

/// The line state at the focus's line, scrolled to the focus (a re-root and the scroll reset).
fn to_line() {
    MODE.with(|m| m.set(Mode::Line));
    VIEW_TOP.with(|v| v.set(None));
    SCROLL_INIT.with(|s| s.set(false));
}

// ---- Entry buttons ------------------------------------------------------------------------------

/// The unit's size in px: a phone's dp, else the interface scale.
fn unit_px() -> f32 {
    display::dp_px().unwrap_or_else(chrome::k)
}

/// Phone text of `sp` sp (the desktop has no sp: its interface scale).
fn sp_text(sp: f32) -> f32 {
    match display::dp_px() {
        Some(dp) => sp * dp,
        None => sp * chrome::k(),
    }
}

/// The entry button's side: 44 dp on a phone, 24 px on the desktop.
fn btn_size() -> f32 {
    match display::dp_px() {
        Some(dp) => 44.0 * dp,
        None => 24.0 * chrome::k(),
    }
}

/// A small square button with a tree glyph. True when pressed.
fn icon_button(r: Rect) -> bool {
    let k = chrome::k();
    let pressed = widgets::button_sized(r.x, r.y, r.w, r.h, "", true, r.h * 0.5);
    let col = Color::new(0.95, 0.9, 0.75, 1.0);
    let u = r.w / 6.0;
    let cx = r.x + r.w / 2.0;
    draw_line(cx, r.y + u * 1.5, cx, r.y + u * 2.5, 1.5 * k, col);
    draw_line(r.x + u * 1.5, r.y + u * 2.5, r.x + r.w - u * 1.5, r.y + u * 2.5, 1.5 * k, col);
    for x in [r.x + u * 1.5, cx, r.x + r.w - u * 1.5] {
        draw_rectangle_lines(x - u * 0.6, r.y + u * 4.0 - u * 0.6, u * 1.2, u * 1.2, 1.5 * k, col);
    }
    draw_rectangle_lines(cx - u * 0.6, r.y + u * 1.5 - u * 0.6, u * 1.2, u * 1.2, 1.5 * k, col);
    pressed
}

/// The army pane's expand button, drawn after the pane in the pane's bottom-right corner, inside it and
/// clear of the portraits (the unit's own portrait is centred at the bottom, the top row holds the others):
/// 36 px on the desktop and sideways, 44 dp on a phone held upright. A press on a portrait still promotes.
/// True when pressed.
pub fn pane_button(content: Rect) -> bool {
    let k = chrome::k();
    let s = if window_layout::portrait() { btn_size() } else { 36.0 * k };
    icon_button(Rect::new(content.x + content.w - s - 4.0 * k, content.y + content.h - s - 4.0 * k, s, s))
}

// ---- Drawing ------------------------------------------------------------------------------------

const GOLD: Color = Color::new(1.0, 0.82, 0.3, 1.0);
const GREEN: Color = Color::new(0.35, 1.0, 0.35, 1.0);
const CREAM: Color = Color::new(0.95, 0.92, 0.8, 1.0);
const GREY: Color = Color::new(0.72, 0.72, 0.72, 0.85);
const CRUMB_CREAM: Color = Color::new(0.78, 0.75, 0.66, 1.0);
/// The phone's button text, in sp.
const PHONE_TEXT_SP: f32 = 14.0;
/// Phone names are 12 sp at least (the mobile check's floor).
const LINE_NAME_SP: f32 = 12.0;
/// A card's side: 44 dp on a phone, 60 px (times the interface scale) on the desktop.
const LINE_CARD_DP: f32 = 44.0;
const LINE_CARD_PX: f32 = 60.0;
/// Units: the gap between a card and its name, the minimum gap between columns, the arrow between rows.
const LINE_NAME_GAP: f32 = 3.0;
const LINE_COL_GAP: f32 = 4.0;
const LINE_ARROW: f32 = 22.0;
/// A crumb card's side as a share of a card.
const LINE_CRUMB: f32 = 0.75;
/// The web's zoom: a portrait's side in world units, the roots drawn larger, the hub's radius.
const PORTRAIT: f32 = 1.0;
const ROOT_SCALE: f32 = 1.25;
const HUB_R: f32 = 0.8;
/// Phone names: every class is named from this zoom (dp per world unit) up.
const PHONE_NAME_ZOOM: f32 = 50.0;
/// Below this zoom (dp per world unit) a tap zooms in at the point instead of picking a class.
const PICK_ZOOM: f32 = 24.0;

fn portrait_box(assets: &Assets, id: u32, r: Rect, lit: Option<Color>) {
    draw_rectangle(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0, Color::new(0.0, 0.0, 0.0, 0.6));
    assets.draw_portrait(UnitId(id), Team::Player, r);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
    if let Some(col) = lit {
        chrome::glow_frame(r, col, true);
        // A small portrait keeps a plain 3 px frame too, so the lit ones read at fit.
        if r.w < 24.0 * chrome::k() {
            let k = chrome::k();
            draw_rectangle_lines(r.x - 2.0 * k, r.y - 2.0 * k, r.w + 4.0 * k, r.h + 4.0 * k, 3.0 * k, col);
        }
    }
}

fn polyline(pts: &[Vec2], th: f32, col: Color) {
    for p in pts.windows(2) {
        draw_line(p[0].x, p[0].y, p[1].x, p[1].y, th, col);
    }
}

/// A grey arrow from `from` to `to`, its head at `to`.
fn draw_arrow(from: Vec2, to: Vec2, u: f32) {
    draw_line(from.x, from.y, to.x, to.y, 1.5 * u, GREY);
    let back = (from - to).normalize_or_zero();
    if back == Vec2::ZERO {
        return;
    }
    for sign in [-1.0f32, 1.0] {
        let (sin, cos) = (0.45 * sign).sin_cos();
        let head = Vec2::new(back.x * cos - back.y * sin, back.x * sin + back.y * cos) * 7.0 * u;
        draw_line(to.x, to.y, to.x + head.x, to.y + head.y, 1.5 * u, GREY);
    }
}

fn point_in(a: Rect, p: Vec2) -> bool {
    p.x >= a.x && p.x <= a.x + a.w && p.y >= a.y && p.y <= a.y + a.h
}

/// A press's drag threshold in dp (px on the desktop): the same as the canvas's.
const DRAG_DP: f32 = 8.0;
/// The «выбран» tag's text in sp (the names' floor).
const TAG_SP: f32 = 12.0;
/// A phone's top button («Всё дерево», «К ветке») label: 12 sp (the smallest a label may be). The desktop
/// keeps its own sizes.
const PHONE_TOP_SP: f32 = 12.0;
/// The phone's top button is 27 dp tall, not the 40 dp touch target: the owner's decision (2026-11-21),
/// who ordered its height cut by 30% with its label and width kept. The «[ ]» button is a square as tall.
const PHONE_TOP_H_DP: f32 = 27.0;

/// Whether a press is a drag, so its release selects nothing: it was one, or the pointer has left its
/// place by more than `slop` px, or the content slid under a touch this frame (`slid`: the touch
/// slide scrolled it while the pointer stayed where the swipe began).
fn press_drags(was: bool, travelled: f32, slop: f32, slid: bool) -> bool {
    was || travelled > slop || slid
}

/// The line view's scroll (px, the content's top moved up) when it opens or re-roots: the content's
/// bottom, where the root row is, and then the least that keeps the selected card and its name in view.
/// `focus` is the card's top and bottom in content px, None when it is not in this line. Content no
/// taller than the view does not scroll.
fn opening_scroll(height: f32, view: f32, focus: Option<(f32, f32)>) -> f32 {
    let range = (height - view).max(0.0);
    let mut s = range;
    if let Some((top, bottom)) = focus {
        if bottom > s + view {
            s = bottom - view;
        }
        // The card's top wins when the card is taller than the view.
        if top < s {
            s = top;
        }
    }
    s.clamp(0.0, range)
}

/// The «выбран» tag of the selected unit: a small dark rounded box with light text, centred above its
/// card `card`, kept inside `area` (so on a card at the top edge it may sit over the card's top).
fn selected_tag(area: Rect, card: Rect) {
    let u = unit_px();
    let phone = display::dp_px().is_some();
    let size = sp_text(TAG_SP);
    let txt = tr("selected");
    let tw = dt_font::legacy_metrics_if(phone, || widgets::measure(txt, size).width);
    let (padx, pady) = (4.0 * u, 2.0 * u);
    let (w, h) = (tw + 2.0 * padx, size * 1.25 + 2.0 * pady);
    let x = (card.x + card.w / 2.0 - w / 2.0).min(area.x + area.w - w).max(area.x);
    let y = (card.y - h - 3.0 * u).min(area.y + area.h - h).max(area.y);
    let r = h * 0.35;
    let bg = Color::new(0.05, 0.04, 0.03, 0.96);
    draw_rectangle(x + r, y, w - 2.0 * r, h, bg);
    draw_rectangle(x, y + r, w, h - 2.0 * r, bg);
    for (cx, cy) in [(x + r, y + r), (x + w - r, y + r), (x + r, y + h - r), (x + w - r, y + h - r)] {
        draw_circle(cx, cy, r, bg);
    }
    dt_font::legacy_metrics_if(phone, || {
        chrome::shadow_centered(txt, x + w / 2.0, y + pady + size * 0.9, size, CREAM);
    });
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// The «[ ]» button of the web: fit the whole tree (the start view). The «+» and «−» buttons are gone
/// (the owner's decision, 2026-11-21: pinch and wheel zoom stay). True when pressed.
fn fit_button(r: Rect) -> bool {
    let k = chrome::k();
    // Razdor's square: the game has no button art of this shape, so it is made as the game's own buttons are made.
    let pressed = widgets::button_composed(r, "", true);
    let col = Color::new(0.95, 0.9, 0.75, 1.0);
    let cx = r.x + r.w / 2.0;
    let cy = r.y + r.h / 2.0;
    let m = r.w * 0.26;
    let arm = m * 0.6;
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let (x, y) = (cx + sx * m, cy + sy * m);
        draw_line(x, y, x - sx * arm, y, 2.0 * k, col);
        draw_line(x, y, x, y - sy * arm, 2.0 * k, col);
    }
    pressed
}

/// The fit button's place: bottom right of the web's area `a`, `s` px square.
fn fit_button_rect(a: Rect, s: f32) -> Rect {
    let k = chrome::k();
    Rect::new(a.x + a.w - s - 6.0 * k, a.y + a.h - s - 8.0 * k, s, s)
}

/// The class whose centre is nearest `at`, within 22 dp (22 px on the desktop).
fn node_at(w: &Radial, ctr: Vec2, center: Vec2, zoom: f32, at: Vec2) -> Option<u32> {
    let reach_px = 22.0 * unit_px();
    let mut best = None;
    let mut best_d = reach_px;
    for &id in &w.order {
        let d = (at - screen(ctr, center, zoom, w.pos[&id])).length();
        if d <= best_d {
            best_d = d;
            best = Some(id);
        }
    }
    best
}

fn screen(ctr: Vec2, center: Vec2, zoom: f32, p: Vec2) -> Vec2 {
    ctr + (p - center) * zoom
}

/// The tree edge from `p` to its child `n` in world units: a radial stub out to the middle radius, an arc
/// along it, a radial stub in (a straight line when the parent has one child).
fn tree_edge(w: &Radial, p: u32, n: u32, straight: bool) -> Vec<Vec2> {
    let (a, b) = (w.pos[&p], w.pos[&n]);
    if straight {
        return vec![a, b];
    }
    let (rp, ap) = (a.length(), a.y.atan2(a.x));
    let (rc, ac) = (b.length(), b.y.atan2(b.x));
    let mut d = ac - ap;
    while d > PI {
        d -= TAU;
    }
    while d < -PI {
        d += TAU;
    }
    if d.abs() < 1e-3 {
        return vec![a, b];
    }
    let rm = (rp + rc) / 2.0;
    let mut out = vec![a];
    for i in 0..=8 {
        let t = ap + d * i as f32 / 8.0;
        out.push(Vec2::new(t.cos(), t.sin()) * rm);
    }
    out.push(b);
    out
}

/// An extra edge as a curve bowed away from the hub, so none crosses the centre.
fn bow(a: Vec2, b: Vec2, bend: f32, n: usize) -> Vec<Vec2> {
    let mid = (a + b) / 2.0;
    let len = (b - a).length();
    let mut dir = mid.normalize_or_zero();
    if dir == Vec2::ZERO {
        let d = b - a;
        dir = Vec2::new(-d.y, d.x).normalize_or_zero();
    }
    let ctrl = mid + dir * bend * len;
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let s = 1.0 - t;
            a * (s * s) + ctrl * (2.0 * s * t) + b * (t * t)
        })
        .collect()
}

/// The open view of the web: the centre between the focus's parent and the middle of its next steps,
/// and the half-extent of what must show.
fn open_view(w: &Radial, g: &Graph, focus: u32) -> Option<(Vec2, f32)> {
    let f = *w.pos.get(&focus)?;
    let steps: Vec<Vec2> = g.kids(focus).iter().filter_map(|k| w.pos.get(k).copied()).collect();
    let parent = w.parent.get(&focus).and_then(|p| w.pos.get(p).copied());
    let centroid = if steps.is_empty() { f } else { steps.iter().copied().sum::<Vec2>() / steps.len() as f32 };
    let centre = match parent {
        Some(p) => (p + centroid) / 2.0,
        None => (f + centroid) / 2.0,
    };
    let mut e = (f - centre).length();
    for &p in parent.iter().chain(steps.iter()) {
        e = e.max((p - centre).length());
    }
    Some((centre, e))
}

/// The web in `a`: the hub, the rings, the tree edges (the focus's path gold), the extra lines, the
/// portraits (the unit's own next steps green), the names, the selected unit's «выбран» and the zoom buttons; pan (drag,
/// touch slide) and zoom (wheel, buttons). Returns the class tapped.
fn web_view(c: &Content, model: &Model, assets: &Assets, a: Rect, focus: u32, ready: &[u32]) -> Option<u32> {
    let k = chrome::k();
    let u = unit_px();
    let phone = display::dp_px().is_some();
    let (g, w) = (&model.g, &model.radial);
    let path = path_to(w, focus);
    let rmax = w.rmax;
    let fit = a.w.min(a.h) * 0.92 / (2.0 * (rmax + PORTRAIT * ROOT_SCALE * 0.5).max(1.0));
    let zmax = 120.0 * u;
    let zmin = fit.min(zmax);
    let ctr = Vec2::new(a.x + a.w / 2.0, a.y + a.h / 2.0);
    let pointer = Vec2::from(widgets::pointer());
    let inside = point_in(a, pointer);
    let dt = macroquad::time::get_frame_time();
    let wheel = widgets::wheel();
    let (tx, ty) = input::map_pan();
    input::swipe_area(a, SwipeKind::Pan);
    // The «[ ]» button: on a phone a square exactly as tall as the top button (27 dp), the owner's decision
    // (2026-11-21); the desktop's 48 px as before.
    let fit_btn = fit_button_rect(a, if phone { PHONE_TOP_H_DP * u } else { 48.0 * u });
    let on_button = point_in(fit_btn, pointer);
    let mut click = None;
    PAN.with(|p| {
        let mut p = p.borrow_mut();
        let p: &mut Pan = &mut p;
        if !p.init {
            p.init = true;
            let (centre, e) = open_view(w, g, focus).unwrap_or((Vec2::ZERO, PORTRAIT));
            // The zoom at which the focus, its parent and its next steps all fit the canvas.
            let pair = 0.9 * a.w.min(a.h) / (2.0 * e + 2.0 * PORTRAIT);
            p.center = centre;
            p.zoom = (if phone { 44.0 * u } else { 64.0 * u }).min(pair);
            p.zoom = p.zoom.clamp(zmin, zmax);
        }
        // A fresh entry to the web (the button, or an open in the web state) starts at the fit zoom.
        if WEB_FIT.with(|f| f.replace(false)) {
            p.center = Vec2::ZERO;
            p.zoom = fit;
            p.target = None;
        }
        // Zoom around the pointer, pan by a drag (mouse) or a slide (touch).
        if inside && wheel != 0.0 {
            let before = p.center + (pointer - ctr) / p.zoom;
            p.zoom = (p.zoom * 1.15f32.powf(wheel)).clamp(zmin, zmax);
            p.center = before - (pointer - ctr) / p.zoom;
        }
        p.center -= Vec2::new(tx, ty) / p.zoom;
        if input::is_mouse_button_pressed(MouseButton::Left) && inside && !on_button {
            p.press = Some(Press { start: pointer, last: pointer, dragging: false });
        }
        if let Some(mut pr) = p.press {
            if input::is_mouse_button_released(MouseButton::Left) {
                if !pr.dragging {
                    click = Some(pr.start);
                }
                p.press = None;
            } else {
                // A tap moves less than 8 dp from its press; past that it is a drag.
                if !pr.dragging && (pointer - pr.start).length() > 8.0 * u {
                    pr.dragging = true;
                }
                if pr.dragging {
                    p.center -= (pointer - pr.last) / p.zoom;
                }
                pr.last = pointer;
                p.press = Some(pr);
            }
        }
        if let Some(t) = p.target {
            p.center = p.center.lerp(t, (dt * 8.0).min(1.0));
            if p.center.distance(t) < 0.005 {
                p.target = None;
            }
        }
        p.center = p.center.clamp(Vec2::splat(-rmax), Vec2::splat(rmax));
    });
    let (mut center, mut zoom) = PAN.with(|p| {
        let p = p.borrow();
        (p.center, p.zoom)
    });
    // A tap on a class picks it. Elsewhere, below PICK_ZOOM a tap zooms in at the point.
    let mut picked = None;
    if let Some(at) = click {
        picked = node_at(w, ctr, center, zoom, at);
        if picked.is_none() && zoom < PICK_ZOOM * u {
            let world = center + (at - ctr) / zoom;
            center = world;
            zoom = (40.0 * u).clamp(zmin, zmax);
            PAN.with(|p| {
                let mut p = p.borrow_mut();
                p.center = world;
                p.zoom = zoom;
                p.target = None;
            });
        }
    }

    let _clip = widgets::Clip::new(a);
    let hub = screen(ctr, center, zoom, Vec2::ZERO);
    for &r in w.rings.iter().skip(1) {
        draw_circle_lines(hub.x, hub.y, r * zoom, 1.0, Color::new(0.6, 0.5, 0.3, 0.10));
    }
    // Extra lines: bowed away from the hub, quieter; brighter when an end is the focus.
    for &(x, y) in &w.extra {
        let lit = x == focus || y == focus;
        let col = Color::new(0.95, 0.45, 0.35, if lit { 0.9 } else { 0.45 });
        let pts: Vec<Vec2> = bow(w.pos[&x], w.pos[&y], 0.3, 10).into_iter().map(|p| screen(ctr, center, zoom, p)).collect();
        polyline(&pts, if lit { 2.0 * k } else { 1.2 * k }, col);
    }
    // Tree lines from each parent; a root hangs off the hub by a thin spoke. The focus's path is gold.
    let mut nkids: HashMap<u32, usize> = HashMap::new();
    for &p in w.parent.values() {
        *nkids.entry(p).or_default() += 1;
    }
    for &n in &w.order {
        let Some(&p) = w.parent.get(&n) else {
            let to = screen(ctr, center, zoom, w.pos[&n]);
            draw_line(hub.x, hub.y, to.x, to.y, 1.0 * k, Color::new(0.75, 0.6, 0.35, 0.25));
            continue;
        };
        let straight = nkids.get(&p).copied().unwrap_or(0) <= 1;
        let pts: Vec<Vec2> = tree_edge(w, p, n, straight).into_iter().map(|q| screen(ctr, center, zoom, q)).collect();
        if path.contains(&n) {
            polyline(&pts, 3.0 * k, GOLD);
        } else {
            polyline(&pts, 1.5 * k, Color::new(0.75, 0.6, 0.35, 0.45));
        }
    }
    // The hub: a small round emblem, no unit.
    draw_circle(hub.x, hub.y, HUB_R * zoom, Color::new(0.12, 0.09, 0.05, 1.0));
    draw_circle_lines(hub.x, hub.y, HUB_R * zoom, 2.0 * k, GOLD);
    draw_circle(hub.x, hub.y, HUB_R * zoom * 0.3, GOLD);

    // Portraits: the unit's own next steps glow; the path lights; the focus is drawn last.
    let on_canvas = |r: Rect| r.x < a.x + a.w && r.x + r.w > a.x && r.y < a.y + a.h && r.y + r.h > a.y;
    let size_of = |id: u32| zoom * PORTRAIT * if w.ring.get(&id) == Some(&1) { ROOT_SCALE } else { 1.0 };
    let rect_of = |id: u32| {
        let s = screen(ctr, center, zoom, w.pos[&id]);
        let z = size_of(id);
        Rect::new(s.x - z / 2.0, s.y - z / 2.0, z, z)
    };
    let lit_of = |id: u32| -> Option<Color> {
        if id == focus {
            Some(GOLD)
        } else if ready.contains(&id) {
            Some(GREEN)
        } else if path.contains(&id) {
            Some(Color::new(1.0, 0.82, 0.3, 0.5))
        } else {
            None
        }
    };
    let mut order: Vec<u32> = w.order.iter().copied().filter(|id| *id != focus && !path.contains(id)).collect();
    order.extend(w.order.iter().copied().filter(|id| *id != focus && path.contains(id)));
    order.push(focus);
    let mut blocked: Vec<(u32, Rect)> = Vec::new();
    for &id in &order {
        if !w.pos.contains_key(&id) {
            continue;
        }
        let r = rect_of(id);
        if on_canvas(r) {
            portrait_box(assets, id, r, lit_of(id));
            blocked.push((id, r));
        }
    }
    // Names: the focus, its path and its next steps always (phones); the others where the zoom makes
    // them readable. A name is drawn only where it hits no other name (and, unless it is one of the
    // focus's own, no other portrait).
    let name_size = if phone { sp_text(PHONE_TEXT_SP) } else { (zoom * 0.28).min(16.0 * u).max(11.0 * k) };
    let others_ok = if phone { zoom >= PHONE_NAME_ZOOM * u } else { zoom * 0.28 >= 11.0 * k };
    let mut priority: Vec<u32> = vec![focus];
    priority.extend(path.iter().copied().filter(|id| *id != focus));
    let extra_steps: Vec<u32> = ready.iter().copied().filter(|id| w.pos.contains_key(id) && !priority.contains(id)).collect();
    priority.extend(extra_steps);
    let rest: Vec<u32> = if others_ok { order.iter().copied().filter(|id| !priority.contains(id)).collect() } else { Vec::new() };
    let mut drawn: Vec<Rect> = Vec::new();
    let can_text = tr("can promote now");
    dt_font::legacy_metrics_if(phone, || {
        let cands = priority.iter().map(|&id| (id, true)).chain(rest.iter().map(|&id| (id, false)));
        for (id, is_priority) in cands {
            if !w.pos.contains_key(&id) {
                continue;
            }
            let r = rect_of(id);
            if !on_canvas(r) {
                continue;
            }
            let name = c.unit(UnitId(id)).name.clone();
            let col = if path.contains(&id) { GOLD } else { CREAM };
            let cx = r.x + r.w / 2.0;
            let top = r.y + r.h + 2.0 * k;
            let size = name_size;
            let text_w = widgets::measure(&name, size).width;
            let bx = Rect::new(cx - text_w / 2.0 - 2.0 * k, top, text_w + 4.0 * k, size * 1.25);
            let clash_name = drawn.iter().any(|d| overlaps(d, &bx));
            let clash_portrait = !is_priority && blocked.iter().any(|(bid, br)| *bid != id && overlaps(br, &bx));
            if clash_name || clash_portrait {
                continue;
            }
            chrome::shadow_centered(&name, cx, top + size * 0.9, size, col);
            drawn.push(bx);
            if ready.contains(&id) {
                let can_size = if phone { size } else { size * 0.85 };
                let can_top = top + size * 1.1;
                let can_w = widgets::measure(can_text, can_size).width;
                let cb = Rect::new(cx - can_w / 2.0 - 2.0 * k, can_top, can_w + 4.0 * k, can_size * 1.25);
                if !drawn.iter().any(|d| overlaps(d, &cb)) {
                    chrome::shadow_centered(can_text, cx, can_top + can_size * 0.9, can_size, GREEN);
                    drawn.push(cb);
                }
            }
        }
    });
    // The selected unit's «выбран» over its portrait (the same tag as the line view's).
    if let Some(focus_rect) = focus_tag_rect(w, focus, rect_of).filter(|r| on_canvas(*r)) {
        selected_tag(a, focus_rect);
    }
    // The zoom buttons: fixed on the canvas, not zoomed.
    // A press changes the view from the next frame on.
    if fit_button(fit_btn) {
        PAN.with(|p| {
            let mut p = p.borrow_mut();
            p.zoom = fit;
            p.center = Vec2::ZERO;
            p.target = None;
        });
    }
    picked
}

/// Where the web's «выбран» tag goes: the focus's portrait, `None` when the focus is not in the web. A class
/// with no promotions is not placed (`radial` places classes with next steps and their own): a squad member
/// of the hero's classes (Рыцарь, Архимаг, Следопыт on «РК1») opens «Всё дерево» without its tag, as its
/// portrait and path are skipped above; reading its place first panicked (`w.pos[&id]`).
fn focus_tag_rect(w: &Radial, focus: u32, rect_of: impl Fn(u32) -> Rect) -> Option<Rect> {
    w.pos.contains_key(&focus).then(|| rect_of(focus))
}

/// A name's card in the line view: its class, its «+N», its place, its size and where its name goes.
struct Node {
    id: u32,
    badge: u32,
    crumb: bool,
    row: usize,
    cx: f32,
    pitch: f32,
    parent: Option<usize>,
    lines: Vec<String>,
    size: f32,
    top: f32,
    tip: f32,
    hit_h: f32,
}

/// Stacks the rows from the bottom: the crumbs, then the tree rows upwards, each name under its card and
/// an arrow's length between two rows. Sets every node's `top`, `tip` and `hit_h`; returns the height.
fn stack(nodes: &mut [Node], lh: f32, u: f32) -> f32 {
    let arrow = LINE_ARROW * u;
    let gap = LINE_NAME_GAP * u;
    let rows = nodes.iter().filter(|n| !n.crumb).map(|n| n.row + 1).max().unwrap_or(0);
    let mut maxl = vec![1usize; rows];
    for n in nodes.iter().filter(|n| !n.crumb) {
        maxl[n.row] = maxl[n.row].max(n.lines.len());
    }
    let crumb_l = nodes.iter().filter(|n| n.crumb).map(|n| n.lines.len()).max().unwrap_or(1);
    let s_tree = nodes.iter().find(|n| !n.crumb).map_or(0.0, |n| n.size);
    // Each block as (the distance from the bottom to its name's lowest edge, to its card's top).
    let mut up = 0.0f32;
    let mut crumb_block = (0.0f32, 0.0f32);
    if let Some(sc) = nodes.iter().find(|n| n.crumb).map(|n| n.size) {
        let top_up = lh * crumb_l as f32 + gap + sc;
        crumb_block = (0.0, top_up);
        up = top_up + arrow;
    }
    let mut tree_block: Vec<(f32, f32)> = Vec::with_capacity(rows);
    for &m in &maxl {
        let top_up = up + lh * m as f32 + gap + s_tree;
        tree_block.push((up, top_up));
        up = top_up + arrow;
    }
    let height = tree_block.iter().map(|b| b.1).fold(crumb_block.1, f32::max);
    for n in nodes.iter_mut() {
        let (ub, top_up, lmax) = if n.crumb {
            (crumb_block.0, crumb_block.1, crumb_l)
        } else {
            let (ub, t) = tree_block[n.row];
            (ub, t, maxl[n.row])
        };
        n.top = height - top_up;
        n.tip = height - (ub + lh * (lmax - n.lines.len()) as f32);
        n.hit_h = n.size + gap + lh * lmax as f32;
    }
    height
}

/// The line view in `a`: the line of the view's top (the focus, or the class a tap re-rooted to) with its
/// crumbs, or the trunk rows; arrows, cards, names, the «+N» badges, the selected unit's «выбран», vertical
/// scroll. Returns the card tapped to select (a «+N» or a crumb re-roots the view instead).
fn line_view(c: &Content, model: &Model, assets: &Assets, a: Rect, focus: u32, ready: &[u32]) -> Option<u32> {
    let u = unit_px();
    let phone = display::dp_px().is_some();
    let anchor = VIEW_TOP.with(|v| v.get()).unwrap_or(focus);
    let pad = 6.0 * u;
    let vp = Rect::new(a.x + pad, a.y + pad, (a.w - 2.0 * pad).max(1.0), (a.h - 2.0 * pad).max(1.0));
    let card0 = if phone { LINE_CARD_DP * u } else { LINE_CARD_PX * u };
    let nsz = sp_text(LINE_NAME_SP);
    let lh = nsz * 1.25;
    let gap = LINE_COL_GAP * u;
    let cards = line_model(&model.g, &model.lv, anchor);
    let (nodes, height) = dt_font::legacy_metrics_if(phone, || {
        let mut nodes: Vec<Node> = cards
            .iter()
            .map(|cd| {
                let pitch = vp.w / cd.cols.max(1) as f32;
                Node {
                    id: cd.id,
                    badge: cd.badge,
                    crumb: cd.crumb,
                    row: cd.row,
                    cx: (cd.col + 0.5) * pitch,
                    pitch,
                    parent: cd.parent,
                    lines: name_lines(&c.unit(UnitId(cd.id)).name, pitch - gap, nsz),
                    size: 0.0,
                    top: 0.0,
                    tip: 0.0,
                    hit_h: 0.0,
                }
            })
            .collect();
        let min_tree = nodes.iter().filter(|n| !n.crumb).map(|n| n.pitch).fold(f32::MAX, f32::min);
        let min_crumb = nodes.iter().filter(|n| n.crumb).map(|n| n.pitch).fold(f32::MAX, f32::min);
        // A card as wide as its column allows (never narrower than its column gap), crumbs smaller.
        let s = card0.min(min_tree - gap).max(gap);
        let sc = (LINE_CRUMB * s).min(min_crumb - gap).max(gap);
        for n in nodes.iter_mut() {
            n.size = if n.crumb { sc } else { s };
        }
        let h = stack(&mut nodes, lh, u);
        (nodes, h)
    });
    let range = (height - vp.h).max(0.0);
    // Short content sits at the bottom (the root grows up); tall content scrolls.
    let base = if height <= vp.h { vp.h - height } else { 0.0 };
    let pointer = Vec2::from(widgets::pointer());
    let inside = point_in(a, pointer);
    if !SCROLL_INIT.with(|s| s.get()) {
        // Open at the bottom, where the root row is (the root grows up), and move only as far as the
        // selected card and its name need to show. A tree that fits shows unscrolled.
        let focus_span = nodes.iter().find(|n| n.id == focus && !n.crumb).map(|n| (n.top, n.top + n.hit_h));
        SCROLL.with(|s| s.set(opening_scroll(height, vp.h, focus_span)));
        SCROLL_INIT.with(|s| s.set(true));
    }
    let mut scroll = SCROLL.with(|s| s.get()).clamp(0.0, range);
    // Wheel and touch slide scroll; a mouse drag scrolls, a release without one is a click.
    let wheel = widgets::wheel();
    if inside && wheel != 0.0 {
        scroll -= wheel * 60.0 * u;
    }
    input::swipe_area(a, SwipeKind::Pan);
    let (_, ty) = input::map_pan();
    scroll -= ty;
    let mut click = None;
    let mut press = LINE_PRESS.with(|p| p.get());
    if input::is_mouse_button_pressed(MouseButton::Left) && inside {
        press = Some(Press { start: pointer, last: pointer, dragging: false });
    }
    if let Some(mut pr) = press {
        if input::is_mouse_button_released(MouseButton::Left) {
            if !pr.dragging {
                click = Some(pr.start);
            }
            press = None;
        } else {
            // A touch slide that moved the content is a drag too, though the pointer stays where it began.
            pr.dragging = press_drags(pr.dragging, (pointer - pr.start).length(), DRAG_DP * u, ty != 0.0);
            if pr.dragging {
                scroll -= pointer.y - pr.last.y;
            }
            pr.last = pointer;
            press = Some(pr);
        }
    }
    LINE_PRESS.with(|p| p.set(press));
    scroll = scroll.clamp(0.0, range);
    SCROLL.with(|s| s.set(scroll));

    let sx = |x: f32| vp.x + x;
    let sy = |y: f32| vp.y + base - scroll + y;
    // A tap: a «+N» card or a crumb re-roots the view to its line; any other card is selected.
    let mut picked = None;
    if let Some(at) = click {
        if let Some(n) = nodes.iter().find(|n| point_in(Rect::new(sx(n.cx) - n.pitch / 2.0, sy(n.top), n.pitch, n.hit_h), at)) {
            if n.badge > 0 || n.crumb {
                VIEW_TOP.with(|v| v.set(Some(n.id)));
                SCROLL_INIT.with(|s| s.set(false));
            } else {
                picked = Some(n.id);
            }
        }
    }

    let _clip = widgets::Clip::new(a);
    for n in &nodes {
        if let Some(p) = n.parent {
            let lo = &nodes[p];
            draw_arrow(Vec2::new(sx(lo.cx), sy(lo.top)), Vec2::new(sx(n.cx), sy(n.tip)), u);
        }
    }
    // The focus is drawn last, so its gold frame is never under a neighbour.
    let mut order: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].id != focus).collect();
    order.extend((0..nodes.len()).filter(|&i| nodes[i].id == focus));
    for &i in &order {
        let n = &nodes[i];
        let r = Rect::new(sx(n.cx) - n.size / 2.0, sy(n.top), n.size, n.size);
        let lit = if n.id == focus {
            Some(GOLD)
        } else if ready.contains(&n.id) {
            Some(GREEN)
        } else {
            None
        };
        portrait_box(assets, n.id, r, lit);
    }
    // Names and the «+N» badges.
    let gap_name = LINE_NAME_GAP * u;
    dt_font::legacy_metrics_if(phone, || {
        for n in &nodes {
            let cx = sx(n.cx);
            let top = sy(n.top);
            let text_top = top + n.size + gap_name;
            let col = if n.id == focus && !n.crumb {
                GOLD
            } else if n.crumb {
                CRUMB_CREAM
            } else {
                CREAM
            };
            for (i, line) in n.lines.iter().enumerate() {
                chrome::shadow_centered(line, cx, text_top + i as f32 * lh + nsz * 0.9, nsz, col);
            }
            if n.badge > 0 {
                let txt = format!("+{}", n.badge);
                let bw = widgets::measure(&txt, nsz).width + 8.0 * u;
                let bh = nsz * 1.25;
                let bx = cx + n.size / 2.0 - bw * 0.6;
                let by = top - bh * 0.35;
                draw_rectangle(bx, by, bw, bh, Color::new(0.05, 0.04, 0.03, 0.96));
                draw_rectangle_lines(bx, by, bw, bh, 1.5 * u, GOLD);
                chrome::shadow_centered(&txt, bx + bw / 2.0, by + nsz * 0.9, nsz, GOLD);
            }
        }
    });
    // The selected card's «выбран», above it.
    if let Some(n) = nodes.iter().find(|n| n.id == focus && !n.crumb) {
        selected_tag(a, Rect::new(sx(n.cx) - n.size / 2.0, sy(n.top), n.size, n.size));
    }
    picked
}

/// The stat card of a class (a fresh level-1 unit): the stacked band of a phone held upright (`band`),
/// else the side card.
fn class_card(c: &Content, assets: &Assets, r: Rect, def: u32, band: bool) {
    let id = UnitId(def);
    let mut unit = Unit::new(c, id, Slot::new(Row::Front, 0));
    unit.heal_full(c);
    let stats = unit.stats(c);
    let name = c.unit(id).name.clone();
    let sheet = unit_sheet::Sheet {
        kind: id,
        name: &name,
        named: false,
        level: unit.level,
        xp: unit.xp,
        need: unit.xp_to_next(c),
        hp: unit.hp,
        now: &stats,
        start: &stats,
        bleed: 0,
        power: stats[Stat::MagicPower],
        wage: class_wage(c, id),
        items: unit.items,
        back_row: false,
        unpaid: false,
        building: 0,
        hero: None,
        status: Vec::new(),
    };
    let mut hover = None;
    if band {
        let _ = unit_sheet::draw_band(assets, c, r, &sheet, false, &mut hover);
    } else {
        let _ = unit_sheet::draw(assets, c, r, &sheet, false, &mut hover);
    }
}

/// Draws the open window over `win` (the army screen's whole window), last of the screen; closes on its own
/// close button or Escape.
pub fn overlay(game: &Game, assets: &Assets, win: Rect) {
    let Some(open) = OPEN.with(|o| o.get()) else { return };
    let Some(model) = MODEL.with(|m| m.borrow().clone()) else { return };
    let c = &game.content;
    // The squad member's own promotion options (`upgrade_tree`, the rule of the pane): green where it can
    // promote now (level 2 or more, not the hero, free).
    let i = open.member;
    let ready: Vec<u32> = match game.squad.get(i) {
        Some(u) => u.upgrade_tree(c, i == 0).into_iter().filter(|t| t.2).map(|t| t.0 .0).collect(),
        None => {
            close();
            return;
        }
    };
    let focus = FOCUS.with(|f| f.get());
    let (inner, close_clicked) = chrome::window(win, tr("Development tree"), chrome::Skin::Marble, true);
    if close_clicked || widgets::key(macroquad::input::KeyCode::Escape) {
        if !close_clicked {
            // Escape is this window's: the screen under it does not see it too.
            widgets::swallow_input();
        }
        close();
        return;
    }
    let k = chrome::k();
    let phone = display::dp_px().is_some();
    let u = unit_px();
    // The stat card goes on top only on an upright phone; a sideways phone holds it on the left, so the
    // tree gets the full height.
    let upright = phone && inner.h > inner.w;
    let (mut card, tree) = if upright {
        let band_h = inner.h * 0.42;
        (
            Rect::new(inner.x + 4.0 * k, inner.y + 4.0 * k, inner.w - 8.0 * k, band_h - 4.0 * k),
            Rect::new(inner.x + 4.0 * k, inner.y + band_h + 4.0 * k, inner.w - 8.0 * k, inner.h - band_h - 8.0 * k),
        )
    } else {
        (
            Rect::new(inner.x + 8.0 * k, inner.y + 8.0 * k, inner.w * 0.36, inner.h - 16.0 * k),
            Rect::new(inner.x + inner.w * 0.4, inner.y + 6.0 * k, inner.w * 0.6 - 8.0 * k, inner.h - 12.0 * k),
        )
    };
    // A next step of the unit: the card says so above its stats.
    if ready.contains(&focus) {
        let bh = if phone { sp_text(PHONE_TEXT_SP) * 1.6 } else { 24.0 * k };
        let banner = Rect::new(card.x, card.y, card.w, bh);
        card = Rect::new(card.x, card.y + bh, card.w, card.h - bh);
        draw_rectangle(banner.x, banner.y, banner.w, banner.h, Color::new(0.04, 0.12, 0.05, 0.9));
        let size = if phone { sp_text(PHONE_TEXT_SP) } else { 14.0 * k };
        dt_font::legacy_metrics_if(phone, || {
            chrome::shadow_centered(tr("can promote now"), banner.x + banner.w / 2.0, banner.y + banner.h / 2.0 + size * 0.35, size, GREEN);
        });
    }
    class_card(c, assets, card, focus, upright);
    // The tree area keeps the window's green marble (as the army window's pane), only a faint shade and
    // a thin dark frame mark it off (a player's request, 2026-11-17: not black).
    draw_rectangle(tree.x - 2.0, tree.y - 2.0, tree.w + 4.0, tree.h + 4.0, Color::new(0.0, 0.05, 0.02, 0.22));
    draw_rectangle_lines(tree.x - 2.0, tree.y - 2.0, tree.w + 4.0, tree.h + 4.0, 1.5 * k, Color::new(0.02, 0.06, 0.03, 0.9));
    // The top strip holds the state switch («Всё дерево» in the line, «К ветке» in the web).
    let bh = if phone { PHONE_TOP_H_DP * u } else { 28.0 * k };
    let strip = Rect::new(tree.x, tree.y, tree.w, (bh + 4.0 * u).min(tree.h));
    let a = Rect::new(tree.x, tree.y + strip.h, tree.w, (tree.h - strip.h).max(1.0));
    let mode = MODE.with(|m| m.get());
    let label = if mode == Mode::Line { tr("Whole tree") } else { tr("Back to the line") };
    let size = if phone { sp_text(PHONE_TOP_SP) } else { 14.0 * k };
    // The label is measured in the button's own role and metrics (as the button draws it), so it is not cut.
    // A phone's label is in the plain face: the decorative button face draws larger at the same size.
    let role = if phone { super::typography::Role::Body } else { super::typography::Role::Button };
    let text_w = role.with(|| dt_font::legacy_metrics_if(true, || widgets::measure(label, size).width));
    // A phone's box keeps 8 dp of room each side of the label; the desktop's 12 px as before.
    let bw = text_w + if phone { 16.0 * u } else { 24.0 * u };
    let top = (strip.x + 4.0 * u, strip.y + 2.0 * u, bw, bh);
    let top_pressed = if phone {
        widgets::button_plain_sized(top.0, top.1, top.2, top.3, label, true, size)
    } else {
        widgets::button_sized(top.0, top.1, top.2, top.3, label, true, size)
    };
    if top_pressed {
        match mode {
            Mode::Line => {
                MODE.with(|m| m.set(Mode::Web));
                WEB_FIT.with(|w| w.set(true));
            }
            Mode::Web => to_line(),
        }
    }
    let mode = MODE.with(|m| m.get());
    let picked = match mode {
        Mode::Line => line_view(c, &model, assets, a, focus, &ready),
        Mode::Web => web_view(c, &model, assets, a, focus, &ready),
    };
    if let Some(id) = picked {
        FOCUS.with(|f| f.set(id));
        // A class tapped in the web: its line opens, scrolled to it.
        if mode == Mode::Web {
            to_line();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whole_tree_of_a_class_outside_it_opens_without_the_selected_tag() {
        // A class with no promotions (the hero's classes on a squad member) is not in the web: «Всё дерево» drew
        // its «выбран» tag by its place and panicked (`no entry found for key`).
        fn rect_of(w: &Radial) -> impl Fn(u32) -> Rect + '_ {
            move |id| {
                let p = w.pos[&id];
                Rect::new(p.x, p.y, 1.0, 1.0)
            }
        }
        let mut w = Radial::default();
        assert_eq!(focus_tag_rect(&w, 7, rect_of(&w)), None);
        // A placed focus keeps its tag.
        w.pos.insert(7, Vec2::new(2.0, 3.0));
        assert_eq!(focus_tag_rect(&w, 7, rect_of(&w)), Some(Rect::new(2.0, 3.0, 1.0, 1.0)));
    }

    fn lv_of(g: &Graph) -> HashMap<u32, u32> {
        leaves(g)
    }

    /// A class with `n` leaf options (a star): `root` → `first..first+n`.
    fn star(root: u32, first: u32, n: u32) -> Vec<(u32, u32)> {
        (0..n).map(|i| (root, first + i)).collect()
    }

    #[test]
    fn empty_graph_has_no_nodes_and_its_class_is_a_line_of_its_own() {
        let g = Graph::default();
        assert!(g.nodes().is_empty());
        assert!(leaves(&g).is_empty());
        assert_eq!(chain_to(&g, 5), vec![5]);
        assert_eq!(line_start(&g, &HashMap::new(), 5), Some(5));
        let w = radial(&g);
        assert!(w.order.is_empty() && w.rings.iter().all(|&r| r == 0.0) && w.rmax == 0.0);
        assert!(line_model(&g, &HashMap::new(), 5).len() == 1);
    }

    #[test]
    fn single_class_with_one_option() {
        let g = Graph::from_edges([(1, 2)]);
        let lv = lv_of(&g);
        assert_eq!(lv[&1], 1);
        assert_eq!(lv[&2], 1);
        assert_eq!(line_start(&g, &lv, 2), Some(1));
        let (lay, cols) = line_layout(&g, 1);
        assert_eq!((lay.len(), cols), (2, 1));
        let w = radial(&g);
        assert_eq!(w.order, vec![1, 2]);
        assert!(w.pos.values().all(|p| p.x.is_finite() && p.y.is_finite()));
        assert!(w.extra.is_empty());
    }

    #[test]
    fn a_cycle_terminates_and_counts_as_one_leaf() {
        let g = Graph::from_edges([(1, 2), (2, 1)]);
        let lv = lv_of(&g);
        assert_eq!(lv[&1], 1, "a cycle with no way out counts 1");
        assert_eq!(chain_to(&g, 1), vec![2, 1], "the cycle is cut where it closes");
        let top = line_start(&g, &lv, 1).expect("a line");
        let (lay, cols) = line_layout(&g, top);
        assert!(lay.len() <= 2 && cols >= 1);
        let w = radial(&g);
        assert_eq!(w.order.len(), 2, "a cycle with no root is placed from its lowest class");
    }

    #[test]
    fn a_self_option_is_not_followed() {
        let g = Graph::from_edges([(3, 3)]);
        let lv = lv_of(&g);
        assert_eq!(lv[&3], 1);
        let (lay, cols) = line_layout(&g, 3);
        assert_eq!((lay.len(), cols), (1, 1));
        let cards = line_model(&g, &lv, 3);
        assert_eq!(cards.len(), 1);
        assert!(cards[0].parent.is_none());
        let w = radial(&g);
        assert_eq!(w.order, vec![3]);
        assert!(w.extra.is_empty() || w.extra.iter().all(|&(a, b)| a == 3 && b == 3));
    }

    #[test]
    fn a_class_with_two_parents_hangs_from_the_first_and_keeps_the_second_as_an_extra_edge() {
        let g = Graph::from_edges([(1, 3), (2, 3)]);
        assert_eq!(chain_to(&g, 3), vec![1, 3]);
        let lv = lv_of(&g);
        let top = line_start(&g, &lv, 3).expect("a line");
        assert_eq!(top, 1);
        let (lay, _) = line_layout(&g, 1);
        assert_eq!(lay.iter().filter(|p| p.id == 3).count(), 1, "each class once");
        let w = radial(&g);
        assert_eq!(w.parent.get(&3), Some(&1));
        assert!(w.extra.contains(&(2, 3)), "the second parent's edge is drawn only");
    }

    #[test]
    fn a_line_starts_at_the_highest_ancestor_that_fits_and_crumbs_hang_under_it() {
        // 0 → 1 → {2..=9}: 1 has 8 leaves (a trunk), so the line starts at 2 with crumbs 0 and 1.
        let mut edges = vec![(0, 1)];
        edges.extend(star(1, 2, 8));
        let g = Graph::from_edges(edges);
        let lv = lv_of(&g);
        assert_eq!(lv[&1], 8);
        assert_eq!(line_start(&g, &lv, 2), Some(2));
        let cards = line_model(&g, &lv, 2);
        let crumbs: Vec<u32> = cards.iter().filter(|c| c.crumb).map(|c| c.id).collect();
        assert_eq!(crumbs, vec![0, 1]);
        let top = cards.iter().position(|c| !c.crumb).expect("the line's top");
        assert_eq!(cards[top].id, 2);
        assert_eq!(cards[top].parent, Some(1), "the line's top hangs from the last crumb");
    }

    #[test]
    fn a_trunk_class_has_no_line_and_its_options_are_rows() {
        // Anchor 1 has 8 leaves: a trunk. Its options 2 and 3 are both trunk classes of 8 leaves.
        let mut edges = star(1, 2, 2);
        edges.extend(star(2, 10, 8));
        edges.extend(star(3, 20, 8));
        let g = Graph::from_edges(edges);
        let lv = lv_of(&g);
        assert_eq!(line_start(&g, &lv, 1), None);
        let rows = trunk_rows(&g, &lv, 1);
        assert_eq!(rows.len(), 3, "anchor, the first trunk option's options");
        assert_eq!(rows[1].len(), 2);
        assert!(rows[1][0].expanded && rows[1][0].id == 2);
        assert!(!rows[1][1].expanded, "the second trunk option is collapsed");
        assert_eq!(rows[1][1].badge, 8, "its «+N» counts the classes under it");
        assert!(rows[2].iter().all(|it| !it.expanded && it.badge == 0), "leaves are cards, no badge");
    }

    #[test]
    fn a_line_option_of_a_trunk_is_one_card_with_its_badge_and_the_trunk_option_expands() {
        // 1 (a trunk: 10 leaves) → 2 (a line: leaves 4 and 5) and 3 (a trunk of 8 leaves).
        let mut edges = star(1, 2, 2);
        edges.extend(star(2, 4, 2));
        edges.extend(star(3, 10, 8));
        let g = Graph::from_edges(edges);
        let lv = lv_of(&g);
        assert_eq!(lv[&1], 10);
        assert!(lv[&2] <= LINE_COLS && lv[&3] > LINE_COLS);
        let rows = trunk_rows(&g, &lv, 1);
        assert_eq!(rows[1], vec![
            TrunkItem { id: 2, badge: 2, expanded: false },
            TrunkItem { id: 3, badge: 0, expanded: true },
        ]);
        assert_eq!(rows[2].len(), 8, "the expanded trunk option's options");
    }

    #[test]
    fn a_wide_root_with_no_parent_is_a_trunk_and_the_radial_web_places_every_class_once() {
        let mut edges = star(1, 2, 12);
        edges.extend(star(2, 20, 3));
        let g = Graph::from_edges(edges);
        let w = radial(&g);
        let mut placed = w.order.clone();
        placed.sort_unstable();
        placed.dedup();
        assert_eq!(placed.len(), w.order.len(), "each class once");
        assert_eq!(w.order.len(), g.nodes().len());
        assert!(w.pos.values().all(|p| p.length() <= w.rmax + 1e-3));
        assert!(w.rings.len() >= 3);
    }

    /// Every class of `g` has a line of at most `LINE_COLS` columns (or is a trunk, from which no line starts
    /// only when its own subtree is wider), and the radial layout places each class once.
    fn check_lines_fit(g: &Graph, c_name: &dyn Fn(u32) -> String) {
        let lv = leaves(g);
        let nodes = g.nodes();
        for &id in &nodes {
            if let Some(top) = line_start(g, &lv, id) {
                let (lay, cols) = line_layout(g, top);
                assert!(cols <= LINE_COLS, "{}: a line of {cols} columns", c_name(id));
                assert!(lay.len() <= CAP, "{}: a line of {} cards", c_name(id), lay.len());
            }
        }
        let w = radial(g);
        let mut placed = w.order.clone();
        placed.sort_unstable();
        placed.dedup();
        assert_eq!(placed.len(), w.order.len(), "each class placed once");
        assert_eq!(w.order.len(), nodes.len(), "every class placed");
    }

    /// The base game's own tree (`RAZDOR_DT_DIR`; skips without it): every class has a line, none wider than
    /// `LINE_COLS` columns, and the radial layout places them all.
    #[test]
    fn real_base_tree_every_class_has_a_line() {
        let Some(dir) = std::env::var_os(razdor::dt::install::ENV_VAR) else { return };
        let dt = razdor::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Content::from_dt(&dt);
        let g = graph(&c);
        check_lines_fit(&g, &|id| format!("class {id}"));
        let lv = leaves(&g);
        for &id in &g.nodes() {
            assert!(line_start(&g, &lv, id).is_some(), "class {id} has no line");
        }
    }

    /// The Evolution mod's tree (`RAZDOR_COMMUNITY_DIR/evolution/install`; skips without it): Бандит (62) has
    /// a line, Нищий (59) is a trunk class (its own subtree is wider than a line), and every line fits.
    #[test]
    fn real_evolution_tree_lines_and_trunks() {
        let Some(dir) = std::env::var_os("RAZDOR_COMMUNITY_DIR") else { return };
        let install = std::path::Path::new(&dir).join("evolution").join("install");
        if !install.is_dir() {
            return;
        }
        let dt = razdor::dt::install::DtInstall::load(&install).expect("the Evolution install loads");
        let c = Content::from_dt(&dt);
        let g = graph(&c);
        let lv = leaves(&g);
        assert_eq!(c.unit(UnitId(62)).name, "Бандит");
        assert_eq!(c.unit(UnitId(59)).name, "Нищий");
        let top = line_start(&g, &lv, 62).expect("Бандит has a line");
        assert!(line_layout(&g, top).1 <= LINE_COLS);
        assert_eq!(line_start(&g, &lv, 59), None, "Нищий is a trunk class");
        assert!(trunk_rows(&g, &lv, 59).len() >= 3, "a trunk view has rows");
        check_lines_fit(&g, &|id| c.unit(UnitId(id)).name.clone());
    }

    #[test]
    fn the_line_of_a_class_cuts_off_at_seven_leaves() {
        // 1 → {2..=7}: 6 leaves (a line). 1 → {2..=8}: 7 leaves (still a line); 8 leaves is a trunk.
        let g7 = Graph::from_edges(star(1, 2, 7));
        assert_eq!(leaves(&g7)[&1], 7);
        assert_eq!(line_start(&g7, &leaves(&g7), 2), Some(1));
        let g8 = Graph::from_edges(star(1, 2, 8));
        assert_eq!(leaves(&g8)[&1], 8);
        assert_eq!(line_start(&g8, &leaves(&g8), 2), Some(2), "the leaf is its own line; its parent is a trunk");
    }

    #[test]
    fn a_line_with_two_leaves_and_a_parent_centres_the_parent_over_them() {
        let g = Graph::from_edges(star(1, 2, 2));
        let (lay, cols) = line_layout(&g, 1);
        assert_eq!(cols, 2);
        let p = lay.iter().find(|p| p.id == 1).expect("top");
        assert!((p.col - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_press_is_a_drag_past_its_slop_or_once_the_content_slides() {
        assert!(!press_drags(false, 3.0, 8.0, false), "a tap that stays still is no drag");
        assert!(press_drags(false, 9.0, 8.0, false), "past the slop it is a drag");
        assert!(press_drags(false, 0.0, 8.0, true), "a touch slide that moved the content is a drag, the pointer staying put");
        assert!(press_drags(true, 0.0, 8.0, false), "a drag stays a drag");
    }

    #[test]
    fn a_tall_line_opens_at_its_root_and_scrolls_only_as_far_as_the_selected_card_needs() {
        // 1000 px of content in a 300 px view: the bottom (the root row) shows at the range, 700.
        assert_eq!(opening_scroll(1000.0, 300.0, None), 700.0, "no selected card in the line: the bottom");
        assert_eq!(opening_scroll(1000.0, 300.0, Some((900.0, 1000.0))), 700.0, "the root selected: still the bottom");
        assert_eq!(opening_scroll(1000.0, 300.0, Some((600.0, 660.0))), 600.0, "a card above the bottom: its top at the top");
        assert_eq!(opening_scroll(1000.0, 300.0, Some((0.0, 40.0))), 0.0, "the leaf at the very top: scrolled to the top");
        assert_eq!(opening_scroll(1000.0, 300.0, Some((100.0, 500.0))), 100.0, "a card taller than the view shows its top");
    }

    #[test]
    fn a_line_that_fits_the_view_does_not_scroll() {
        assert_eq!(opening_scroll(200.0, 300.0, Some((0.0, 50.0))), 0.0);
        assert_eq!(opening_scroll(200.0, 300.0, None), 0.0);
    }
}
