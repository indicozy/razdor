//! The trailer's shot file (dev-only, for the trailer's footage): a text file that plays a
//! scripted camera, hero walks, days and events on a map, and is recorded frame-exact
//! (`ui::trailer`). This module is the pure part: the format's parser, the keyframe sampling
//! and easing, and the frame clock. It has no window and no drawing.
//!
//! The format is line based (`docs/trailer-shots.md` has the reference and three examples).
//! Every error names its line and field. Players never see any of this: nothing here is read
//! unless `RAZDOR_TRAILER_SHOT` or `razdor --trailer` asks for it.

use std::collections::BTreeMap;

/// The camera's keyed values of one keyframe; each is optional (absent: the value of the
/// nearest keyframe that sets it holds, see [`sample_values`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Values {
    pub pitch: Option<f32>,
    pub fov: Option<f32>,
    pub dist: Option<f32>,
    pub bend: Option<f32>,
    pub flat: Option<f32>,
    pub radius: Option<f32>,
    pub width: Option<f32>,
    pub ahead: Option<f32>,
    pub behind: Option<f32>,
    pub near: Option<f32>,
    pub yaw: Option<f32>,
    /// The eye's distance as a multiple of the fitted distance (see `view25::Trailer::fit`).
    pub fit: Option<f32>,
    /// The eyes view: `eye` (0 or 1, on from the middle of its segment), the eye's `height` in
    /// cells, and `horizon` (0 or 1: the far ground curves down rather than rolling up).
    pub eye: Option<f32>,
    pub height: Option<f32>,
    pub horizon: Option<f32>,
    /// The flat view's zoom (`view flat` only): 1 is the classic map at any window size.
    pub zoom: Option<f32>,
}

/// Where the camera looks: a map cell, the hero's cell plus an offset in cells, or an army's
/// displayed place (its id as `A<id>` names it) plus an offset in cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Cell(f32, f32),
    Hero(f32, f32),
    Army(u8, f32, f32),
}

/// The easing of a segment, named by the keyframe it ends at (`ease=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Ease {
    Linear,
    In,
    Out,
    #[default]
    InOut,
}

impl Ease {
    /// The eased progress of `u` in `0..=1`.
    pub fn apply(self, u: f32) -> f32 {
        let u = u.clamp(0.0, 1.0);
        match self {
            Ease::Linear => u,
            Ease::In => u * u * u,
            Ease::Out => 1.0 - (1.0 - u).powi(3),
            Ease::InOut => {
                if u < 0.5 {
                    4.0 * u * u * u
                } else {
                    1.0 - (-2.0 * u + 2.0).powi(3) / 2.0
                }
            }
        }
    }
}

/// One camera keyframe: its time, the values it sets, its target and the easing of the segment
/// that ends at it.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub t: f64,
    pub values: Values,
    pub target: Option<Target>,
    pub ease: Ease,
    /// The file's line of the keyframe (its errors name it).
    pub line: usize,
}

/// A world action on the timeline.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// The hero walks the route, cell after cell, at play speed.
    Walk { t: f64, route: Vec<(i32, i32)>, line: usize },
    /// Time passes in real time, as the game's wait does: `hours` game hours, armies march.
    Pass { t: f64, hours: u32, line: usize },
    /// The cells within `radius` of `cell` are explored.
    Reveal { t: f64, cell: (i32, i32), radius: i32, line: usize },
}

impl Action {
    pub fn t(&self) -> f64 {
        match self {
            Action::Walk { t, .. } | Action::Pass { t, .. } | Action::Reveal { t, .. } => *t,
        }
    }
}

/// The map the shot plays on: an install's scenario by its file name, or a save file.
#[derive(Clone, Debug, PartialEq)]
pub enum MapRef {
    Scenario(String),
    Save(String),
}

/// A parsed shot file. Every field has its default (see [`Shot::default`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub map: Option<MapRef>,
    /// The hero's class: `knight`, `archmage` or `ranger`.
    pub hero: String,
    /// The event generator's seed.
    pub seed: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub music: bool,
    /// `true`: a battle is drawn on its screen (`battles show`); `false`: it is fought on the
    /// map and the camera stays on it (`battles map`, the default).
    pub battles_show: bool,
    /// The choice of each event by its id: 0 is the first button (the default), 1 the second.
    pub events: BTreeMap<u32, usize>,
    pub keys: Vec<Key>,
    pub actions: Vec<Action>,
    /// The length in seconds: `duration`, else the last time any line names.
    pub duration: f64,
    /// Whether the file sets `duration` itself.
    pub duration_set: bool,
    /// The clock's hour at the start (`hour`, 0 to 24, fractions allowed); `None`: the map's own.
    pub hour: Option<f64>,
    /// `clock hold`: the light keeps the hour of the start for the whole shot.
    pub clock_hold: bool,
    /// The look settings the run takes over (`look key=value`, in file order; see [`LOOK_KEYS`]).
    /// They are never written to the settings file.
    pub look: Vec<(String, String)>,
    /// `ui on`: the interface is drawn as a player sees it (bottom bar, minimap, the map's widgets).
    /// Event windows are still answered, without being drawn.
    pub ui: bool,
    /// `fog on`: the fog of war (black, or the look's mist) is drawn.
    pub fog: bool,
    /// `view flat`: the classic flat map with its classic camera (`target` and `zoom` only).
    pub flat: bool,
    /// `battle <army id>`: the hero's army fights the army with this id (`--plan` prints the
    /// ids as `A<id>`) at frame 0. `None`: no battle is set up.
    pub battle: Option<u32>,
    /// The header's `battle_speed <x>`: the pace of a battle on its screen (1 is the pace of the
    /// game's animations). `None` without a header line.
    pub battle_speed: Option<SpeedKey>,
    /// The timed `battle_speed <t> <x>` lines, in time order; the pace ramps linearly between
    /// them and holds before the first and after the last.
    pub speed_keys: Vec<SpeedKey>,
}

/// A battle's pace at a time: the header's or a timed line's, with the line it came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedKey {
    pub t: f64,
    pub x: f32,
    /// The file's line of the key (its errors name it).
    pub line: usize,
}

/// The range a battle's pace may take: 1/20 to 8 times the game's animations.
const PACE_RANGE: std::ops::RangeInclusive<f32> = 0.05..=8.0;

impl Default for Shot {
    fn default() -> Shot {
        Shot {
            map: None,
            hero: "knight".into(),
            seed: 0x5EED,
            width: 1920,
            height: 1080,
            fps: 60,
            music: true,
            battles_show: false,
            events: BTreeMap::new(),
            keys: Vec::new(),
            actions: Vec::new(),
            duration: 0.0,
            duration_set: false,
            hour: None,
            clock_hold: false,
            look: Vec::new(),
            ui: false,
            fog: false,
            flat: false,
            battle: None,
            battle_speed: None,
            speed_keys: Vec::new(),
        }
    }
}

/// The look settings a shot may set (`look key=value`), each with the values it takes. `preset` is
/// «Качество графики»'s presets (`original` or `оригинал`, `low`, `medium`, `high`), applied before
/// the other keys. The values are read by `ui::trailer`.
pub const LOOK_KEYS: [(&str, &[&str]); 10] = [
    ("preset", &["original", "оригинал", "low", "medium", "high"]),
    ("lighting", &["off", "on", "cycle", "fixed"]),
    ("shadows", &["painted", "sun"]),
    ("water", &["off", "on"]),
    ("water_rate", &["every", "other", "third"]),
    ("water_quality", &["high", "medium", "low"]),
    ("weather", &["off", "on"]),
    ("living", &["off", "on"]),
    ("mist", &["off", "flat", "clouds"]),
    ("anim", &["on", "near", "off"]),
];

/// The parse errors, one per problem: `line N: field: what is wrong`.
pub type Errors = Vec<String>;

/// A field's error: the field's name and what is wrong with it.
type FieldError = (&'static str, String);

/// Parses a shot file. Errors are collected line by line, so one run shows every mistake.
pub fn parse(text: &str) -> Result<Shot, Errors> {
    let mut shot = Shot::default();
    let mut errors = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line_no = i + 1;
        let body = raw.split('#').next().unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        let mut words = body.split_whitespace();
        let head = words.next().unwrap_or_default();
        let rest: Vec<&str> = words.collect();
        if let Err((field, what)) = apply_line(&mut shot, head, &rest, line_no) {
            errors.push(format!("line {line_no}: {field}: {what}"));
        }
    }
    if shot.map.is_none() {
        errors.push("map: the shot names no map (`map <name>` or `save <file>`)".to_string());
    }
    let last_time = shot.keys.iter().map(|k| k.t).chain(shot.actions.iter().map(Action::t)).fold(0.0, f64::max);
    if !shot.duration_set {
        shot.duration = last_time;
    } else if shot.duration < last_time {
        errors.push(format!("duration: {}s ends before a line at {last_time}s", shot.duration));
    }
    if shot.duration <= 0.0 {
        errors.push("duration: the shot has no time to play (give `duration <seconds>` or a timed line)".to_string());
    }
    shot.actions.sort_by(|a, b| a.t().total_cmp(&b.t()));
    if let Some(first) = shot.battle_speed.iter().chain(shot.speed_keys.first()).next() {
        if !shot.battles_show {
            errors.push(format!("line {}: battle_speed: the pace is for a battle on its screen (`battles show`)", first.line));
        }
    }
    for key in &shot.keys {
        let flat_key = |name: &str| if shot.flat { "zoom" == name || "target" == name } else { name != "zoom" };
        let named = [("zoom", key.values.zoom.is_some())].into_iter().chain(
            [("pitch", key.values.pitch.is_some()), ("fov", key.values.fov.is_some()), ("dist", key.values.dist.is_some()), ("fit", key.values.fit.is_some()), ("bend", key.values.bend.is_some()), ("flat", key.values.flat.is_some()), ("radius", key.values.radius.is_some()), ("width", key.values.width.is_some()), ("ahead", key.values.ahead.is_some()), ("behind", key.values.behind.is_some()), ("near", key.values.near.is_some()), ("yaw", key.values.yaw.is_some()), ("eye", key.values.eye.is_some()), ("height", key.values.height.is_some()), ("horizon", key.values.horizon.is_some())],
        );
        for (name, set) in named {
            if set && !flat_key(name) {
                let why = if shot.flat { "`view flat` takes target and zoom only (`view 25d` for the others)" } else { "zoom is the flat view's (`view flat`); the 2.5D view takes fit" };
                errors.push(format!("line {}: {name}: {why}", key.line));
            }
        }
    }
    if errors.is_empty() {
        Ok(shot)
    } else {
        Err(errors)
    }
}

/// One line of the file, applied to the shot.
fn apply_line(shot: &mut Shot, head: &str, rest: &[&str], line_no: usize) -> Result<(), FieldError> {
    match head {
        "map" => shot.map = Some(MapRef::Scenario(name(rest).map_err(|e| ("map", e))?.trim_end_matches(".DTm").to_string())),
        "save" => shot.map = Some(MapRef::Save(name(rest).map_err(|e| ("save", e))?)),
        "hero" => match one(rest).map_err(|e| ("hero", e))? {
            c @ ("knight" | "archmage" | "ranger") => shot.hero = c.to_string(),
            c => return Err(("hero", format!("`{c}`: knight, archmage or ranger"))),
        },
        "seed" => {
            let v = one(rest).map_err(|e| ("seed", e))?;
            shot.seed = v.parse::<u32>().map_err(|_| ("seed", format!("`{v}`: not a whole number from 0 to 4294967295")))?;
        }
        "size" => {
            let (w, h) = parse_size(one(rest).map_err(|e| ("size", e))?)?;
            shot.width = w;
            shot.height = h;
        }
        "fps" => {
            let v = one(rest).map_err(|e| ("fps", e))?;
            let f: u32 = v.parse().map_err(|_| ("fps", format!("`{v}`: not a whole number")))?;
            if !(1..=120).contains(&f) {
                return Err(("fps", format!("{f}: from 1 to 120")));
            }
            shot.fps = f;
        }
        "music" => match one(rest).map_err(|e| ("music", e))? {
            "on" => shot.music = true,
            "off" => shot.music = false,
            v => return Err(("music", format!("`{v}`: on or off"))),
        },
        "battles" => match one(rest).map_err(|e| ("battles", e))? {
            "map" => shot.battles_show = false,
            "show" => shot.battles_show = true,
            v => return Err(("battles", format!("`{v}`: map or show"))),
        },
        "ui" => shot.ui = on_off(rest, "ui")?,
        "fog" => shot.fog = on_off(rest, "fog")?,
        "view" => match one(rest).map_err(|e| ("view", e))? {
            "flat" => shot.flat = true,
            "25d" => shot.flat = false,
            v => return Err(("view", format!("`{v}`: flat or 25d"))),
        },
        "hour" => {
            let v = one(rest).map_err(|e| ("hour", e))?;
            let h = v.parse::<f64>().ok().filter(|h| h.is_finite() && (0.0..24.0).contains(h));
            shot.hour = Some(h.ok_or(("hour", format!("`{v}`: an hour from 0 to 23 (fractions allowed, e.g. 6.5)")))?);
        }
        "clock" => match one(rest).map_err(|e| ("clock", e))? {
            "run" => shot.clock_hold = false,
            "hold" => shot.clock_hold = true,
            v => return Err(("clock", format!("`{v}`: run or hold"))),
        },
        "look" => {
            if rest.is_empty() {
                return Err(("look", "needs at least one key=value, e.g. `look lighting=on shadows=sun`".to_string()));
            }
            for item in rest {
                shot.look.push(parse_look(item)?);
            }
        }
        "duration" => {
            let v = one(rest).map_err(|e| ("duration", e))?;
            let d = time_word(v).ok_or(("duration", format!("`{v}`: not a time (seconds, e.g. 16 or 16s)")))?;
            if d <= 0.0 {
                return Err(("duration", format!("{d}: must be above 0")));
            }
            shot.duration = d;
            shot.duration_set = true;
        }
        "event" => match rest {
            [id, choice] => match (id.parse::<u32>(), choice.parse::<usize>()) {
                (Ok(id), Ok(c)) => {
                    shot.events.insert(id, c);
                }
                _ => return Err(("event", format!("`event {}`: needs an event number and a choice number", rest.join(" ")))),
            },
            _ => return Err(("event", "needs two words: the event number and the choice (0 is the first button)".to_string())),
        },
        "at" => {
            let key = parse_at(rest, line_no)?;
            if shot.keys.last().is_some_and(|k| k.t == key.t) {
                return Err(("at", format!("{}s: another keyframe has this time", key.t)));
            }
            if shot.keys.last().is_some_and(|k| k.t > key.t) {
                return Err(("at", format!("{}s: keyframes must go in time order", key.t)));
            }
            shot.keys.push(key);
        }
        "walk" => {
            let (t, route) = parse_walk(rest)?;
            shot.actions.push(Action::Walk { t, route, line: line_no });
        }
        "pass" => {
            let (t, hours) = parse_pass(rest)?;
            shot.actions.push(Action::Pass { t, hours, line: line_no });
        }
        "reveal" => {
            let (t, cell, radius) = parse_reveal(rest)?;
            shot.actions.push(Action::Reveal { t, cell, radius, line: line_no });
        }
        "battle" => {
            let v = one(rest).map_err(|e| ("battle", e))?;
            let id: u32 = v.trim_start_matches('A').parse().map_err(|_| ("battle", format!("`{v}`: an army's id (the number --plan prints as A<id>)")))?;
            shot.battle = Some(id);
        }
        "battle_speed" => {
            let key = match rest {
                [x] => SpeedKey { t: 0.0, x: pace(x)?, line: line_no },
                [t, x] => SpeedKey { t: time_word(t).ok_or(("battle_speed", format!("`{t}`: not a time (seconds, e.g. 2 or 2s)")))?, x: pace(x)?, line: line_no },
                _ => return Err(("battle_speed", "write `battle_speed <x>` (the header) or `battle_speed <t> <x>` (a timed pace), e.g. `battle_speed 2s 0.25`".to_string())),
            };
            if rest.len() == 1 {
                shot.battle_speed = Some(key);
            } else {
                if shot.speed_keys.last().is_some_and(|k| k.t == key.t) {
                    return Err(("battle_speed", format!("{}s: another battle_speed line has this time", key.t)));
                }
                if shot.speed_keys.last().is_some_and(|k| k.t > key.t) {
                    return Err(("battle_speed", format!("{}s: battle_speed lines must go in time order", key.t)));
                }
                shot.speed_keys.push(key);
            }
        }
        other => {
            return Err(("line", format!("`{other}`: no such line (map, save, hero, seed, size, fps, music, battles, hour, clock, look, ui, fog, view, duration, event, at, walk, pass, reveal, battle, battle_speed)")));
        }
    }
    Ok(())
}

/// `on` or `off` for a line of one word.
fn on_off(rest: &[&str], field: &'static str) -> Result<bool, FieldError> {
    match one(rest).map_err(|e| (field, e))? {
        "on" => Ok(true),
        "off" => Ok(false),
        v => Err((field, format!("`{v}`: on or off"))),
    }
}

/// One `key=value` of a `look` line, checked against [`LOOK_KEYS`].
fn parse_look(item: &str) -> Result<(String, String), FieldError> {
    let (k, v) = item.split_once('=').ok_or(("look", format!("`{item}`: write key=value")))?;
    let Some((_, values)) = LOOK_KEYS.iter().find(|(name, _)| *name == k) else {
        let names: Vec<&str> = LOOK_KEYS.iter().map(|(n, _)| *n).collect();
        return Err(("look", format!("`{k}`: no such key ({})", names.join(", "))));
    };
    if !values.contains(&v) {
        return Err(("look", format!("`{k}={v}`: {}", values.join(", "))));
    }
    Ok((k.to_string(), v.to_string()))
}

/// A name that may hold spaces (`map Другой берег`): the words joined back.
fn name(rest: &[&str]) -> Result<String, String> {
    if rest.is_empty() {
        return Err("no name given".to_string());
    }
    Ok(rest.join(" "))
}

fn one<'a>(rest: &[&'a str]) -> Result<&'a str, String> {
    match rest {
        [v] => Ok(v),
        [] => Err("no value given".to_string()),
        _ => Err(format!("one value only, got {}", rest.len())),
    }
}

fn parse_size(v: &str) -> Result<(u32, u32), FieldError> {
    let bad = || ("size", format!("`{v}`: write WIDTHxHEIGHT, e.g. 1920x1080"));
    let (w, h) = v.split_once('x').ok_or_else(bad)?;
    let (w, h): (u32, u32) = (w.parse().map_err(|_| bad())?, h.parse().map_err(|_| bad())?);
    if !(16..=5120).contains(&w) || !(16..=2160).contains(&h) || w % 2 != 0 || h % 2 != 0 {
        return Err(("size", format!("{w}x{h}: width 16 to 5120 and height 16 to 2160, both even")));
    }
    Ok((w, h))
}

/// `16`, `16s`, `1.5s`: seconds.
fn time_word(v: &str) -> Option<f64> {
    v.strip_suffix('s').unwrap_or(v).parse::<f64>().ok().filter(|t| t.is_finite() && *t >= 0.0)
}

/// `x,y` as two numbers.
fn pair(v: &str) -> Option<(f32, f32)> {
    let (x, y) = v.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn parse_target(v: &str) -> Result<Target, String> {
    if let Some(off) = v.strip_prefix("hero") {
        if off.is_empty() {
            return Ok(Target::Hero(0.0, 0.0));
        }
        let body = off.strip_prefix('+').unwrap_or(off);
        return pair(body).map(|(x, y)| Target::Hero(x, y)).ok_or_else(|| format!("`{v}`: write hero or hero+dx,dy"));
    }
    if let Some(army) = v.strip_prefix("army:") {
        // `army:<id>` or `army:<id>+dx,dy` (a `-` offset is written as it is: `army:36-1,2`).
        let digits = army.find(|c: char| !c.is_ascii_digit()).unwrap_or(army.len());
        let (id, off) = army.split_at(digits);
        let bad = || format!("`{v}`: write army:<id>, or army:<id>+dx,dy (the id as --plan prints it, without the A)");
        let id: u8 = id.parse().map_err(|_| bad())?;
        let off = off.strip_prefix('+').unwrap_or(off);
        return if off.is_empty() { Ok(Target::Army(id, 0.0, 0.0)) } else { pair(off).map(|(x, y)| Target::Army(id, x, y)).ok_or_else(bad) };
    }
    pair(v).map(|(x, y)| Target::Cell(x, y)).ok_or_else(|| format!("`{v}`: write a cell as x,y, or hero, or hero+dx,dy, or army:<id>, or army:<id>+dx,dy"))
}

/// The camera keys a keyframe may set.
const CAMERA_KEYS: [&str; 16] = ["pitch", "fov", "dist", "fit", "bend", "flat", "radius", "width", "ahead", "behind", "near", "yaw", "eye", "height", "horizon", "zoom"];

fn parse_at(rest: &[&str], line: usize) -> Result<Key, FieldError> {
    let Some((time, items)) = rest.split_first() else {
        return Err(("at", "needs a time first, e.g. `at 16s pitch=12`".to_string()));
    };
    let t = time_word(time).ok_or(("at", format!("`{time}`: not a time (seconds, e.g. 16 or 16s)")))?;
    let mut key = Key { t, values: Values::default(), target: None, ease: Ease::default(), line };
    for item in items {
        let (k, v) = item.split_once('=').ok_or(("at", format!("`{item}`: write key=value")))?;
        match k {
            "target" => key.target = Some(parse_target(v).map_err(|e| ("target", e))?),
            "ease" => {
                key.ease = match v {
                    "linear" => Ease::Linear,
                    "in" => Ease::In,
                    "out" => Ease::Out,
                    "inout" => Ease::InOut,
                    _ => return Err(("ease", format!("`{v}`: linear, in, out or inout"))),
                }
            }
            _ => {
                let Some(&name) = CAMERA_KEYS.iter().find(|&&c| c == k) else {
                    return Err(("at", format!("`{k}`: no such key (pitch, fov, dist, fit, bend, flat, radius, width, ahead, behind, near, yaw, eye, height, horizon, zoom, target, ease)")));
                };
                let n: f32 = v.parse().map_err(|_| (name, format!("`{v}` is not a number")))?;
                check_range(name, n).map_err(|e| (name, e))?;
                *slot(&mut key.values, name) = Some(n);
            }
        }
    }
    Ok(key)
}

fn slot<'a>(v: &'a mut Values, name: &str) -> &'a mut Option<f32> {
    match name {
        "pitch" => &mut v.pitch,
        "fov" => &mut v.fov,
        "dist" => &mut v.dist,
        "fit" => &mut v.fit,
        "zoom" => &mut v.zoom,
        "bend" => &mut v.bend,
        "flat" => &mut v.flat,
        "radius" => &mut v.radius,
        "width" => &mut v.width,
        "ahead" => &mut v.ahead,
        "behind" => &mut v.behind,
        "near" => &mut v.near,
        "eye" => &mut v.eye,
        "height" => &mut v.height,
        "horizon" => &mut v.horizon,
        _ => &mut v.yaw,
    }
}

/// The ranges the camera takes: the pitch past the player's limits, and 90 (the top-down view,
/// rendered as 89.9, where `look_at` is degenerate).
fn check_range(k: &str, n: f32) -> Result<(), String> {
    let ok = match k {
        "pitch" => (0.0..=90.0).contains(&n),
        "fov" => (5.0..=120.0).contains(&n),
        "dist" | "fit" | "radius" | "width" => n > 0.0,
        "zoom" => (0.4..=2.0).contains(&n),
        "bend" => (0.0..=89.0).contains(&n),
        "flat" | "ahead" | "behind" | "near" => n >= 0.0,
        "eye" | "horizon" => (0.0..=1.0).contains(&n),
        "height" => (0.1..=5.0).contains(&n),
        _ => n.is_finite(),
    };
    if ok {
        return Ok(());
    }
    Err(match k {
        "pitch" => format!("{n}: from 0 to 90 (90 is the top-down view, 0 to 5 the eyes view)"),
        "fov" => format!("{n}: from 5 to 120 degrees"),
        "bend" => format!("{n}: from 0 to 89 degrees"),
        "eye" | "horizon" => format!("{n}: 0 or 1"),
        "height" => format!("{n}: from 0.1 to 5 cells"),
        "dist" | "fit" | "radius" | "width" => format!("{n}: must be above 0"),
        "zoom" => format!("{n}: from 0.4 to 2 (1 is the classic map)"),
        _ => format!("{n}: must not be negative"),
    })
}

fn parse_walk(rest: &[&str]) -> Result<(f64, Vec<(i32, i32)>), FieldError> {
    let (time, cells) = rest.split_first().ok_or(("walk", "needs a time and a route of cells".to_string()))?;
    let t = time_word(time).ok_or(("walk", format!("`{time}`: not a time")))?;
    if cells.is_empty() {
        return Err(("walk", "the route has no cell (write x,y x,y ...)".to_string()));
    }
    let mut route = Vec::new();
    for c in cells {
        let (x, y) = pair(c).ok_or(("walk", format!("`{c}`: a cell is x,y")))?;
        route.push((x as i32, y as i32));
    }
    Ok((t, route))
}

fn parse_pass(rest: &[&str]) -> Result<(f64, u32), FieldError> {
    let [time, span] = rest else {
        return Err(("pass", "needs a time and a span, e.g. `pass 4s 3d` or `pass 4s 12h`".to_string()));
    };
    let t = time_word(time).ok_or(("pass", format!("`{time}`: not a time")))?;
    let bad = || ("pass", format!("`{span}`: a span is a number of hours (h) or days (d), e.g. 3d"));
    let (n, unit) = span.split_at(span.len().saturating_sub(1));
    let n: u32 = n.parse().map_err(|_| bad())?;
    let hours = match unit {
        "h" => n,
        "d" => n.saturating_mul(24),
        _ => return Err(bad()),
    };
    if hours == 0 || hours > 24 * 365 {
        return Err(("pass", format!("`{span}`: from 1 hour to 365 days")));
    }
    Ok((t, hours))
}

fn parse_reveal(rest: &[&str]) -> Result<(f64, (i32, i32), i32), FieldError> {
    let [time, cell, radius] = rest else {
        return Err(("reveal", "needs a time, a cell and a radius, e.g. `reveal 2s 12,30 3`".to_string()));
    };
    let t = time_word(time).ok_or(("reveal", format!("`{time}`: not a time")))?;
    let (x, y) = pair(cell).ok_or(("reveal", format!("`{cell}`: a cell is x,y")))?;
    let r: i32 = radius.parse().map_err(|_| ("reveal", format!("`{radius}`: a radius is a whole number of cells")))?;
    if !(0..=40).contains(&r) {
        return Err(("reveal", format!("{r}: a radius from 0 to 40 cells")));
    }
    Ok((t, (x as i32, y as i32), r))
}

/// A battle's pace (`battle_speed`), a number in [`PACE_RANGE`].
fn pace(v: &str) -> Result<f32, FieldError> {
    let x: f32 = v.parse().map_err(|_| ("battle_speed", format!("`{v}` is not a number")))?;
    if !x.is_finite() || !PACE_RANGE.contains(&x) {
        return Err(("battle_speed", format!("{x}: from {} to {} times the animations", PACE_RANGE.start(), PACE_RANGE.end())));
    }
    Ok(x)
}

impl Shot {
    /// The number of frames the shot renders.
    pub fn frame_count(&self) -> u64 {
        frame_count(self.duration, self.fps)
    }

    /// Whether the shot paces its battle (a `battle_speed` line): its battle is then played on
    /// its screen at that pace, not at once (`battles show` without a pace ends it at once).
    pub fn battle_paced(&self) -> bool {
        self.battle_speed.is_some() || !self.speed_keys.is_empty()
    }

    /// The battle's pace at shot time `t`: the header's `x` until a timed line, the timed
    /// lines' ramp between them, the first (or last) pace held before (after) them. `None`
    /// when the shot paces no battle.
    pub fn battle_pace(&self, t: f64) -> Option<f32> {
        let mut points: Vec<(f64, f32)> = self.battle_speed.iter().map(|k| (k.t, k.x)).collect();
        // A timed line at 0 is the start's pace: it takes the header's place.
        points.retain(|p| !self.speed_keys.iter().any(|k| k.t == p.0));
        points.extend(self.speed_keys.iter().map(|k| (k.t, k.x)));
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        pace_at(&points, t)
    }
}

/// The ramp of a battle's pace: linear between the points `(time, pace)` (sorted by time), the
/// first pace held before the first point and the last after the last.
pub fn pace_at(points: &[(f64, f32)], t: f64) -> Option<f32> {
    let first = *points.first()?;
    if t <= first.0 {
        return Some(first.1);
    }
    let last = *points.last()?;
    if t >= last.0 {
        return Some(last.1);
    }
    let i = points.windows(2).position(|w| t >= w[0].0 && t < w[1].0)?;
    let (a, b) = (points[i], points[i + 1]);
    let u = ((t - a.0) / (b.0 - a.0)) as f32;
    Some(a.1 + (b.1 - a.1) * u)
}

/// The frames of a `duration` at `fps`: frame `i` is at time `i / fps`, for `i` below the count.
pub fn frame_count(duration: f64, fps: u32) -> u64 {
    (duration * fps as f64).round() as u64
}

/// The shot's time of frame `i`.
pub fn frame_time(i: u64, fps: u32) -> f64 {
    i as f64 / fps as f64
}

/// The camera's values at time `t`: each is interpolated between the two keyframes that set it,
/// eased by the later one's `ease`; before the first and after the last keyframe that sets it,
/// it holds. `yaw` takes the short way round.
pub fn sample_values(keys: &[Key], t: f64) -> Values {
    let mut out = Values::default();
    let track = |name: &str| sample_scalar(keys.iter().filter_map(|k| slot_value(&k.values, name).map(|v| (k.t, v, k.ease))), t, name == "yaw");
    out.pitch = track("pitch");
    out.fov = track("fov");
    out.dist = track("dist");
    out.fit = track("fit");
    out.bend = track("bend");
    out.flat = track("flat");
    out.radius = track("radius");
    out.width = track("width");
    out.ahead = track("ahead");
    out.behind = track("behind");
    out.near = track("near");
    out.yaw = track("yaw");
    out.eye = track("eye");
    out.height = track("height");
    out.horizon = track("horizon");
    out.zoom = track("zoom");
    out
}

fn slot_value(v: &Values, name: &str) -> Option<f32> {
    let mut copy = *v;
    *slot(&mut copy, name)
}

/// One scalar track: the samples `(time, value, ease of the segment that ends there)`.
fn sample_scalar(samples: impl Iterator<Item = (f64, f32, Ease)>, t: f64, angle: bool) -> Option<f32> {
    let s: Vec<(f64, f32, Ease)> = samples.collect();
    let first = *s.first()?;
    if t <= first.0 {
        return Some(first.1);
    }
    let last = *s.last()?;
    if t >= last.0 {
        return Some(last.1);
    }
    let i = s.windows(2).position(|w| t >= w[0].0 && t < w[1].0)?;
    let (a, b) = (s[i], s[i + 1]);
    let u = ((t - a.0) / (b.0 - a.0)) as f32;
    let e = b.2.apply(u);
    Some(if angle { lerp_angle(a.1, b.1, e) } else { a.1 + (b.1 - a.1) * e })
}

/// Interpolation of an angle in degrees, the short way round (`e` is the eased progress).
pub fn lerp_angle(a: f32, b: f32, e: f32) -> f32 {
    let d = (b - a).rem_euclid(360.0);
    let d = if d > 180.0 { d - 360.0 } else { d };
    a + d * e
}

/// The camera's target at time `t`: the two targets of the segment it lies in and the eased
/// progress from the first to the second. None before any keyframe sets a target; before the
/// first and after the last, the target holds.
pub fn sample_target(keys: &[Key], t: f64) -> Option<(Target, Target, f32)> {
    let s: Vec<(f64, Target, Ease)> = keys.iter().filter_map(|k| k.target.map(|g| (k.t, g, k.ease))).collect();
    let first = *s.first()?;
    if t <= first.0 {
        return Some((first.1, first.1, 0.0));
    }
    let last = *s.last()?;
    if t >= last.0 {
        return Some((last.1, last.1, 0.0));
    }
    let i = s.windows(2).position(|w| t >= w[0].0 && t < w[1].0)?;
    let (a, b) = (s[i], s[i + 1]);
    let u = ((t - a.0) / (b.0 - a.0)) as f32;
    Some((a.1, b.1, b.2.apply(u)))
}

/// The real time a `pass` of `hours` takes, as the game's wait runs it: two wait ticks an hour
/// of [`crate::rules::game::STEP_SECONDS`] each.
pub fn pass_seconds(hours: u32) -> f64 {
    hours as f64 * 2.0 * crate::rules::game::STEP_SECONDS as f64
}

/// One sound the game asked for during a render: a sound effect, or a music track that starts
/// (its file name in `Sounds/`, as `_Sounds.ini` gives it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundEvent {
    Effect(String),
    Music(String),
}

/// The sound log of a render: each event with the frame it starts on, in frame order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoundLog {
    pub events: Vec<(u64, SoundEvent)>,
}

impl SoundLog {
    pub fn push(&mut self, frame: u64, event: SoundEvent) {
        self.events.push((frame, event));
    }

    /// The events as the mixer takes them: the start in seconds (`frame / fps`) and the file.
    pub fn starts(&self, fps: u32) -> Vec<(f64, &SoundEvent)> {
        self.events.iter().map(|(f, e)| (frame_time(*f, fps), e)).collect()
    }
}

/// The mixer's rate and format: 48 kHz, stereo, 16-bit.
pub const MIX_RATE: u32 = 48_000;

/// A clip placed on the mix: its start in seconds, its samples, its gain, and how long it plays
/// at most (`None`: to its end).
pub struct Placed<'a> {
    pub start: f64,
    pub pcm: &'a crate::dt::sound::Pcm,
    pub gain: f32,
    pub max_secs: Option<f64>,
}

/// The clip's samples at [`MIX_RATE`], stereo, as left and right pairs.
fn stereo(pcm: &crate::dt::sound::Pcm) -> Vec<[i32; 2]> {
    let p = pcm.resampled(MIX_RATE);
    let channels = p.channels.max(1) as usize;
    let width = (p.bits.max(8) / 8) as usize;
    let frame = channels * width;
    let sample = |at: usize| -> i32 {
        if width == 2 {
            i16::from_le_bytes([p.data[at], p.data[at + 1]]) as i32
        } else {
            (p.data[at] as i32 - 128) << 8
        }
    };
    let frames = p.data.len() / frame;
    (0..frames)
        .map(|i| {
            let base = i * frame;
            if channels == 1 {
                let s = sample(base);
                [s, s]
            } else {
                [sample(base), sample(base + width)]
            }
        })
        .collect()
}

/// Mixes the clips into one WAV of `total` seconds (48 kHz, stereo, 16-bit): each clip added at
/// its start, scaled by its gain, and the sum clipped to the 16-bit range.
pub fn mix_wav(clips: &[Placed], total: f64) -> Vec<u8> {
    let frames = (total.max(0.0) * MIX_RATE as f64).round() as usize;
    let mut out = vec![0i32; frames * 2];
    for c in clips {
        let samples = stereo(c.pcm);
        let at = (c.start.max(0.0) * MIX_RATE as f64).round() as usize;
        let cap = c.max_secs.map_or(samples.len(), |s| ((s * MIX_RATE as f64).round() as usize).min(samples.len()));
        for (i, s) in samples.iter().take(cap).enumerate() {
            let j = at + i;
            if j >= frames {
                break;
            }
            out[j * 2] += (s[0] as f32 * c.gain) as i32;
            out[j * 2 + 1] += (s[1] as f32 * c.gain) as i32;
        }
    }
    let data_len = (out.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + out.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&MIX_RATE.to_le_bytes());
    wav.extend_from_slice(&(MIX_RATE * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for v in out {
        wav.extend_from_slice(&(v.clamp(i16::MIN as i32, i16::MAX as i32) as i16).to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod eyes_keys {
    use super::*;

    /// `eye`, `height` and `horizon` parse as the camera keys do: they sample between keyframes (a 0/1
    /// key reads its middle as 0.5), and out-of-range values name their item.
    #[test]
    fn the_eye_keys_sample_between_keyframes_and_check_their_ranges() {
        let s = parse("map M\nat 0s eye=0 height=0.7 horizon=0 target=hero\nat 2s eye=1 height=1.2 horizon=1 target=hero\nduration 2\n").expect("parses");
        let v = sample_values(&s.keys, 1.0);
        assert_eq!(v.eye, Some(0.5));
        assert!((v.height.unwrap() - 0.95).abs() < 1e-4, "{v:?}");
        assert_eq!(v.horizon, Some(0.5));
        assert!(parse("map M\nat 0s eye=2\nduration 1\n").is_err());
        assert!(parse("map M\nat 0s height=9\nduration 1\n").is_err());
        assert!(parse("map M\nat 0s horizon=-1\nduration 1\n").is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Shot {
        match parse(text) {
            Ok(s) => s,
            Err(e) => panic!("should parse, got {e:?}"),
        }
    }

    fn errs(text: &str) -> Vec<String> {
        parse(text).expect_err("should fail")
    }

    #[test]
    fn the_defaults_are_the_documented_ones() {
        let s = ok("map РК1-Замок\nat 4s pitch=12\n");
        assert_eq!(s.hero, "knight");
        assert_eq!(s.seed, 0x5EED);
        assert_eq!((s.width, s.height, s.fps), (1920, 1080, 60));
        assert!(s.music);
        assert!(!s.battles_show);
        assert!(s.events.is_empty());
        assert_eq!(s.duration, 4.0, "the duration is the last timed line");
        assert_eq!(s.keys[0].ease, Ease::InOut);
        assert_eq!(s.map, Some(MapRef::Scenario("РК1-Замок".into())));
    }

    #[test]
    fn a_map_name_drops_the_dtm_extension() {
        let s = ok("map Другой берег.DTm\nat 1 pitch=12\n");
        assert_eq!(s.map, Some(MapRef::Scenario("Другой берег".into())));
    }

    #[test]
    fn every_error_names_its_line_and_field() {
        let text = "map РК1\nat 2s pitch=95\nat 3s pitch=12\nat 1s fov=40\nwalk 3s 1,2 x\nfps 0\nhero wizard\nsize 1920x1081\nevent 8\nmusic maybe\nbogus 1";
        let all = errs(text).join("\n");
        assert!(all.contains("line 2: pitch: 95: from 0 to 90"), "{all}");
        assert!(all.contains("line 4: at: 1s: keyframes must go in time order"), "{all}");
        assert!(all.contains("line 5: walk"), "{all}");
        assert!(all.contains("line 6: fps: 0: from 1 to 120"), "{all}");
        assert!(all.contains("line 7: hero"), "{all}");
        assert!(all.contains("line 8: size"), "{all}");
        assert!(all.contains("line 9: event"), "{all}");
        assert!(all.contains("line 10: music"), "{all}");
        assert!(all.contains("line 11: line"), "{all}");
    }

    #[test]
    fn an_unknown_camera_key_is_named() {
        let all = errs("map РК1\nat 2s colour=3\n").join("\n");
        assert!(all.contains("line 2: at: `colour`: no such key"), "{all}");
    }

    #[test]
    fn a_shot_without_a_map_or_time_is_refused() {
        let e = errs("fps 30\n");
        assert!(e.iter().any(|m| m.starts_with("map:")), "{e:?}");
        assert!(e.iter().any(|m| m.starts_with("duration:")), "{e:?}");
    }

    #[test]
    fn the_duration_must_cover_the_timed_lines() {
        let e = errs("map РК1\nduration 3\nat 5s pitch=20\n");
        assert!(e.iter().any(|m| m.contains("ends before a line at 5s")), "{e:?}");
    }

    #[test]
    fn targets_parse_as_cells_heroes_and_offsets() {
        let s = ok("map РК1\nat 1s target=12,30\nat 2s target=hero\nat 3s target=hero+1,-2\n");
        assert_eq!(s.keys[0].target, Some(Target::Cell(12.0, 30.0)));
        assert_eq!(s.keys[1].target, Some(Target::Hero(0.0, 0.0)));
        assert_eq!(s.keys[2].target, Some(Target::Hero(1.0, -2.0)));
        let all = errs("map РК1\nat 1s target=here\n").join("\n");
        assert!(all.contains("line 2: target"), "{all}");
    }

    #[test]
    fn the_actions_are_sorted_and_parsed() {
        let s = ok("map РК1\npass 9s 3d\nwalk 2s 10,10 11,10 12,11\nreveal 1s 10,10 2\nat 20s pitch=12\n");
        assert_eq!(s.actions.iter().map(Action::t).collect::<Vec<_>>(), vec![1.0, 2.0, 9.0]);
        assert_eq!(s.actions[2], Action::Pass { t: 9.0, hours: 72, line: 2 });
        let Action::Walk { route, .. } = &s.actions[1] else { panic!("the walk") };
        assert_eq!(route, &vec![(10, 10), (11, 10), (12, 11)]);
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let s = ok("# the descent\n\nmap РК1 # the start map\n  at 1s pitch=12  # low\n");
        assert_eq!(s.keys.len(), 1);
    }

    #[test]
    fn the_hour_clock_and_look_lines_parse_with_their_defaults() {
        let s = ok("map РК1\nat 1s pitch=12\n");
        assert_eq!((s.hour, s.clock_hold, s.look.len()), (None, false, 0), "the map's own hour, the clock runs, no look");
        let s = ok("map РК1\nhour 6.5\nclock hold\nlook lighting=on shadows=sun\nlook preset=оригинал\nat 1s pitch=12\n");
        assert_eq!(s.hour, Some(6.5));
        assert!(s.clock_hold);
        assert_eq!(s.look, vec![("lighting".to_string(), "on".to_string()), ("shadows".to_string(), "sun".to_string()), ("preset".to_string(), "оригинал".to_string())]);
        assert!(!ok("map РК1\nclock run\nat 1s pitch=12\n").clock_hold);
    }

    #[test]
    fn a_bad_hour_clock_or_look_names_its_line_and_field() {
        let all = errs("map РК1\nhour 24\nhour soon\nclock maybe\nlook colour=3\nlook lighting=maybe\nlook shadows\nlook\nat 1s pitch=12\n").join("\n");
        assert!(all.contains("line 2: hour: `24`: an hour from 0 to 23"), "{all}");
        assert!(all.contains("line 3: hour: `soon`"), "{all}");
        assert!(all.contains("line 4: clock: `maybe`: run or hold"), "{all}");
        assert!(all.contains("line 5: look: `colour`: no such key"), "{all}");
        assert!(all.contains("line 6: look: `lighting=maybe`: off, on, cycle, fixed"), "{all}");
        assert!(all.contains("line 7: look: `shadows`: write key=value"), "{all}");
        assert!(all.contains("line 8: look: needs at least one key=value"), "{all}");
    }

    #[test]
    fn the_ui_fog_and_view_lines_default_to_hidden_and_the_2_5d_view() {
        let s = ok("map РК1\nat 1s pitch=12\n");
        assert_eq!((s.ui, s.fog, s.flat), (false, false, false));
        let s = ok("map РК1\nui on\nfog on\nview flat\nat 0s target=hero zoom=1.25\nduration 1\n");
        assert_eq!((s.ui, s.fog, s.flat), (true, true, true));
        assert_eq!(s.keys[0].values.zoom, Some(1.25));
        assert_eq!(s.keys[0].line, 5, "a key knows its line");
        assert!(!ok("map РК1\nui off\nview 25d\nat 1s pitch=12\n").flat);
    }

    #[test]
    fn a_bad_ui_fog_or_view_names_its_field() {
        let all = errs("map РК1\nui maybe\nfog 1\nview wide\nat 1s pitch=12\n").join("\n");
        assert!(all.contains("line 2: ui: `maybe`: on or off"), "{all}");
        assert!(all.contains("line 3: fog: `1`: on or off"), "{all}");
        assert!(all.contains("line 4: view: `wide`: flat or 25d"), "{all}");
    }

    #[test]
    fn the_flat_view_takes_only_target_and_zoom() {
        let all = errs("map РК1\nview flat\nat 0s pitch=12 target=hero\nat 1s zoom=1 fit=1\n").join("\n");
        assert!(all.contains("line 3: pitch: `view flat` takes target and zoom only"), "{all}");
        assert!(all.contains("line 4: fit: `view flat` takes target and zoom only"), "{all}");
        let all = errs("map РК1\nat 0s zoom=1.2\n").join("\n");
        assert!(all.contains("line 2: zoom: zoom is the flat view's"), "{all}");
        assert!(parse("map РК1\nat 0s zoom=3\nview flat\n").is_err(), "the zoom's range is 0.4 to 2");
    }

    #[test]
    fn the_frame_clock_counts_frames_of_the_fps() {
        assert_eq!(frame_count(16.0, 60), 960);
        assert_eq!(frame_count(16.0, 30), 480);
        assert_eq!(frame_time(60, 60), 1.0);
        assert!((frame_time(1, 25) - 0.04).abs() < 1e-12);
    }

    #[test]
    fn easing_hits_both_ends_and_is_monotonic() {
        for e in [Ease::Linear, Ease::In, Ease::Out, Ease::InOut] {
            assert!(e.apply(0.0).abs() < 1e-6);
            assert!((e.apply(1.0) - 1.0).abs() < 1e-6);
            let mut prev = -1.0;
            for i in 0..=100 {
                let v = e.apply(i as f32 / 100.0);
                assert!(v >= prev - 1e-6, "{e:?} goes back at {i}");
                prev = v;
            }
        }
        assert!(Ease::In.apply(0.5) < 0.5 && Ease::Out.apply(0.5) > 0.5);
        assert!((Ease::InOut.apply(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn values_interpolate_between_their_keys_and_hold_outside() {
        let s = ok("map РК1\nat 0s pitch=90 bend=0\nat 16s pitch=12 bend=78 ease=linear\n");
        let v = sample_values(&s.keys, 8.0);
        assert!((v.pitch.unwrap() - 51.0).abs() < 1e-4);
        assert!((v.bend.unwrap() - 39.0).abs() < 1e-4);
        assert_eq!(sample_values(&s.keys, -1.0).pitch, Some(90.0));
        assert_eq!(sample_values(&s.keys, 99.0).pitch, Some(12.0));
        assert_eq!(sample_values(&s.keys, 8.0).fov, None, "a key nobody sets stays unset");
    }

    #[test]
    fn a_value_holds_until_a_later_key_sets_it() {
        let s = ok("map РК1\nat 0s dist=10\nat 2s pitch=20\nat 4s dist=30 ease=linear\n");
        assert_eq!(sample_values(&s.keys, 1.0).dist, Some(10.0 + 20.0 * 0.25), "dist moves from 0s to 4s");
        assert_eq!(sample_values(&s.keys, 3.0).pitch, Some(20.0));
    }

    #[test]
    fn the_segment_ease_belongs_to_the_later_key() {
        let s = ok("map РК1\nat 0s pitch=10\nat 2s pitch=20 ease=in\n");
        let v = sample_values(&s.keys, 1.0).pitch.unwrap();
        assert!((v - (10.0 + 10.0 * 0.125)).abs() < 1e-4, "cubic in at the middle is 1/8, got {v}");
    }

    #[test]
    fn yaw_turns_the_short_way_round() {
        let near_zero = |v: f32| v.rem_euclid(360.0).min(360.0 - v.rem_euclid(360.0)) < 1e-3;
        assert!(near_zero(lerp_angle(350.0, 10.0, 0.5)), "350 to 10 passes 0");
        assert!(near_zero(lerp_angle(10.0, 350.0, 0.5)), "10 to 350 goes back through 0");
        assert!((lerp_angle(0.0, 90.0, 0.5) - 45.0).abs() < 1e-4);
        assert!((lerp_angle(170.0, -170.0, 0.5) - 180.0).abs() < 1e-3, "170 to 190 is the short way");
        let s = ok("map РК1\nat 0s yaw=350\nat 2s yaw=370 ease=linear\n");
        let mid = sample_values(&s.keys, 1.0).yaw.unwrap().rem_euclid(360.0);
        assert!(mid.min(360.0 - mid) < 1e-3, "350 to 370 passes 0, got {mid}");
    }

    #[test]
    fn targets_blend_between_their_keys() {
        let s = ok("map РК1\nat 0s target=hero\nat 4s target=10,10 ease=linear\n");
        assert_eq!(sample_target(&s.keys, 1.0), Some((Target::Hero(0.0, 0.0), Target::Cell(10.0, 10.0), 0.25)));
        assert_eq!(sample_target(&s.keys, 9.0), Some((Target::Cell(10.0, 10.0), Target::Cell(10.0, 10.0), 0.0)));
        let none = ok("map РК1\nat 1s pitch=12\n");
        assert_eq!(sample_target(&none.keys, 0.5), None);
    }

    fn mono16(samples: &[i16], rate: u32) -> crate::dt::sound::Pcm {
        let data = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        crate::dt::sound::Pcm { rate, channels: 1, bits: 16, data }
    }

    fn sample_at(wav: &[u8], frame: usize) -> (i16, i16) {
        let at = 44 + frame * 4;
        (i16::from_le_bytes([wav[at], wav[at + 1]]), i16::from_le_bytes([wav[at + 2], wav[at + 3]]))
    }

    #[test]
    fn the_sound_log_keeps_frame_order_and_gives_seconds() {
        let mut log = SoundLog::default();
        log.push(0, SoundEvent::Music("a.ogg".into()));
        log.push(90, SoundEvent::Effect("Battle-Fight.wav".into()));
        let starts = log.starts(60);
        assert_eq!(starts[1].0, 1.5);
        assert_eq!(starts[1].1, &SoundEvent::Effect("Battle-Fight.wav".into()));
    }

    #[test]
    fn a_clip_lands_at_its_start_and_is_scaled() {
        let pcm = mono16(&[1000, 2000], MIX_RATE);
        let wav = mix_wav(&[Placed { start: 0.5, pcm: &pcm, gain: 0.5, max_secs: None }], 1.0);
        assert_eq!(wav.len(), 44 + MIX_RATE as usize * 4);
        assert_eq!(sample_at(&wav, 0), (0, 0));
        let at = MIX_RATE as usize / 2;
        assert_eq!(sample_at(&wav, at), (500, 500));
        assert_eq!(sample_at(&wav, at + 1), (1000, 1000));
    }

    #[test]
    fn clips_add_and_clip_at_the_sixteen_bit_range() {
        let loud = mono16(&[30000], MIX_RATE);
        let wav = mix_wav(&[Placed { start: 0.0, pcm: &loud, gain: 1.0, max_secs: None }, Placed { start: 0.0, pcm: &loud, gain: 1.0, max_secs: None }], 0.001);
        assert_eq!(sample_at(&wav, 0), (i16::MAX, i16::MAX), "30000 + 30000 is clipped");
    }

    #[test]
    fn a_clip_can_be_cut_short() {
        let pcm = mono16(&[100, 100, 100, 100], MIX_RATE);
        let wav = mix_wav(&[Placed { start: 0.0, pcm: &pcm, gain: 1.0, max_secs: Some(2.0 / MIX_RATE as f64) }], 0.001);
        assert_eq!(sample_at(&wav, 1), (100, 100));
        assert_eq!(sample_at(&wav, 2), (0, 0), "cut after two frames");
    }

    #[test]
    fn a_mono_clip_at_another_rate_is_resampled_to_the_mix() {
        let pcm = mono16(&[1000, 1000, 1000, 1000], MIX_RATE / 2);
        let wav = mix_wav(&[Placed { start: 0.0, pcm: &pcm, gain: 1.0, max_secs: None }], 0.001);
        assert_eq!(wav.len(), 44 + 48 * 4, "one millisecond at 48 kHz");
        assert_eq!(sample_at(&wav, 0).0, 1000);
    }

    #[test]
    fn pass_spans_take_the_games_wait_pace() {
        assert!((pass_seconds(1) - 0.3).abs() < 1e-6, "two wait ticks of 0.15 s");
        assert!((pass_seconds(72) - 21.6).abs() < 1e-6);
    }

    #[test]
    fn the_battle_lines_default_to_no_battle_and_the_instant_end() {
        let s = ok("map РК1-Начало пути\nduration 4\n");
        assert_eq!(s.battle, None);
        assert_eq!(s.battle_speed, None);
        assert!(s.speed_keys.is_empty());
        assert!(!s.battle_paced());
        assert_eq!(s.battle_pace(1.0), None, "no pace set: the battle is not paced");
    }

    #[test]
    fn a_battle_line_names_the_army_and_a_bad_one_names_its_line() {
        let s = ok("map РК1-Начало пути\nbattle A36\nbattles show\nduration 2\n");
        assert_eq!(s.battle, Some(36));
        assert_eq!(ok("map РК1-Начало пути\nbattle 7\nduration 2\n").battle, Some(7));
        let e = errs("map РК1-Начало пути\nbattle lots\nduration 2\n");
        assert_eq!(e, vec!["line 2: battle: `lots`: an army's id (the number --plan prints as A<id>)".to_string()]);
        assert!(errs("map РК1-Начало пути\nbattle 1 2\nduration 2\n")[0].starts_with("line 2: battle: one value only"));
    }

    #[test]
    fn a_header_pace_and_timed_paces_parse_with_their_lines() {
        let s = ok("map РК1-Начало пути\nbattles show\nbattle_speed 1\nbattle_speed 2s 4\nbattle_speed 4 0.25\nduration 6\n");
        assert_eq!(s.battle_speed.map(|k| (k.t, k.x, k.line)), Some((0.0, 1.0, 3)));
        assert_eq!(s.speed_keys.iter().map(|k| (k.t, k.x, k.line)).collect::<Vec<_>>(), vec![(2.0, 4.0, 4), (4.0, 0.25, 5)]);
        assert!(s.battle_paced());
    }

    #[test]
    fn a_bad_pace_names_its_line_and_field() {
        let e = errs("map РК1-Начало пути\nbattles show\nbattle_speed 9\nduration 2\n");
        assert_eq!(e, vec!["line 3: battle_speed: 9: from 0.05 to 8 times the animations".to_string()]);
        let e = errs("map РК1-Начало пути\nbattles show\nbattle_speed 1s fast\nduration 2\n");
        assert_eq!(e, vec!["line 3: battle_speed: `fast` is not a number".to_string()]);
        let e = errs("map РК1-Начало пути\nbattles show\nbattle_speed x 2\nduration 2\n");
        assert_eq!(e, vec!["line 3: battle_speed: `x`: not a time (seconds, e.g. 2 or 2s)".to_string()]);
        let e = errs("map РК1-Начало пути\nbattles show\nbattle_speed 2s 1\nbattle_speed 2s 2\nduration 4\n");
        assert_eq!(e, vec!["line 4: battle_speed: 2s: another battle_speed line has this time".to_string()]);
        let e = errs("map РК1-Начало пути\nbattles show\nbattle_speed 4s 1\nbattle_speed 2s 2\nduration 4\n");
        assert_eq!(e, vec!["line 4: battle_speed: 2s: battle_speed lines must go in time order".to_string()]);
    }

    #[test]
    fn a_paced_battle_needs_the_battles_show_line() {
        let e = errs("map РК1-Начало пути\nbattle_speed 1\nduration 2\n");
        assert_eq!(e, vec!["line 2: battle_speed: the pace is for a battle on its screen (`battles show`)".to_string()]);
        // The order of the lines does not matter.
        assert!(parse("map РК1-Начало пути\nbattle_speed 1\nbattles show\nduration 2\n").is_ok());
    }

    #[test]
    fn the_pace_ramps_between_its_lines_and_holds_at_the_ends() {
        let s = ok("map РК1-Начало пути\nbattles show\nbattle_speed 1\nbattle_speed 2s 4\nbattle_speed 4s 0.25\nduration 6\n");
        assert_eq!(s.battle_pace(0.0), Some(1.0));
        assert_eq!(s.battle_pace(1.0), Some(2.5), "halfway from 1 to 4");
        assert_eq!(s.battle_pace(2.0), Some(4.0));
        assert_eq!(s.battle_pace(3.0), Some(2.125));
        assert_eq!(s.battle_pace(5.0), Some(0.25), "held after the last line");
    }

    #[test]
    fn a_timed_pace_at_zero_takes_the_headers_place() {
        let s = ok("map РК1-Начало пути\nbattles show\nbattle_speed 1\nbattle_speed 0s 0.5\nbattle_speed 2s 2\nduration 4\n");
        assert_eq!(s.battle_pace(0.0), Some(0.5));
        assert_eq!(s.battle_pace(1.0), Some(1.25));
        let only_timed = ok("map РК1-Начало пути\nbattles show\nbattle_speed 2s 3\nduration 4\n");
        assert_eq!(only_timed.battle_pace(0.0), Some(3.0), "held before the first line");
        assert_eq!(only_timed.battle_pace(3.0), Some(3.0));
    }

    #[test]
    fn a_battle_line_with_no_pace_is_not_paced() {
        let s = ok("map РК1-Начало пути\nbattles show\nbattle %s\nduration 2\n".replace("%s", "A3").as_str());
        assert_eq!(s.battle, Some(3));
        assert!(!s.battle_paced(), "the instant end stays the default");
    }

    #[test]
    fn a_target_can_follow_an_army_with_an_offset() {
        let s = ok("map РК1-Начало пути\nat 0s target=army:36\nat 2s target=army:36+1,-2\nat 4s target=army:7-3,0\nduration 4\n");
        assert_eq!(s.keys[0].target, Some(Target::Army(36, 0.0, 0.0)));
        assert_eq!(s.keys[1].target, Some(Target::Army(36, 1.0, -2.0)));
        assert_eq!(s.keys[2].target, Some(Target::Army(7, -3.0, 0.0)));
        let e = errs("map РК1-Начало пути\nat 0s target=army:\nduration 2\n");
        assert_eq!(e, vec!["line 2: target: `army:`: write army:<id>, or army:<id>+dx,dy (the id as --plan prints it, without the A)".to_string()]);
        let e = errs("map РК1-Начало пути\nat 0s target=army:A36\nduration 2\n");
        assert!(e[0].starts_with("line 2: target: `army:A36`"), "{e:?}");
        let e = errs("map РК1-Начало пути\nat 0s target=army:300\nduration 2\n");
        assert!(e[0].starts_with("line 2: target: `army:300`"), "{e:?}");
    }

    #[test]
    fn an_army_target_is_sampled_between_its_keys_like_a_cell() {
        let s = ok("map РК1-Начало пути\nat 0s target=army:36\nat 4s target=army:36+2,0\nduration 4\n");
        assert_eq!(sample_target(&s.keys, 2.0), Some((Target::Army(36, 0.0, 0.0), Target::Army(36, 2.0, 0.0), 0.5)));
    }
}
