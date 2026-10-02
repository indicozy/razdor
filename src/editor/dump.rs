//! The original editor's text dump (DTMapEdit 0x5a5660, reader 0x5a7930): every text of a
//! map as plain lines, for translating a map
//! (docs/reference/editor/mapcheck-files.md §5).
//!
//! A save asked for as `.DTD` writes the map as `<name>.DTm` and the dump next to it as
//! `<name>.Eng` when the title starts with an ASCII letter (or one of the six characters
//! between `Z` and `a`), else `<name>.Rus`. Opening a `.DTD` name reads `<name>.DTm`, then
//! the dump chosen the same way by the title just loaded.

use std::path::{Path, PathBuf};

use crate::dt::dtm::{CustomArtefact, FlagScript, Scenario};
use crate::dt::text;

use super::mapfile::change_ext;

/// A line break inside a text, as the dump writes it.
const BREAK: &str = " %/";
/// The spellings of the line-break marker the reader turns back into a line break, in the
/// order it replaces them.
const BREAKS_READ: [&str; 4] = [" %/ ", "%/ ", " %/", "%/"];

/// The dump's file next to the map at `map`: `.Eng` or `.Rus` by the title's first character.
pub fn dump_path(map: &Path, title: &str) -> PathBuf {
    let latin = title.chars().next().is_some_and(|c| ('A'..='z').contains(&c));
    change_ext(map, if latin { ".Eng" } else { ".Rus" })
}

/// The text up to its first `%` (the flag script and an army's archetype are not dumped).
fn before_percent(s: &str) -> &str {
    s.split_once('%').map_or(s, |(t, _)| t)
}

/// The dump of `s`, as the bytes of its file (cp1251, CR LF line ends). Custom artefacts
/// (`[I#n]` blocks) are never written: the save that writes the dump has just dropped them.
pub fn write_dump(s: &Scenario) -> Vec<u8> {
    let mut lines: Vec<String> = vec!["[Head]".into(), s.title.clone(), s.description.clone()];
    if !s.campaign_name.is_empty() {
        lines.push(s.campaign_name.clone());
    }
    for (i, b) in s.buildings.iter().enumerate() {
        lines.extend([format!("[B#{}]", i + 1), b.name.clone(), b.owner_name.clone(), b.description.clone()]);
    }
    for (i, a) in s.armies.iter().enumerate() {
        lines.extend([format!("[A#{}]", i + 1), before_percent(&a.name).to_string(), a.leader_name.clone(), a.description.clone()]);
    }
    for (i, e) in s.events.iter().enumerate() {
        lines.extend([format!("[E#{}]", i + 1), before_percent(&e.title).to_string()]);
        if e.conditions.confirm_question == 1 {
            lines.push(e.question.clone());
        }
        if !e.message.is_empty() {
            lines.push(e.message.clone());
        }
    }
    for (i, n) in s.named_characters.iter().enumerate() {
        lines.extend([format!("[U#{}]", i + 1), n.name.clone()]);
    }
    let mut out = Vec::new();
    for l in lines {
        out.extend(text::encode(&l.replace("\r\n", BREAK)));
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Why a dump was not read to its end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DumpError {
    /// The first line is not `[Head]`: the file is ignored.
    NoHead,
    /// The number of a block tag on this line (1-based) cannot be read. The original shows
    /// its error and then reads the same line again forever; the import stops here.
    BadNumber(usize),
    /// A block numbered 0 on this line: the original stops with a range error; the import
    /// stops here.
    ZeroNumber(usize),
}

struct Lines<'a> {
    lines: Vec<&'a str>,
    next: usize,
}

impl<'a> Lines<'a> {
    /// The next line with its line-break markers turned back into line breaks (empty at the
    /// end of the file).
    fn read(&mut self) -> String {
        let line = self.lines.get(self.next).copied().unwrap_or("");
        self.next += 1;
        BREAKS_READ.iter().fold(line.to_string(), |l, m| l.replace(m, "\r\n"))
    }

    /// The number of the line [`Lines::read`] gave last, 1-based.
    fn number(&self) -> usize {
        self.next
    }
}

fn tag_number(line: &str) -> Option<i64> {
    let (_, rest) = line.split_once('#')?;
    let (n, _) = rest.split_once(']')?;
    n.trim().parse().ok()
}

fn starts_block(line: &str) -> bool {
    line.starts_with('[')
}

/// Reads a dump into `s` (and the map's custom artefacts), as the original's import does:
/// the blocks in the writer's order, each read while lines start with its tag; a block's
/// further strings only while the next line does not start with `[`, so missing trailing
/// strings keep their old value. An event's title keeps its old flag script, and its
/// question is read only when the question box is set. Ids are not checked against the
/// records: the original writes the texts of an id past the end into unused slots, which
/// nothing shows; here they are skipped. `base_artefacts` is the size of the install's
/// artefact list: the `[I#n]` blocks name artefact n of the editor's whole list.
pub fn read_dump(s: &mut Scenario, custom: &mut [CustomArtefact], base_artefacts: usize, bytes: &[u8]) -> Result<(), DumpError> {
    let text = text::decode(bytes);
    let mut r = Lines { lines: text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect(), next: 0 };
    if r.read() != "[Head]" {
        return Err(DumpError::NoHead);
    }
    s.title = r.read().chars().take(64).collect();
    s.description = r.read();
    let mut line = r.read();
    if !starts_block(&line) {
        s.campaign_name = line;
        line = r.read();
    }
    // The id of a block line, or the reason the import stops there.
    fn id(line: &str, at: usize) -> Result<usize, DumpError> {
        match tag_number(line) {
            None => Err(DumpError::BadNumber(at)),
            Some(n) if n < 0 => Err(DumpError::BadNumber(at)),
            Some(0) => Err(DumpError::ZeroNumber(at)),
            Some(n) => Ok(n as usize),
        }
    }
    // Up to `n` strings of a block: the first always, the others while no block starts.
    fn strings(r: &mut Lines, n: usize) -> (Vec<String>, String) {
        let mut got = vec![r.read()];
        let mut line = r.read();
        while got.len() < n && !starts_block(&line) {
            got.push(line);
            line = r.read();
        }
        (got, line)
    }
    while line.starts_with("[B#") {
        let i = id(&line, r.number())?;
        let (got, next) = strings(&mut r, 3);
        if let Some(b) = s.buildings.get_mut(i - 1) {
            for (k, v) in got.into_iter().enumerate() {
                *[&mut b.name, &mut b.owner_name, &mut b.description][k] = v;
            }
        }
        line = next;
    }
    while line.starts_with("[A#") {
        let i = id(&line, r.number())?;
        let (got, next) = strings(&mut r, 3);
        if let Some(a) = s.armies.get_mut(i - 1) {
            for (k, v) in got.into_iter().enumerate() {
                *[&mut a.name, &mut a.leader_name, &mut a.description][k] = v;
            }
        }
        line = next;
    }
    while line.starts_with("[E#") {
        let i = id(&line, r.number())?;
        let title = r.read();
        let mut next = r.read();
        let event = s.events.get_mut(i - 1);
        let asks = event.as_ref().is_some_and(|e| e.conditions.confirm_question == 1);
        let (mut question, mut message) = (None, None);
        if !starts_block(&next) {
            if asks {
                question = Some(next);
                next = r.read();
            }
            if !starts_block(&next) {
                message = Some(next);
                next = r.read();
            }
        }
        if let Some(e) = event {
            let script = e.title.find('%').map(|p| e.title[p..].to_string()).unwrap_or_default();
            e.title = title + &script;
            e.flags = FlagScript::from_title(&e.title);
            if let Some(q) = question {
                e.question = q;
            }
            if let Some(m) = message {
                e.message = m;
            }
        }
        line = next;
    }
    while line.starts_with("[I#") {
        let i = id(&line, r.number())?;
        let (got, next) = strings(&mut r, 2);
        // Artefact i of the editor's list: past the install's list, a custom one. The
        // original renames an install artefact in its own memory, which no map keeps.
        if let Some(c) = i.checked_sub(base_artefacts + 1).and_then(|k| custom.get_mut(k)) {
            for (k, v) in got.into_iter().enumerate() {
                *[&mut c.name, &mut c.description][k] = v;
            }
        }
        line = next;
    }
    while line.starts_with("[U#") {
        let i = id(&line, r.number())?;
        let name: String = r.read().chars().take(64).collect();
        if let Some(n) = s.named_characters.get_mut(i - 1) {
            n.name = name;
        }
        line = r.read();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Building, Event, NamedCharacter};

    fn map() -> Scenario {
        let mut s = Scenario { title: "Поход".into(), description: "Две\r\nстроки".into(), ..Scenario::default() };
        s.buildings = vec![Building { name: "Замок".into(), owner_name: "Барон".into(), description: "Стены".into(), ..Building::default() }];
        s.armies = vec![Army { name: "Отряд%#рыцарь".into(), leader_name: "Вождь".into(), description: String::new(), ..Army::default() }];
        let mut asks = Event { title: "Сделка%+Флаг".into(), question: "Да?".into(), message: "Готово.".into(), ..Event::default() };
        asks.conditions.confirm_question = 1;
        let silent = Event { title: "Тихо".into(), question: "не спрашивается".into(), ..Event::default() };
        s.events = vec![asks, silent];
        for e in &mut s.events {
            e.flags = FlagScript::from_title(&e.title);
        }
        s.named_characters = vec![NamedCharacter { unit: 5, name: "Имя".into() }];
        s
    }

    #[test]
    fn the_writer_dumps_every_text_in_blocks() {
        let text = text::decode(&write_dump(&map()));
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(
            lines,
            [
                "[Head]", "Поход", "Две %/строки", // no campaign line when it is empty
                "[B#1]", "Замок", "Барон", "Стены",
                "[A#1]", "Отряд", "Вождь", "",
                "[E#1]", "Сделка", "Да?", "Готово.",
                "[E#2]", "Тихо", // no question without the box, no empty message
                "[U#1]", "Имя", "",
            ]
        );
        assert_eq!(dump_path(Path::new("/m/Поход.DTm"), "Поход"), Path::new("/m/Поход.Rus"));
        assert_eq!(dump_path(Path::new("/m/a.DTm"), "Quest"), Path::new("/m/a.Eng"));
        assert_eq!(dump_path(Path::new("/m/a.DTm"), "_x"), Path::new("/m/a.Eng"), "the six characters between Z and a count");
        assert_eq!(dump_path(Path::new("/m/a.DTm"), ""), Path::new("/m/a.Rus"));
    }

    #[test]
    fn the_reader_puts_texts_back_and_keeps_flag_scripts() {
        let mut s = map();
        let dump = "[Head]\r\nTrip\r\nOne %/two\r\nCampaign\r\n[B#1]\r\nCastle\r\n[A#1]\r\nBand\r\nChief\r\nDesc\r\n\
                    [E#1]\r\nDeal\r\nYes?\r\nDone.\r\n[E#2]\r\nQuiet\r\nNow%/ heard\r\n[E#9]\r\nnone\r\n[U#1]\r\nName\r\n";
        read_dump(&mut s, &mut [], 0, dump.as_bytes()).unwrap();
        assert_eq!((s.title.as_str(), s.description.as_str(), s.campaign_name.as_str()), ("Trip", "One\r\ntwo", "Campaign"));
        // Missing trailing strings keep their old value.
        let b = &s.buildings[0];
        assert_eq!((b.name.as_str(), b.owner_name.as_str(), b.description.as_str()), ("Castle", "Барон", "Стены"));
        assert_eq!((s.armies[0].name.as_str(), s.armies[0].description.as_str()), ("Band", "Desc"));
        assert_eq!((s.events[0].title.as_str(), s.events[0].question.as_str(), s.events[0].message.as_str()), ("Deal%+Флаг", "Yes?", "Done."));
        assert_eq!(s.events[0].flags.as_ref().and_then(|f| f.set.as_deref()), Some("Флаг"));
        // Without the question box the second line is the message.
        assert_eq!((s.events[1].question.as_str(), s.events[1].message.as_str()), ("не спрашивается", "Now\r\nheard"));
        assert_eq!(s.named_characters[0].name, "Name");
        // A dump of a map reads back to the same texts, but for an army name's part from
        // its `%`, which the writer leaves out and the reader does not keep (the original's
        // behaviour).
        let mut back = map();
        back.title = "x".into();
        read_dump(&mut back, &mut [], 0, &write_dump(&map())).unwrap();
        let mut want = map();
        want.armies[0].name = "Отряд".into();
        assert_eq!(back, want);
    }

    #[test]
    fn the_reader_stops_where_the_original_hangs() {
        let mut s = map();
        assert_eq!(read_dump(&mut s, &mut [], 0, b"Head\r\n"), Err(DumpError::NoHead));
        assert_eq!(s, map(), "a file without [Head] is ignored");
        let bad = b"[Head]\r\nT\r\nD\r\n[B#x]\r\nName\r\n";
        assert_eq!(read_dump(&mut s, &mut [], 0, bad), Err(DumpError::BadNumber(4)));
        let zero = b"[Head]\r\nT\r\nD\r\n[B#1]\r\nA\r\n[A#0]\r\nB\r\n";
        assert_eq!(read_dump(&mut s, &mut [], 0, zero), Err(DumpError::ZeroNumber(6)));
        assert_eq!(s.buildings[0].name, "A", "what came before stays read");
    }

    #[test]
    fn custom_artefact_blocks_name_artefacts_past_the_install_list() {
        let mut s = map();
        let mut custom = vec![CustomArtefact { record: vec![0; 230], name: "a".into(), description: "b".into() }];
        let dump = b"[Head]\r\nT\r\nD\r\n[I#3]\r\nInstall item\r\n[I#11]\r\nCustom\r\n[U#1]\r\nX\r\n";
        read_dump(&mut s, &mut custom, 10, dump).unwrap();
        assert_eq!((custom[0].name.as_str(), custom[0].description.as_str()), ("Custom", "b"));
    }
}
