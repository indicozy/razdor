//! Diagnostics of a content package: what `razdor-pack check` reports, for people and agents.

use serde::Serialize;

/// Errors make a package unusable; warnings do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    fn word(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One problem found in a package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Package-relative path with `/` separators; empty for the package as a whole.
    pub file: String,
    /// 1-based line in `file`, when known.
    pub line: Option<u32>,
    pub message: String,
    /// What to do about it.
    pub hint: Option<String>,
}

/// The diagnostics of one check, in the order they were found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub diagnostics: Vec<Diagnostic>,
}

impl Report {
    pub fn push(&mut self, severity: Severity, file: &str, line: Option<u32>, message: impl Into<String>, hint: Option<String>) {
        self.diagnostics.push(Diagnostic { severity, file: file.to_string(), line, message: message.into(), hint });
    }

    pub fn error(&mut self, file: &str, line: Option<u32>, message: impl Into<String>) {
        self.push(Severity::Error, file, line, message, None);
    }

    pub fn error_hint(&mut self, file: &str, line: Option<u32>, message: impl Into<String>, hint: impl Into<String>) {
        self.push(Severity::Error, file, line, message, Some(hint.into()));
    }

    pub fn warning(&mut self, file: &str, line: Option<u32>, message: impl Into<String>) {
        self.push(Severity::Warning, file, line, message, None);
    }

    pub fn warning_hint(&mut self, file: &str, line: Option<u32>, message: impl Into<String>, hint: impl Into<String>) {
        self.push(Severity::Warning, file, line, message, Some(hint.into()));
    }

    pub fn errors(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
    }

    pub fn warnings(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Warning).count()
    }

    pub fn has_errors(&self) -> bool {
        self.errors() > 0
    }

    /// One line per diagnostic (`error: units/a.toml:12: message (hint: …)`), then a summary.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for d in &self.diagnostics {
            out.push_str(d.severity.word());
            out.push_str(": ");
            if !d.file.is_empty() {
                out.push_str(&d.file);
                if let Some(line) = d.line {
                    out.push_str(&format!(":{line}"));
                }
                out.push_str(": ");
            }
            out.push_str(&d.message);
            if let Some(hint) = &d.hint {
                out.push_str(&format!(" (hint: {hint})"));
            }
            out.push('\n');
        }
        let plural = |n: usize, word: &str| if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") };
        out.push_str(&format!("{}, {}\n", plural(self.errors(), "error"), plural(self.warnings(), "warning")));
        out
    }

    /// The report as JSON: `{"ok": …, "diagnostics": [{"severity", "file", "line", "message", "hint"}]}`.
    pub fn to_json(&self) -> String {
        #[derive(Serialize)]
        struct Json<'a> {
            ok: bool,
            diagnostics: &'a [Diagnostic],
        }
        let json = Json { ok: !self.has_errors(), diagnostics: &self.diagnostics };
        serde_json::to_string_pretty(&json).expect("a report serialises")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_lines_carry_location_and_hint() {
        let mut r = Report::default();
        r.error_hint("units/a.toml", Some(12), "unknown alias", "dt:knight");
        r.warning("", None, "odd entry");
        let text = r.to_text();
        assert!(text.starts_with("error: units/a.toml:12: unknown alias (hint: dt:knight)\n"));
        assert!(text.contains("warning: odd entry\n"));
        assert!(text.ends_with("1 error, 1 warning\n"));
        assert!(r.has_errors());
    }

    #[test]
    fn json_has_ok_flag() {
        let mut r = Report::default();
        assert!(r.to_json().contains("\"ok\": true"));
        r.error("a.toml", None, "bad");
        let json = r.to_json();
        assert!(json.contains("\"ok\": false"));
        assert!(json.contains("\"severity\": \"error\""));
    }
}
