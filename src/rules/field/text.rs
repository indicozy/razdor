//! The battle log of a field battle, as text keys. A line is a key and its arguments (`{0}`,
//! `{1}`, …); the screen resolves it in the game's language: a package's text first, then the
//! table here, else the key itself. No macroquad, no Lua.

/// One line of the log: a text key and its arguments.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LogLine {
    pub key: String,
    pub args: Vec<String>,
}

/// The built-in lines: key, English, Russian.
const BUILTIN: &[(&str, &str, &str)] = &[
    ("field.turn", "turn {0}", "ход {0}"),
    ("field.print", "{0}", "{0}"),
    ("field.moves", "{0} moves to {1}", "{0} идёт на {1}"),
    ("field.stopped", "{0} is stopped at {1}", "{0} останавливается на {1}"),
    ("field.cannot_reach", "{0} cannot reach {1}", "{0} не может дойти до {1}"),
    ("field.occupied", "{0} cannot step onto {1}: someone stands there", "{0} не может встать на {1}: там кто-то стоит"),
    ("field.no_target", "{0} has no target in reach", "{0} не видит цели в досягаемости"),
    ("field.shoots", "{0} shoots {1} for {2}", "{0} стреляет в {1}: {2}"),
    ("field.strikes", "{0} strikes {1} for {2}", "{0} бьёт {1}: {2}"),
    ("field.falls", "{0} falls", "{0} падает"),
    ("field.no_gate", "{0} has no gate at {1}", "{0}: нет ворот на {1}"),
    ("field.gate_unreached", "{0} cannot reach the gate at {1}", "{0} не достаёт до ворот на {1}"),
    ("field.gate_broken", "{0} breaks the gate at {1}", "{0} ломает ворота на {1}"),
    ("field.gate_hit", "{0} hits the gate at {1} ({2} left)", "{0} бьёт ворота на {1} (осталось {2})"),
    ("field.place_default", "the script's place for unit {0} at {1} is not on open ground; the default cell is used", "место юнита {0} на {1} не открытая земля: взято место по умолчанию"),
    ("field.no_cell", "no cell for {0}: it does not take part", "нет места для {0}: он не участвует"),
    ("field.script_error", "the battle script failed: {0}; the defenders win", "скрипт боя дал сбой: {0}; защитники побеждают"),
    ("field.turn_limit", "the turn limit of {0} is reached", "достигнут предел в {0} ходов"),
    ("field.over", "the battle is over: {0}", "бой окончен: {0}"),
    ("field.defenders_hold", "the defenders hold the field", "защитники удержали поле"),
    ("field.attackers_hold", "the attackers hold the field", "нападающие заняли поле"),
];

/// The built-in template of `key` in `lang` (`ru` or English for every other language).
pub fn builtin(lang: &str, key: &str) -> Option<&'static str> {
    BUILTIN.iter().find(|(k, _, _)| *k == key).map(|(_, en, ru)| if lang == "ru" { *ru } else { *en })
}

/// Fills `{n}` with the arguments.
pub fn fill(template: &str, args: &[String]) -> String {
    let mut out = template.to_string();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

/// A line in `lang`: the package's text for its key (`texts`), else the built-in one, else the key.
pub fn render(line: &LogLine, lang: &str, texts: &std::collections::BTreeMap<String, String>) -> String {
    let template = texts.get(&line.key).cloned().or_else(|| builtin(lang, &line.key).map(str::to_string)).unwrap_or_else(|| line.key.clone());
    fill(&template, &line.args)
}

/// The line in English, for the DT battle log the field battle leaves behind.
pub fn english(line: &LogLine) -> String {
    fill(builtin("en", &line.key).unwrap_or(&line.key), &line.args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_fills_its_arguments_and_a_package_text_wins() {
        let l = LogLine { key: "field.gate_hit".into(), args: vec!["Varga".into(), "5,3".into(), "12".into()] };
        assert_eq!(english(&l), "Varga hits the gate at 5,3 (12 left)");
        let mut texts = std::collections::BTreeMap::new();
        texts.insert("field.gate_hit".to_string(), "{0} бьёт {1}".to_string());
        assert_eq!(render(&l, "ru", &texts), "Varga бьёт 5,3");
        let unknown = LogLine { key: "siege.nothing".into(), args: vec![] };
        assert_eq!(render(&unknown, "en", &texts), "siege.nothing");
    }
}
