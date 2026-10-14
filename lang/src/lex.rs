//! Source text to statements, statements to tokens, and a cursor that the
//! parsers share.
//!
//! A statement starts on a line with no indentation. The indented lines
//! that follow are its continuation lines. `--` starts a comment that runs
//! to the end of the line, except inside a string.

use std::fmt;

/// An error with the file and line it comes from.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{file}:{line}: {msg}")]
pub struct Error {
    pub file: String,
    pub line: usize,
    pub msg: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    Word(String),
    Str(String),
    Num(String),
    /// The possessive `'s`.
    Poss,
    Sym(char),
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Word(w) | Tok::Num(w) => write!(f, "{w}"),
            Tok::Str(s) => write!(f, "\"{s}\""),
            Tok::Poss => write!(f, "'s"),
            Tok::Sym(c) => write!(f, "{c}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stmt {
    pub file: String,
    pub line: usize,
    /// The first line's tokens, then each continuation line's.
    pub lines: Vec<Vec<Tok>>,
}

impl Stmt {
    /// All tokens, continuation lines included.
    pub fn tokens(&self) -> Vec<Tok> {
        self.lines.concat()
    }

    pub fn error(&self, msg: impl Into<String>) -> Error {
        Error { file: self.file.clone(), line: self.line, msg: msg.into() }
    }
}

pub fn statements(file: &str, src: &str) -> Result<Vec<Stmt>, Error> {
    let mut out: Vec<Stmt> = Vec::new();
    for (i, raw) in src.lines().enumerate() {
        let line = i + 1;
        let text = strip_comment(raw);
        if text.trim().is_empty() {
            continue;
        }
        let toks = tokens(text).map_err(|msg| Error { file: file.to_string(), line, msg })?;
        let indented = text.starts_with(' ') || text.starts_with('\t');
        match out.last_mut() {
            Some(s) if indented => s.lines.push(toks),
            None if indented => {
                return Err(Error { file: file.to_string(), line, msg: "indented line outside a statement".into() });
            }
            _ => out.push(Stmt { file: file.to_string(), line, lines: vec![toks] }),
        }
    }
    Ok(out)
}

/// The line without its `--` comment. A `--` inside a string is kept.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_str = false;
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b'"' => in_str = !in_str,
            b'-' if !in_str && bytes.get(i + 1) == Some(&b'-') => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Splits one line into tokens.
pub fn tokens(text: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' {
            let start = i + 1;
            let mut j = start;
            while j < chars.len() && chars[j] != '"' {
                j += 1;
            }
            if j == chars.len() {
                return Err("unterminated string".into());
            }
            out.push(Tok::Str(chars[start..j].iter().collect()));
            i = j + 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            out.push(Tok::Num(chars[start..i].iter().collect()));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Word(chars[start..i].iter().collect()));
            let possessive = chars.get(i) == Some(&'\'')
                && chars.get(i + 1) == Some(&'s')
                && !chars.get(i + 2).is_some_and(|d| d.is_alphanumeric());
            if possessive {
                out.push(Tok::Poss);
                i += 2;
            }
        } else if "{}:,=|()".contains(c) {
            out.push(Tok::Sym(c));
            i += 1;
        } else {
            return Err(format!("unexpected character {c:?}"));
        }
    }
    Ok(out)
}

/// A position in a statement's tokens, with the helpers the parsers share.
pub struct Cursor<'a> {
    toks: &'a [Tok],
    pos: usize,
    file: &'a str,
    line: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(toks: &'a [Tok], file: &'a str, line: usize) -> Self {
        Cursor { toks, pos: 0, file, line }
    }

    pub fn error(&self, msg: impl Into<String>) -> Error {
        Error { file: self.file.to_string(), line: self.line, msg: msg.into() }
    }

    pub fn peek(&self) -> Option<&'a Tok> {
        self.toks.get(self.pos)
    }

    pub fn peek_at(&self, k: usize) -> Option<&'a Tok> {
        self.toks.get(self.pos + k)
    }

    pub fn next(&mut self) -> Option<&'a Tok> {
        let t = self.toks.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }

    /// True if the next token is the word `w`.
    pub fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(x)) if x == w)
    }

    /// Consumes the word `w` if it is next.
    pub fn eat_word(&mut self, w: &str) -> bool {
        let found = self.is_word(w);
        if found {
            self.pos += 1;
        }
        found
    }

    pub fn expect_word(&mut self, w: &str) -> Result<(), Error> {
        if self.eat_word(w) { Ok(()) } else { Err(self.error(format!("expected `{w}`, found {}", self.found()))) }
    }

    pub fn eat_sym(&mut self, c: char) -> bool {
        let found = matches!(self.peek(), Some(Tok::Sym(x)) if *x == c);
        if found {
            self.pos += 1;
        }
        found
    }

    pub fn expect_sym(&mut self, c: char) -> Result<(), Error> {
        if self.eat_sym(c) { Ok(()) } else { Err(self.error(format!("expected `{c}`, found {}", self.found()))) }
    }

    pub fn eat_poss(&mut self) -> bool {
        let found = matches!(self.peek(), Some(Tok::Poss));
        if found {
            self.pos += 1;
        }
        found
    }

    /// Consumes and returns the next word.
    pub fn word(&mut self) -> Result<String, Error> {
        match self.peek() {
            Some(Tok::Word(w)) => {
                self.pos += 1;
                Ok(w.clone())
            }
            _ => Err(self.error(format!("expected a word, found {}", self.found()))),
        }
    }

    /// Consumes and returns the next word, which must start with a capital letter.
    pub fn kind_name(&mut self) -> Result<String, Error> {
        match self.peek() {
            Some(Tok::Word(w)) if w.starts_with(char::is_uppercase) => {
                self.pos += 1;
                Ok(w.clone())
            }
            _ => Err(self.error(format!("expected a kind name, found {}", self.found()))),
        }
    }

    pub fn expect_end(&self) -> Result<(), Error> {
        if self.at_end() { Ok(()) } else { Err(self.error(format!("unexpected {} at the end", self.found()))) }
    }

    /// The next token, for error messages.
    pub fn found(&self) -> String {
        self.peek().map_or_else(|| "the end of the statement".to_string(), |t| format!("`{t}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Tok {
        Tok::Word(s.into())
    }

    #[test]
    fn possessives_strings_numbers_and_symbols() {
        let t = tokens(r#"its form's order's total is at most 120 EUR, "a -- b" {x: Text}"#).unwrap();
        assert_eq!(t[..6], [w("its"), w("form"), Tok::Poss, w("order"), Tok::Poss, w("total")]);
        assert!(t.contains(&Tok::Num("120".into())));
        assert!(t.contains(&Tok::Str("a -- b".into())));
        assert!(t.contains(&Tok::Sym('{')) && t.contains(&Tok::Sym(':')));
    }

    #[test]
    fn statements_join_indented_lines_and_drop_comments() {
        let src = "-- a comment\nkind A { x: Text }   -- trailing\n\neffect ask A via forms within 3 days\n  sends its x\n  reports filled\n";
        let s = statements("f.st", src).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].line, s[0].lines.len()), (2, 1));
        assert_eq!((s[1].line, s[1].lines.len()), (4, 3));
        assert_eq!(s[1].lines[1][0], w("sends"));
    }

    #[test]
    fn errors_name_the_file_and_line() {
        let e = statements("f.st", "kind A {\n  x: Text \u{2014} }\n").unwrap_err();
        assert_eq!((e.file.as_str(), e.line), ("f.st", 2));
        let e = statements("f.st", "  indented first").unwrap_err();
        assert_eq!(e.msg, "indented line outside a statement");
    }

    #[test]
    fn cursor_reads_words_and_reports_what_it_found() {
        let toks = tokens("rule paid : no Order").unwrap();
        let mut c = Cursor::new(&toks, "f.st", 7);
        assert!(c.eat_word("rule"));
        assert_eq!(c.word().unwrap(), "paid");
        c.expect_sym(':').unwrap();
        assert!(c.expect_word("every").is_err());
        assert!(c.eat_word("no"));
        assert_eq!(c.kind_name().unwrap(), "Order");
        assert!(c.at_end());
        assert_eq!(c.found(), "the end of the statement");
    }
}
