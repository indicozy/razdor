//! The names the original gives a building it places (docs/reference/editor/main-window.md
//! §10.4, worldgen.md §3.5): lists read at start-up from the editor's language ini
//! (`DTMapEdit_Rus.Ini` of the install, read at runtime), position-seeded draws from them, and
//! the word pairs some types put in front.

use std::path::Path;

use crate::rules::rng::Rng;

/// The editor's language ini in the install.
pub const LANGUAGE_INI: &str = "DTMapEdit_Rus.Ini";

/// The name lists of one ini section: group g (1–15) serves picture type g.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NameLists(pub Vec<Vec<String>>);

impl NameLists {
    pub fn group(&self, g: u8) -> &[String] {
        self.0.get(g as usize).map_or(&[], |v| v.as_slice())
    }
}

/// What the building names come from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NamePools {
    /// `[Names]`: building names.
    pub names: NameLists,
    /// `[Heros]`: owner names.
    pub owners: NameLists,
    /// `[Label]` pairs, "name word/owner word": `Mill`, `Farm`, `Village`, `Settlement`,
    /// `Castle`.
    pub mill: (String, String),
    pub farm: (String, String),
    pub village: (String, String),
    pub settlement: (String, String),
    pub castle: (String, String),
}

/// The lines of an ini text as the editor's reader keeps them (0x4d45b8): every line, trailing
/// spaces cut, blank lines kept.
fn lines(text: &str) -> Vec<&str> {
    text.split('\n').map(|l| l.trim_end_matches(['\r', ' ', '\t'])).collect()
}

/// The lines of section `name`: from the line after its header to the one before the next.
fn section<'a>(lines: &[&'a str], name: &str) -> Vec<&'a str> {
    let header = format!("[{name}]");
    let Some(start) = lines.iter().position(|l| l.trim().eq_ignore_ascii_case(&header)) else { return Vec::new() };
    lines[start + 1..].iter().take_while(|l| !l.trim_start().starts_with('[')).copied().collect()
}

/// A list section (0x59e1b3): read in order, stopping at the first empty line; `#n` (the
/// number in the two characters after `#`) opens group n, 1–15, the rest of that line being
/// a comment; any other line is an entry of the group opened last (before the first `#`,
/// nothing). Duplicates are kept.
fn name_lists(lines: &[&str]) -> NameLists {
    let mut groups = vec![Vec::new(); 16];
    let mut current: Option<usize> = None;
    for l in lines {
        if l.is_empty() {
            break;
        }
        if let Some(rest) = l.strip_prefix('#') {
            let n: String = rest.chars().take(2).collect();
            current = n.trim().parse::<usize>().ok().filter(|g| (1..=15).contains(g));
            continue;
        }
        if let Some(g) = current {
            groups[g].push(l.to_string());
        }
    }
    NameLists(groups)
}

/// `a/b` split at the first `/`; without one the second part is empty.
fn pair(v: &str) -> (String, String) {
    match v.split_once('/') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (v.to_string(), String::new()),
    }
}

impl NamePools {
    /// The pools of the ini text `text` (already decoded).
    pub fn parse(text: &str) -> NamePools {
        let all = lines(text);
        let label = section(&all, "Label");
        let value = |key: &str| {
            label.iter().find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == key).map(|(_, v)| v.trim().to_string())).unwrap_or_default()
        };
        NamePools {
            names: name_lists(&section(&all, "Names")),
            owners: name_lists(&section(&all, "Heros")),
            mill: pair(&value("Mill")),
            farm: pair(&value("Farm")),
            village: pair(&value("Village")),
            settlement: pair(&value("Settlement")),
            castle: pair(&value("Castle")),
        }
    }

    /// The install's editor language ini, if it is there.
    pub fn load(install: &Path) -> Option<NamePools> {
        let path = crate::dt::install::find_path(install, LANGUAGE_INI).ok()?;
        let bytes = std::fs::read(path).ok()?;
        Some(NamePools::parse(&crate::dt::text::decode(&bytes)))
    }
}

/// Entry `i` of a list. The original reads past the end of a list with a list-index error
/// (an empty group); Razdor gives an empty name there.
fn entry(list: &[String], i: i32) -> String {
    usize::try_from(i).ok().and_then(|i| list.get(i)).cloned().unwrap_or_default()
}

/// One draw from a whole list.
fn draw(rng: &mut Rng, list: &[String]) -> String {
    let i = rng.random(list.len() as i32);
    entry(list, i)
}

/// The name and owner name of a new building of picture `(picture_type, variant)` at
/// `(x, y)` (0x5959a0–0x5961f0). The generator is first set to `x·11 + y·7 + variant·3 +
/// picture type`, so a spot always gives the same names; then, by picture type:
/// - village: variant 3 draws one of two pairs (settlement or village), variant 4 one of
///   two (mill or farm), others take the village pair; the pair's first word and a space go
///   before a drawn name, its second before a drawn owner name;
/// - castle: a drawn name and owner name; the owner becomes the castle pair's second part
///   with its `#` replaced by the drawn owner name, a space and the castle's name; the name
///   gets the pair's first part and a space in front;
/// - house: picture 5 one of entries 16–18 of the ruins list, picture 6 entry 19, others no
///   name;
/// - altar: variants 0–1, 2 and 3–4 one of entries 0–2, 3–5 and 6–8 of its list;
/// - ruins: variants 0, 4, 5 one of entries 3–9; 1 of 10–11; 2, 6, 7 of 0–2; 3 of 15–17;
///   8 of 12–14; no owner name;
/// - every other type: a drawn name, then a drawn owner name.
pub fn building_names(pools: &NamePools, rng: &mut Rng, x: u16, y: u16, picture_type: u8, variant: u8) -> (String, String) {
    *rng = Rng::new((x as u32).wrapping_mul(11).wrapping_add((y as u32).wrapping_mul(7)).wrapping_add(variant as u32 * 3).wrapping_add(picture_type as u32));
    let names = pools.names.group(picture_type);
    let owners = pools.owners.group(picture_type);
    let pick = |rng: &mut Rng, n: i32, base: i32| entry(names, rng.random(n) + base);
    match picture_type {
        2 => {
            let (a, b) = match variant {
                3 => {
                    if rng.random(2) == 0 {
                        &pools.settlement
                    } else {
                        &pools.village
                    }
                }
                4 => {
                    if rng.random(2) == 0 {
                        &pools.mill
                    } else {
                        &pools.farm
                    }
                }
                _ => &pools.village,
            };
            let name = format!("{a} {}", draw(rng, names));
            let owner = format!("{b} {}", draw(rng, owners));
            (name, owner)
        }
        3 => {
            let name = draw(rng, names);
            let hero = draw(rng, owners);
            let owner = format!("{} {name}", pools.castle.1.replace('#', &hero));
            (format!("{} {name}", pools.castle.0), owner)
        }
        8 => {
            let ruins = pools.names.group(12);
            let name = match variant {
                5 => entry(ruins, rng.random(3) + 16),
                6 => entry(ruins, 19),
                _ => String::new(),
            };
            (name, String::new())
        }
        10 => {
            let name = match variant {
                0 | 1 => pick(rng, 3, 0),
                2 => pick(rng, 3, 3),
                3 | 4 => pick(rng, 3, 6),
                _ => String::new(),
            };
            (name, String::new())
        }
        12 => {
            let name = match variant {
                0 | 4 | 5 => pick(rng, 7, 3),
                1 => pick(rng, 2, 10),
                2 | 6 | 7 => pick(rng, 3, 0),
                3 => pick(rng, 3, 15),
                8 => pick(rng, 3, 12),
                _ => String::new(),
            };
            (name, String::new())
        }
        _ => {
            let name = draw(rng, names);
            (name, draw(rng, owners))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A pool of our own: group g's entries are `n<g>.<i>` and `o<g>.<i>`.
    pub fn pools() -> NamePools {
        let mut text = String::from("[Label]\r\nMill=M1/M2\r\nFarm=F1/F2\r\nVillage=V1/V2\r\nSettlement=S1/S2\r\nCastle=C1/K # of\r\n[Names]\r\n");
        for g in 1..=15 {
            text.push_str(&format!("#{g} comment\r\n"));
            for i in 0..20 {
                text.push_str(&format!("n{g}.{i}  \r\n"));
            }
        }
        text.push_str("\r\nignored after the blank line\r\n[Heros]\r\n");
        for g in 1..=15 {
            text.push_str(&format!("#{g:>2}\r\n"));
            for i in 0..3 {
                text.push_str(&format!("o{g}.{i}\r\n"));
            }
        }
        NamePools::parse(&text)
    }

    #[test]
    fn lists_read_as_the_original() {
        let p = pools();
        assert_eq!(p.names.group(1).len(), 20);
        assert_eq!(p.names.group(15).last().map(String::as_str), Some("n15.19"), "trailing spaces cut");
        assert!(p.names.group(0).is_empty() && p.names.group(16).is_empty());
        assert_eq!(p.owners.group(10), ["o10.0", "o10.1", "o10.2"]);
        assert_eq!(p.castle, ("C1".to_string(), "K # of".to_string()));
        assert_eq!(p.mill, ("M1".to_string(), "M2".to_string()));
        // A blank line ends the list.
        let short = NamePools::parse("[Names]\n#1\na\n\nb\n#2\nc\n");
        assert_eq!((short.names.group(1), short.names.group(2).len()), (&["a".to_string()][..], 0));
    }

    #[test]
    fn the_install_lists_have_the_shipped_sizes() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let p = NamePools::load(Path::new(&dir)).expect("the editor's language ini");
        let sizes: Vec<(usize, usize)> = [1, 2, 3, 4, 5, 6, 7, 12, 13, 14].iter().map(|g| (p.names.group(*g).len(), p.owners.group(*g).len())).collect();
        assert_eq!(sizes, [(15, 8), (46, 23), (60, 77), (7, 7), (24, 1), (6, 1), (11, 8), (20, 1), (1, 1), (1, 1)]);
        for pair in [&p.mill, &p.farm, &p.village, &p.settlement, &p.castle] {
            assert!(!pair.0.is_empty() && !pair.1.is_empty());
        }
        assert!(p.castle.1.contains('#'));
    }

    #[test]
    fn names_are_seeded_by_the_spot() {
        let p = pools();
        let mut rng = Rng::new(12345);
        // Seed 10·11 + 20·7 + 0·3 + 1 = 251; then Random(20), Random(3).
        let mut expect = Rng::new(251);
        let (n, o) = (expect.random(20), expect.random(3));
        assert_eq!(building_names(&p, &mut rng, 10, 20, 1, 0), (format!("n1.{n}"), format!("o1.{o}")));
        assert_eq!(rng.state(), expect.state(), "the stream goes on from the draws");
        // The same spot gives the same names whatever came before.
        let mut other = Rng::new(7);
        assert_eq!(building_names(&p, &mut other, 10, 20, 1, 0), building_names(&p, &mut Rng::new(99), 10, 20, 1, 0));
        // A village of variant 3: Random(2) picks the pair first.
        let mut e = Rng::new(5 * 11 + 5 * 7 + 3 * 3 + 2);
        let pair = if e.random(2) == 0 { ("S1", "S2") } else { ("V1", "V2") };
        let (n, o) = (e.random(20), e.random(3));
        assert_eq!(building_names(&p, &mut rng, 5, 5, 2, 3), (format!("{} n2.{n}", pair.0), format!("{} o2.{o}", pair.1)));
        // A castle: the owner is the pair's second part with # replaced, then the name.
        let mut e = Rng::new(3 * 11 + 4 * 7 + 3);
        let (n, o) = (e.random(20), e.random(3));
        assert_eq!(building_names(&p, &mut rng, 3, 4, 3, 0), (format!("C1 n3.{n}"), format!("K o3.{o} of n3.{n}")));
        // Ruins variant 3: one of entries 15–17; house 6: entry 19 of the ruins, no draw.
        let mut e = Rng::new(11 + 7 + 9 + 12);
        assert_eq!(building_names(&p, &mut rng, 1, 1, 12, 3).0, format!("n12.{}", e.random(3) + 15));
        let before = Rng::new(11 + 7 + 6 * 3 + 8);
        assert_eq!(building_names(&p, &mut rng, 1, 1, 8, 6), ("n12.19".to_string(), String::new()));
        assert_eq!(rng.state(), before.state());
        // An empty group gives an empty name (the original stops with a list-index error).
        assert_eq!(building_names(&NamePools::default(), &mut rng, 1, 1, 9, 0), (String::new(), String::new()));
    }
}
