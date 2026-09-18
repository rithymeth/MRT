//! Source positions and error rendering for the MRT 2.0 frontend.
//!
//! The MRT 1.x toolchain tracks only a line number per token, so every error
//! it can produce reads `[line 17]` and points at a whole line. Spans are the
//! one thing the old frontend cannot retrofit cheaply, and everything built
//! later -- a language server, a formatter, a type checker with "expected X,
//! found Y" -- needs them, so they go in at the bottom here.
//!
//! # Offsets are in `char`s, not bytes
//!
//! Every offset in this crate counts Unicode scalar values, matching how the
//! Python reference lexer indexes its source string. That keeps the two
//! frontends reporting the same column for the same mistake in a file with
//! non-ASCII text, which byte offsets would not. It costs one `Vec<char>` per
//! file, which at compiler-frontend scale is not worth optimising away.

use std::fmt;

/// A half-open range of source, `[start, end)`, in `char` offsets.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end, "span start must not exceed its end");
        Span { start, end }
    }

    /// A zero-width span, for pointing *between* two characters (end of file,
    /// or "something is missing here").
    pub fn empty(at: u32) -> Self {
        Span { start: at, end: at }
    }

    /// The smallest span covering both. Used to grow a node's span as its
    /// children are parsed.
    pub fn to(self, other: Span) -> Self {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn len(self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// A 1-based line and column, as a human would quote it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for LineCol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// One source file, plus the index needed to turn an offset back into a line
/// and column.
pub struct SourceFile {
    name: String,
    chars: Vec<char>,
    /// Offset of the first character of each line. `line_starts[0]` is always
    /// 0, so line N (1-based) begins at `line_starts[N - 1]`.
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let mut line_starts = vec![0u32];
        for (i, c) in chars.iter().enumerate() {
            if *c == '\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        SourceFile {
            name: name.into(),
            chars,
            line_starts,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn len(&self) -> u32 {
        self.chars.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn chars(&self) -> &[char] {
        &self.chars
    }

    /// The text a span covers. An out-of-range span yields what overlap there
    /// is rather than panicking: a diagnostic is the wrong place to crash.
    pub fn slice(&self, span: Span) -> String {
        let start = (span.start as usize).min(self.chars.len());
        let end = (span.end as usize).min(self.chars.len());
        self.chars[start..end].iter().collect()
    }

    pub fn line_col(&self, offset: u32) -> LineCol {
        // The last line start at or before `offset`.
        let line_index = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(next) => next - 1,
        };
        LineCol {
            line: line_index as u32 + 1,
            column: offset - self.line_starts[line_index] + 1,
        }
    }

    /// One line's text, without its newline. Out-of-range lines give `""`.
    pub fn line_text(&self, line: u32) -> String {
        if line == 0 || line as usize > self.line_starts.len() {
            return String::new();
        }
        let start = self.line_starts[line as usize - 1] as usize;
        let end = self
            .line_starts
            .get(line as usize)
            .map(|next| *next as usize - 1)
            .unwrap_or(self.chars.len());
        self.chars[start..end.max(start)].iter().collect()
    }

    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }
}

/// JSON-style quoting, so a value containing a newline, tab or quote stays on
/// one line of a dump. Shared by the token and AST dumps, and mirrored exactly
/// by the Python dumpers -- the comparison is only meaningful if both sides
/// escape identically. Hand-rolled to keep the crate dependency-free.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// Which frontend stage produced a diagnostic. Carried so the compatibility
/// renderer can reproduce MRT 1.x's prefixes, which differ by stage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Lex,
    Parse,
    Resolve,
}

/// One problem with the source.
///
/// `message` is the MRT 1.x message verbatim, so the compatibility renderer
/// can reproduce the old output byte for byte and the conformance harness has
/// something exact to compare. Anything the old frontend could not express
/// goes in `notes`, which only the rich renderer shows.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub stage: Stage,
    pub message: String,
    pub span: Span,
    /// The line MRT 1.x would have reported. Usually the line `span` starts
    /// on, but not always: the old lexer stamps a token with the line it
    /// *ends* on, and a few errors are reported against a construct's opening
    /// line instead. Stored explicitly rather than derived so conformance
    /// never depends on rediscovering those quirks.
    pub legacy_line: u32,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(stage: Stage, message: impl Into<String>, span: Span, legacy_line: u32) -> Self {
        Diagnostic {
            severity: Severity::Error,
            stage,
            message: message.into(),
            span,
            legacy_line,
            notes: Vec::new(),
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// The MRT 1.x rendering, exactly as `python -m mrt` prints it:
    ///
    /// ```text
    /// Syntax Error: Unexpected character '&' (did you mean '&&'?) [line 3]
    /// ```
    ///
    /// This exists so the Rust frontend can be diffed against the reference
    /// implementation byte for byte. It is not the default output.
    pub fn render_compat(&self) -> String {
        let prefix = match self.stage {
            Stage::Lex | Stage::Parse => "Syntax Error",
            Stage::Resolve => "Resolve Error",
        };
        format!("{}: {} [line {}]", prefix, self.message, self.legacy_line)
    }

    /// The rendering meant for humans:
    ///
    /// ```text
    /// error: Unexpected character '&' (did you mean '&&'?)
    ///  --> main.mrt:3:9
    ///   |
    /// 3 |     if (a & b) { }
    ///   |         ^
    /// ```
    pub fn render(&self, file: &SourceFile) -> String {
        let start = file.line_col(self.span.start);
        let mut out = format!("{}: {}\n", self.severity.label(), self.message);
        out.push_str(&format!(" --> {}:{}\n", file.name(), start));

        let text = file.line_text(start.line);
        let gutter_width = start.line.to_string().len();
        let pad = " ".repeat(gutter_width);

        out.push_str(&format!("{} |\n", pad));
        out.push_str(&format!("{} | {}\n", start.line, text));

        // Underline the span, clamped to this line so a multi-line span does
        // not run off the end of it.
        let line_len = text.chars().count() as u32;
        let column = start.column.min(line_len + 1);
        let end_col = if file.line_col(self.span.end).line == start.line {
            file.line_col(self.span.end).column
        } else {
            line_len + 1
        };
        let width = (end_col.saturating_sub(column)).max(1);

        out.push_str(&format!(
            "{} | {}{}\n",
            pad,
            " ".repeat(column.saturating_sub(1) as usize),
            "^".repeat(width as usize)
        ));

        for note in &self.notes {
            out.push_str(&format!("{} = note: {}\n", pad, note));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_is_one_based_and_handles_line_ends() {
        let f = SourceFile::new("t.mrt", "ab\ncd\n");
        assert_eq!(f.line_col(0), LineCol { line: 1, column: 1 });
        assert_eq!(f.line_col(1), LineCol { line: 1, column: 2 });
        // The newline itself belongs to the line it terminates.
        assert_eq!(f.line_col(2), LineCol { line: 1, column: 3 });
        assert_eq!(f.line_col(3), LineCol { line: 2, column: 1 });
        assert_eq!(f.line_col(6), LineCol { line: 3, column: 1 });
    }

    #[test]
    fn offsets_count_chars_not_bytes() {
        // "é" is two bytes but one char; a byte-offset implementation would
        // report column 3 for the `x`.
        let f = SourceFile::new("t.mrt", "é x");
        assert_eq!(f.line_col(2), LineCol { line: 1, column: 3 });
        assert_eq!(f.slice(Span::new(0, 1)), "é");
    }

    #[test]
    fn line_text_excludes_the_newline() {
        let f = SourceFile::new("t.mrt", "first\nsecond\nthird");
        assert_eq!(f.line_text(1), "first");
        assert_eq!(f.line_text(2), "second");
        assert_eq!(f.line_text(3), "third");
        assert_eq!(f.line_text(9), "");
    }

    #[test]
    fn a_file_with_no_trailing_newline_still_has_its_last_line() {
        let f = SourceFile::new("t.mrt", "only");
        assert_eq!(f.line_count(), 1);
        assert_eq!(f.line_text(1), "only");
    }

    #[test]
    fn spans_join() {
        assert_eq!(Span::new(2, 4).to(Span::new(9, 11)), Span::new(2, 11));
        assert_eq!(Span::new(9, 11).to(Span::new(2, 4)), Span::new(2, 11));
    }

    #[test]
    fn compat_rendering_matches_the_python_format() {
        let d = Diagnostic::error(Stage::Lex, "Unterminated string", Span::new(4, 5), 3);
        assert_eq!(
            d.render_compat(),
            "Syntax Error: Unterminated string [line 3]"
        );
    }

    #[test]
    fn rich_rendering_points_at_the_span() {
        let f = SourceFile::new("main.mrt", "var a = 1;\nvar b = &;\n");
        let d = Diagnostic::error(Stage::Lex, "Unexpected character '&'", Span::new(19, 20), 2);
        let rendered = d.render(&f);
        assert!(rendered.contains("main.mrt:2:9"), "{rendered}");
        assert!(rendered.contains("2 | var b = &;"), "{rendered}");
        assert!(rendered.contains("  |         ^"), "{rendered}");
    }
}
