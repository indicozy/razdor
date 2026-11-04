//! The reference scan of one story variable: where it is declared (`[vars]` of the triggers' files),
//! read (a condition `var = …` of a trigger or a dialog choice, the `var_changed` event) and written
//! (`set`, `add`, `random_var` actions of a trigger or a choice). It reads the package's parsed model
//! (`pack::triggers`), so each use carries the line of its table, as `pack::uses` does for armies and
//! buildings. Covered: every trigger (any map), every dialog choice. Not covered: a map's own victory
//! and the campaign graph, which hold no variables.

use toml_edit::{DocumentMut, InlineTable, Item, Value};

use crate::pack::load::Package;
use crate::pack::triggers::{Action, Cond, Event};

/// How a use touches the variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Declared,
    Read,
    Write,
}

impl Role {
    /// The word the agent sees: `declared`, `read` or `write`.
    pub fn name(self) -> &'static str {
        match self {
            Role::Declared => "declared",
            Role::Read => "read",
            Role::Write => "write",
        }
    }
}

/// One use of a variable: the role, the file, its 1-based line, what holds it, and the trigger or
/// dialog choice it is in (what the story graph selects).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarUse {
    pub role: Role,
    pub file: String,
    pub line: u32,
    /// `trigger `id``, `trigger `id` (map `m`)`, or ``dialog `d`, choice `c` ``.
    pub what: String,
    pub holder: Holder,
}

/// The story node a use is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    /// The variable's declaration (`[vars]`).
    Declared,
    Trigger(String),
    /// A dialog's choice: the dialog's id and the choice's id.
    Choice(String, String),
}

/// The uses of variable `name` in the package, in file and line order.
pub fn variable_uses(pkg: &Package, name: &str) -> Vec<VarUse> {
    let mut out = Vec::new();
    for v in pkg.vars.iter().filter(|v| v.value.name == name) {
        out.push(VarUse { role: Role::Declared, file: v.file.clone(), line: v.line, what: "the variable's declaration".into(), holder: Holder::Declared });
    }
    for t in &pkg.triggers {
        let what = match &t.value.map {
            Some(m) => format!("trigger `{}` (map `{m}`)", t.value.id),
            None => format!("trigger `{}`", t.value.id),
        };
        let holder = Holder::Trigger(t.value.id.clone());
        if matches!(&t.value.when, Event::VarChanged(n) if n == name) {
            out.push(VarUse { role: Role::Read, file: t.file.clone(), line: t.line, what: what.clone(), holder: holder.clone() });
        }
        let site = Site { file: &t.file, line: t.line, what: &what, holder: &holder };
        scan(name, &site, &t.value.cond, &t.value.then, &mut out);
    }
    for d in &pkg.dialogs {
        for c in &d.value.choices {
            let what = format!("dialog `{}`, choice `{}`", d.value.id, c.id);
            let holder = Holder::Choice(d.value.id.clone(), c.id.clone());
            let site = Site { file: &d.file, line: d.line, what: &what, holder: &holder };
            scan(name, &site, &c.cond, &c.then, &mut out);
        }
    }
    out.sort_by(|a, b| (a.file.as_str(), a.line, a.role as u8).cmp(&(b.file.as_str(), b.line, b.role as u8)));
    // A trigger that both listens to the variable and tests it is one read on its line.
    out.dedup();
    out
}

/// Where a table sits: its file and line, what it is called in a list, and who holds it.
struct Site<'a> {
    file: &'a str,
    line: u32,
    what: &'a str,
    holder: &'a Holder,
}

/// The conditions and actions of one table, against `name`.
fn scan(name: &str, site: &Site, conds: &[Cond], actions: &[Action], out: &mut Vec<VarUse>) {
    let mut push = |role| out.push(VarUse { role, file: site.file.to_string(), line: site.line, what: site.what.to_string(), holder: site.holder.clone() });
    if conds.iter().any(|c| matches!(c, Cond::Var { name: n, .. } if n == name)) {
        push(Role::Read);
    }
    if actions.iter().any(|a| match a {
        Action::Set { name: n, .. } | Action::Add { name: n, .. } | Action::RandomVar { name: n, .. } => n == name,
        _ => false,
    }) {
        push(Role::Write);
    }
}

/// The text of a story file with the variable renamed: its `[vars]` key, the `var` of a condition
/// and of a `var_changed` event, and the `set`, `add` and `random_var` of an action, in its triggers
/// and in its dialogs' choices. `None` when nothing in it names `old`.
pub fn rename_in_story(text: &str, old: &str, new: &str) -> Result<Option<String>, String> {
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let mut changed = false;
    if let Some(Item::ArrayOfTables(triggers)) = doc.get_mut("trigger") {
        for t in triggers.iter_mut() {
            if let Some(Item::Value(Value::InlineTable(when))) = t.get_mut("when") {
                rename_row(when, old, new, &mut changed);
            }
            rename_rows(t.get_mut("if"), old, new, &mut changed);
            rename_rows(t.get_mut("then"), old, new, &mut changed);
        }
    }
    if let Some(Item::ArrayOfTables(dialogs)) = doc.get_mut("dialog") {
        for d in dialogs.iter_mut() {
            if let Some(Item::ArrayOfTables(choices)) = d.get_mut("choice") {
                for c in choices.iter_mut() {
                    rename_rows(c.get_mut("if"), old, new, &mut changed);
                    rename_rows(c.get_mut("then"), old, new, &mut changed);
                }
            }
        }
    }
    if !changed && !declares(text, old)? {
        return Ok(None);
    }
    let mut out = doc.to_string();
    // The declaration's key is respelled in place (toml_edit has no key setter): its span is
    // replaced by the new name.
    let parsed = toml_edit::Document::parse(out.clone()).map_err(|e| e.to_string())?;
    let span = parsed.as_table().get("vars").and_then(Item::as_table).and_then(|v| v.get_key_value(old)).and_then(|(k, _)| k.span());
    if let Some(span) = span {
        out.replace_range(span, new);
        changed = true;
    }
    Ok(changed.then_some(out))
}

/// Whether the text declares `name` in its `[vars]` table.
fn declares(text: &str, name: &str) -> Result<bool, String> {
    let parsed = toml_edit::Document::parse(text.to_string()).map_err(|e| e.to_string())?;
    Ok(parsed.as_table().get("vars").and_then(Item::as_table).is_some_and(|v| v.contains_key(name)))
}

/// The rows of an array (`if`, `then`) renamed in place.
fn rename_rows(item: Option<&mut Item>, old: &str, new: &str, changed: &mut bool) {
    if let Some(Item::Value(Value::Array(rows))) = item {
        for row in rows.iter_mut() {
            if let Value::InlineTable(t) = row {
                rename_row(t, old, new, changed);
            }
        }
    }
}

/// One condition, action or event: the keys that name a variable (`var`, `set`, `add`, `random_var`)
/// get the new name when they hold the old one.
fn rename_row(t: &mut InlineTable, old: &str, new: &str, changed: &mut bool) {
    for key in ["var", "set", "add", "random_var"] {
        if let Some(v) = t.get_mut(key) {
            if v.as_str() == Some(old) {
                let decor = v.decor().clone();
                *v = Value::from(new);
                *v.decor_mut() = decor;
                *changed = true;
            }
        }
    }
}

/// The files a rename changes, as `(path, new text)`: the story files (triggers and dialogs) that
/// name `old`. `text_of` gives a file's current text. Nothing changes when `new` is not an id, is
/// already a variable, or `old` is not one.
pub fn rename_plan(pkg: &Package, old: &str, new: &str, text_of: &dyn Fn(&str) -> Option<String>) -> Result<Vec<(String, String)>, String> {
    if !crate::studio::characters::valid_id(new) {
        return Err(format!("`{new}` is not a name: use lowercase letters, digits and _ only"));
    }
    if !pkg.vars.iter().any(|v| v.value.name == old) {
        return Err(format!("no variable `{old}`"));
    }
    if old == new {
        return Err("the new name is the same".into());
    }
    if pkg.vars.iter().any(|v| v.value.name == new) {
        return Err(format!("a variable `{new}` already exists"));
    }
    let mut files: Vec<String> = pkg.files.paths().filter(|p| (p.starts_with("triggers/") || p.starts_with("dialogs/")) && p.ends_with(".toml")).map(str::to_string).collect();
    files.sort();
    let mut plan = Vec::new();
    for file in files {
        let Some(text) = text_of(&file) else { continue };
        if let Some(next) = rename_in_story(&text, old, new)? {
            plan.push((file, next));
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::files::Files;

    fn package(files: &[(&str, &str)]) -> Package {
        let mut f = Files::default();
        for (p, t) in files {
            f.insert(p, *t);
        }
        Package::parse(f).0
    }

    #[test]
    fn finds_declaration_reads_and_writes_with_their_lines() {
        let pkg = package(&[
            ("triggers/a.toml", "[vars]\ndread = 0\n\n[[trigger]]\nid = \"ambush\"\nwhen = { event = \"var_changed\", var = \"dread\" }\nif = [{ var = \"dread\", op = \">=\", value = 2 }]\nthen = [{ set = \"oath\", value = true }, { add = \"dread\", value = 1 }]\n"),
            ("dialogs/d.toml", "[[dialog]]\nid = \"d\"\ntitle = \"t\"\ntext = \"x\"\n[[dialog.choice]]\nid = \"c\"\ntext = \"x\"\nthen = [{ random_var = \"dread\", min = 1, max = 3 }]\n"),
        ]);
        let uses = variable_uses(&pkg, "dread");
        // A trigger's uses are on its table's line; a dialog choice has no line of its own in the
        // parsed model, so it is the dialog's line (`what` names the choice).
        let got: Vec<(Role, &str, u32)> = uses.iter().map(|u| (u.role, u.file.as_str(), u.line)).collect();
        assert_eq!(
            got,
            vec![
                (Role::Write, "dialogs/d.toml", 1),
                (Role::Declared, "triggers/a.toml", uses[1].line),
                (Role::Read, "triggers/a.toml", 4),
                (Role::Write, "triggers/a.toml", 4),
            ],
            "{uses:?}"
        );
        assert!(uses.iter().any(|u| u.what == "trigger `ambush`" && u.role == Role::Read));
        assert_eq!(uses[0].what, "dialog `d`, choice `c`");
        let oath = variable_uses(&pkg, "oath");
        assert_eq!((oath.len(), oath[0].role, oath[0].line), (1, Role::Write, 4));
        assert!(variable_uses(&pkg, "nothing").is_empty());
    }
}
