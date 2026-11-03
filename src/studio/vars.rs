//! The reference scan of one story variable: where it is declared (`[vars]` of the triggers' files),
//! read (a condition `var = …` of a trigger or a dialog choice, the `var_changed` event) and written
//! (`set`, `add`, `random_var` actions of a trigger or a choice). It reads the package's parsed model
//! (`pack::triggers`), so each use carries the line of its table, as `pack::uses` does for armies and
//! buildings. Covered: every trigger (any map), every dialog choice. Not covered: a map's own victory
//! and the campaign graph, which hold no variables.

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

/// One use of a variable: the role, the file, its 1-based line, and what holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarUse {
    pub role: Role,
    pub file: String,
    pub line: u32,
    /// `trigger `id``, `trigger `id` (map `m`)`, or ``dialog `d`, choice `c` ``.
    pub what: String,
}

/// The uses of variable `name` in the package, in file and line order.
pub fn variable_uses(pkg: &Package, name: &str) -> Vec<VarUse> {
    let mut out = Vec::new();
    for v in pkg.vars.iter().filter(|v| v.value.name == name) {
        out.push(VarUse { role: Role::Declared, file: v.file.clone(), line: v.line, what: "the variable's declaration".into() });
    }
    for t in &pkg.triggers {
        let what = match &t.value.map {
            Some(m) => format!("trigger `{}` (map `{m}`)", t.value.id),
            None => format!("trigger `{}`", t.value.id),
        };
        if matches!(&t.value.when, Event::VarChanged(n) if n == name) {
            out.push(VarUse { role: Role::Read, file: t.file.clone(), line: t.line, what: what.clone() });
        }
        scan(name, &t.file, t.line, &what, &t.value.cond, &t.value.then, &mut out);
    }
    for d in &pkg.dialogs {
        for c in &d.value.choices {
            let what = format!("dialog `{}`, choice `{}`", d.value.id, c.id);
            scan(name, &d.file, d.line, &what, &c.cond, &c.then, &mut out);
        }
    }
    out.sort_by(|a, b| (a.file.as_str(), a.line, a.role as u8).cmp(&(b.file.as_str(), b.line, b.role as u8)));
    // A trigger that both listens to the variable and tests it is one read on its line.
    out.dedup();
    out
}

/// The conditions and actions of one table, against `name`.
fn scan(name: &str, file: &str, line: u32, what: &str, conds: &[Cond], actions: &[Action], out: &mut Vec<VarUse>) {
    let mut push = |role| out.push(VarUse { role, file: file.to_string(), line, what: what.to_string() });
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
