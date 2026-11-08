//! Python literals: the value model, a recursive-descent parser and a writer.
//!
//! An Artisan `.alog` file is one Python dictionary literal. This module reads
//! and writes the subset of Python that such files use — dictionaries, lists,
//! tuples, strings, integers, floats, `True`, `False` and `None` — without
//! depending on Python.
//!
//! The writer follows Python's `repr` conventions (`{'a': [1, 2.5]}`,
//! shortest round-trip floats, `1e-05` style exponents), so a file that was
//! parsed and written again without changes is byte-for-byte identical as long
//! as it only contains ASCII. Non-ASCII characters are written as `\x`, `\u`
//! or `\U` escapes so the output is always plain ASCII. Everything it writes is
//! accepted by Python's `ast.literal_eval`, with one exception: NaN has no
//! literal form and is written as `nan` (Python's `repr`). Infinities are
//! written as `1e999` and `-1e999`, which `ast.literal_eval` reads back as
//! infinities.

use std::fmt;

use thiserror::Error;

/// Maximum nesting depth of containers. Artisan files nest three levels deep;
/// the limit only protects the parser's stack against hostile input.
const MAX_DEPTH: usize = 128;

/// A Python literal value.
///
/// Dictionaries keep their insertion order (like Python's `dict`), so a parsed
/// file can be written back in its original order.
#[derive(Debug, Clone, PartialEq)]
pub enum PyValue {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<PyValue>),
    Tuple(Vec<PyValue>),
    Dict(Vec<(PyValue, PyValue)>),
}

impl PyValue {
    /// Python's name of the value's type, for error messages.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            PyValue::None => "None",
            PyValue::Bool(_) => "bool",
            PyValue::Int(_) => "int",
            PyValue::Float(_) => "float",
            PyValue::Str(_) => "str",
            PyValue::List(_) => "list",
            PyValue::Tuple(_) => "tuple",
            PyValue::Dict(_) => "dict",
        }
    }

    /// A string value.
    #[must_use]
    pub fn str(s: impl Into<String>) -> Self {
        PyValue::Str(s.into())
    }

    /// The value as a float, for `int` and `float` values.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            PyValue::Int(i) => Some(i as f64),
            PyValue::Float(f) => Some(f),
            _ => None,
        }
    }

    /// The value as an integer, for `int` values and floats without a
    /// fractional part.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            PyValue::Int(i) => Some(i),
            PyValue::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(f as i64),
            _ => None,
        }
    }

    /// The value as a string slice, for `str` values.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PyValue::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The value as a bool, for `bool` values.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            PyValue::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// The elements of a list or tuple.
    #[must_use]
    pub fn as_list(&self) -> Option<&[PyValue]> {
        match self {
            PyValue::List(items) | PyValue::Tuple(items) => Some(items),
            _ => None,
        }
    }

    /// The entries of a dictionary.
    #[must_use]
    pub fn as_dict(&self) -> Option<&[(PyValue, PyValue)]> {
        match self {
            PyValue::Dict(entries) => Some(entries),
            _ => None,
        }
    }

    /// Looks up a string key in a dictionary. Returns `None` for other values.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&PyValue> {
        self.as_dict()?
            .iter()
            .find(|(k, _)| k.as_str() == Some(key))
            .map(|(_, v)| v)
    }

    /// Mutable lookup of a string key in a dictionary.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut PyValue> {
        match self {
            PyValue::Dict(entries) => entries
                .iter_mut()
                .find(|(k, _)| k.as_str() == Some(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets a string key in a dictionary, replacing an existing value in place
    /// or appending a new entry. Does nothing for other values.
    pub fn insert(&mut self, key: &str, value: PyValue) {
        if let PyValue::Dict(entries) = self {
            match entries.iter_mut().find(|(k, _)| k.as_str() == Some(key)) {
                Some((_, v)) => *v = value,
                None => entries.push((PyValue::str(key), value)),
            }
        }
    }

    /// Removes every entry whose string key matches the predicate. Does nothing
    /// for values that are not dictionaries.
    pub fn remove_keys_where(&mut self, mut predicate: impl FnMut(&str) -> bool) {
        if let PyValue::Dict(entries) = self {
            entries.retain(|(k, _)| !k.as_str().is_some_and(&mut predicate));
        }
    }

    /// Writes the value as a Python literal.
    #[must_use]
    pub fn to_literal(&self) -> String {
        let mut out = String::new();
        // Writing into a String cannot fail.
        let _ = write_value(&mut out, self);
        out
    }
}

impl fmt::Display for PyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_value(f, self)
    }
}

impl From<f64> for PyValue {
    fn from(v: f64) -> Self {
        PyValue::Float(v)
    }
}

impl From<i64> for PyValue {
    fn from(v: i64) -> Self {
        PyValue::Int(v)
    }
}

impl From<bool> for PyValue {
    fn from(v: bool) -> Self {
        PyValue::Bool(v)
    }
}

impl From<&str> for PyValue {
    fn from(v: &str) -> Self {
        PyValue::Str(v.to_owned())
    }
}

impl From<String> for PyValue {
    fn from(v: String) -> Self {
        PyValue::Str(v)
    }
}

impl<T: Into<PyValue>> From<Vec<T>> for PyValue {
    fn from(v: Vec<T>) -> Self {
        PyValue::List(v.into_iter().map(Into::into).collect())
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A syntax error in a Python literal, with its position.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message} at byte {offset} (line {line}, column {column})")]
pub struct ParseError {
    /// What went wrong.
    pub message: String,
    /// Byte offset into the input where the problem was found.
    pub offset: usize,
    /// 1-based line number of `offset`.
    pub line: usize,
    /// 1-based column (in characters) of `offset`.
    pub column: usize,
}

impl ParseError {
    fn new(input: &str, offset: usize, message: impl Into<String>) -> Self {
        let offset = offset.min(input.len());
        let mut start = offset;
        while !input.is_char_boundary(start) {
            start -= 1;
        }
        let before = &input[..start];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before[line_start..].chars().count() + 1;
        ParseError {
            message: message.into(),
            offset,
            line,
            column,
        }
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parses one Python literal. Leading and trailing whitespace and comments are
/// allowed; anything else after the value is an error.
///
/// # Errors
///
/// Returns a [`ParseError`] with the byte offset of the first problem.
pub fn parse(input: &str) -> Result<PyValue, ParseError> {
    let mut parser = Parser {
        input,
        bytes: input.as_bytes(),
        pos: 0,
        depth: 0,
    };
    // A UTF-8 byte order mark is tolerated at the very start.
    if input.starts_with('\u{feff}') {
        parser.pos = '\u{feff}'.len_utf8();
    }
    parser.skip_ws();
    let value = parser.value()?;
    parser.skip_ws();
    if parser.pos < parser.bytes.len() {
        return Err(parser.error("unexpected content after the value"));
    }
    Ok(value)
}

struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError::new(self.input, self.pos, message)
    }

    fn error_at(&self, offset: usize, message: impl Into<String>) -> ParseError {
        ParseError::new(self.input, offset, message)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' => self.pos += 1,
                // An explicit line join, as Python allows between tokens.
                b'\\' if matches!(self.peek_at(1), Some(b'\n' | b'\r')) => self.pos += 2,
                b'#' => {
                    while let Some(b) = self.peek() {
                        if b == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn describe_here(&self) -> String {
        match self.input[self.pos..].chars().next() {
            Some(c) => format!("unexpected character {c:?}"),
            None => "unexpected end of input".to_owned(),
        }
    }

    fn value(&mut self) -> Result<PyValue, ParseError> {
        match self.peek() {
            None => Err(self.error("unexpected end of input, expected a value")),
            Some(b'{') => self.container(b'}', Kind::Dict),
            Some(b'[') => self.container(b']', Kind::List),
            Some(b'(') => self.container(b')', Kind::Tuple),
            Some(b'\'' | b'"') => self.strings(),
            Some(b'-' | b'+') => self.signed(),
            Some(b'0'..=b'9' | b'.') => self.number(false),
            Some(b) if b.is_ascii_alphabetic() || b == b'_' => {
                if self.at_string_prefix() {
                    self.strings()
                } else {
                    self.name()
                }
            }
            Some(_) => Err(self.error(format!("{}, expected a value", self.describe_here()))),
        }
    }

    /// Whether the input continues with a string prefix (`u`, `r`, `b`, …)
    /// immediately followed by a quote.
    fn at_string_prefix(&self) -> bool {
        let mut i = 0;
        while i < 2 {
            match self.peek_at(i) {
                Some(b'u' | b'U' | b'r' | b'R' | b'b' | b'B' | b'f' | b'F') => i += 1,
                Some(b'\'' | b'"') => return i > 0,
                _ => return false,
            }
        }
        matches!(self.peek_at(i), Some(b'\'' | b'"'))
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error(format!("nesting deeper than {MAX_DEPTH} levels")));
        }
        Ok(())
    }

    fn container(&mut self, close: u8, kind: Kind) -> Result<PyValue, ParseError> {
        let open_at = self.pos;
        self.enter()?;
        self.pos += 1;
        let mut items = Vec::new();
        let mut entries = Vec::new();
        let mut saw_comma = false;
        loop {
            self.skip_ws();
            match self.peek() {
                None => {
                    return Err(self.error_at(
                        open_at,
                        format!("unclosed '{}'", char::from(kind.open())),
                    ));
                }
                Some(b) if b == close => {
                    self.pos += 1;
                    break;
                }
                Some(_) => {}
            }
            let item_at = self.pos;
            let item = self.value()?;
            self.skip_ws();
            if kind == Kind::Dict {
                if self.peek() != Some(b':') {
                    if matches!(self.peek(), Some(b',' | b'}')) {
                        return Err(self.error_at(item_at, "set literals are not supported"));
                    }
                    return Err(self.error(format!("{}, expected ':'", self.describe_here())));
                }
                self.pos += 1;
                self.skip_ws();
                let value = self.value()?;
                entries.push((item, value));
                self.skip_ws();
            } else {
                items.push(item);
            }
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    saw_comma = true;
                }
                Some(b) if b == close => {}
                _ => {
                    return Err(self.error(format!(
                        "{}, expected ',' or '{}'",
                        self.describe_here(),
                        char::from(close)
                    )));
                }
            }
        }
        self.depth -= 1;
        Ok(match kind {
            Kind::Dict => PyValue::Dict(entries),
            Kind::List => PyValue::List(items),
            // `(x)` is a parenthesised value, `(x,)` a one-element tuple.
            Kind::Tuple if items.len() == 1 && !saw_comma => {
                items.pop().unwrap_or(PyValue::Tuple(Vec::new()))
            }
            Kind::Tuple => PyValue::Tuple(items),
        })
    }

    fn signed(&mut self) -> Result<PyValue, ParseError> {
        let start = self.pos;
        let mut negative = false;
        while let Some(b @ (b'-' | b'+')) = self.peek() {
            if b == b'-' {
                negative = !negative;
            }
            self.pos += 1;
            self.skip_ws();
        }
        match self.peek() {
            Some(b'0'..=b'9' | b'.') => self.number(negative),
            Some(b) if b.is_ascii_alphabetic() => match self.name()? {
                PyValue::Float(f) => Ok(PyValue::Float(if negative { -f } else { f })),
                _ => Err(self.error_at(start, "a sign must be followed by a number")),
            },
            _ => Err(self.error_at(start, "a sign must be followed by a number")),
        }
    }

    fn number(&mut self, negative: bool) -> Result<PyValue, ParseError> {
        let start = self.pos;
        // Integers in other bases.
        if self.peek() == Some(b'0')
            && let Some(radix) = match self.peek_at(1) {
                Some(b'x' | b'X') => Some(16),
                Some(b'o' | b'O') => Some(8),
                Some(b'b' | b'B') => Some(2),
                _ => None,
            }
        {
            self.pos += 2;
            let digits_at = self.pos;
            while self
                .peek()
                .is_some_and(|b| b == b'_' || char::from(b).is_digit(radix))
            {
                self.pos += 1;
            }
            let digits: String = self.input[digits_at..self.pos]
                .chars()
                .filter(|&c| c != '_')
                .collect();
            if digits.is_empty() {
                return Err(self.error_at(start, "missing digits after the base prefix"));
            }
            let value = i64::from_str_radix(&digits, radix)
                .map_err(|_| self.error_at(start, "integer out of range"))?;
            self.reject_trailing_identifier(start)?;
            return Ok(PyValue::Int(if negative { -value } else { value }));
        }

        let mut is_float = false;
        self.digits();
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let mark = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.peek().is_some_and(|b| b.is_ascii_digit()) {
                is_float = true;
                self.digits();
            } else {
                self.pos = mark;
            }
        }
        if matches!(self.peek(), Some(b'j' | b'J')) {
            return Err(self.error_at(start, "complex numbers are not supported"));
        }
        self.reject_trailing_identifier(start)?;
        let text: String = self.input[start..self.pos]
            .chars()
            .filter(|&c| c != '_')
            .collect();
        if text == "." || text.is_empty() {
            return Err(self.error_at(start, "expected a number"));
        }
        if is_float {
            let value: f64 = text
                .parse()
                .map_err(|_| self.error_at(start, format!("invalid float {text:?}")))?;
            Ok(PyValue::Float(if negative { -value } else { value }))
        } else {
            if text.len() > 1 && text.starts_with('0') && text.bytes().any(|b| b != b'0') {
                return Err(self.error_at(start, "leading zeros are not allowed in integers"));
            }
            let signed = if negative {
                format!("-{text}")
            } else {
                text
            };
            signed
                .parse::<i64>()
                .map(PyValue::Int)
                .map_err(|_| self.error_at(start, "integer out of range"))
        }
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_digit() || b == b'_') {
            self.pos += 1;
        }
    }

    fn reject_trailing_identifier(&self, start: usize) -> Result<(), ParseError> {
        if self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(self.error_at(start, "invalid number"));
        }
        Ok(())
    }

    fn name(&mut self) -> Result<PyValue, ParseError> {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.pos += 1;
        }
        match &self.input[start..self.pos] {
            "True" => Ok(PyValue::Bool(true)),
            "False" => Ok(PyValue::Bool(false)),
            "None" => Ok(PyValue::None),
            // Not Python literals, but what Python's `repr` writes for these
            // floats; tolerated so such files can still be read.
            "inf" | "Infinity" => Ok(PyValue::Float(f64::INFINITY)),
            "nan" | "NaN" => Ok(PyValue::Float(f64::NAN)),
            other => Err(self.error_at(start, format!("unknown name `{other}`"))),
        }
    }

    /// One string literal, or several adjacent ones, which Python joins.
    fn strings(&mut self) -> Result<PyValue, ParseError> {
        let mut out = self.string()?;
        loop {
            let mark = self.pos;
            self.skip_ws();
            if matches!(self.peek(), Some(b'\'' | b'"')) || self.at_string_prefix() {
                out.push_str(&self.string()?);
            } else {
                self.pos = mark;
                return Ok(PyValue::Str(out));
            }
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        let mut raw = false;
        while let Some(b) = self.peek() {
            match b {
                b'u' | b'U' => self.pos += 1,
                b'r' | b'R' => {
                    raw = true;
                    self.pos += 1;
                }
                b'b' | b'B' => return Err(self.error("bytes literals are not supported")),
                b'f' | b'F' => return Err(self.error("f-strings are not literals")),
                _ => break,
            }
        }
        let Some(quote) = self.peek() else {
            return Err(self.error("unexpected end of input, expected a string"));
        };
        let triple = self.peek_at(1) == Some(quote) && self.peek_at(2) == Some(quote);
        self.pos += if triple { 3 } else { 1 };

        let mut out = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.error_at(start, "unterminated string"));
            };
            if b == quote {
                if !triple {
                    self.pos += 1;
                    return Ok(out);
                }
                if self.peek_at(1) == Some(quote) && self.peek_at(2) == Some(quote) {
                    self.pos += 3;
                    return Ok(out);
                }
            }
            match b {
                b'\n' | b'\r' if !triple => {
                    return Err(self.error_at(start, "unterminated string (end of line)"));
                }
                b'\\' if raw => {
                    // In raw strings a backslash only protects the next
                    // character from ending the string; both are kept.
                    out.push('\\');
                    self.pos += 1;
                    if let Some(c) = self.input[self.pos..].chars().next() {
                        out.push(c);
                        self.pos += c.len_utf8();
                    }
                }
                b'\\' => self.escape(&mut out)?,
                _ => {
                    let rest = &self.input[self.pos..];
                    let len = rest
                        .bytes()
                        .position(|c| c == quote || c == b'\\' || c == b'\n' || c == b'\r')
                        .unwrap_or(rest.len())
                        .max(1);
                    let mut chunk_end = self.pos + len;
                    while !self.input.is_char_boundary(chunk_end) {
                        chunk_end += 1;
                    }
                    out.push_str(&self.input[self.pos..chunk_end]);
                    self.pos = chunk_end;
                }
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), ParseError> {
        let at = self.pos;
        self.pos += 1;
        let Some(c) = self.input[self.pos..].chars().next() else {
            return Err(self.error_at(at, "unterminated escape sequence"));
        };
        self.pos += c.len_utf8();
        match c {
            '\n' => {}
            '\r' => {
                if self.peek() == Some(b'\n') {
                    self.pos += 1;
                }
            }
            '\\' => out.push('\\'),
            '\'' => out.push('\''),
            '"' => out.push('"'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'a' => out.push('\x07'),
            'b' => out.push('\x08'),
            'f' => out.push('\x0c'),
            'v' => out.push('\x0b'),
            '0'..='7' => {
                let mut value = c.to_digit(8).unwrap_or(0);
                for _ in 0..2 {
                    match self.peek() {
                        Some(d @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(d - b'0');
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                out.push(char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER));
            }
            'x' => {
                let value = self.hex_digits(2, at)?;
                out.push(char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER));
            }
            'u' | 'U' => {
                let value = self.hex_digits(if c == 'u' { 4 } else { 8 }, at)?;
                self.push_code_point(out, value, at)?;
            }
            'N' => return Err(self.error_at(at, "named unicode escapes (\\N{...}) are not supported")),
            other => {
                // Python keeps unknown escapes as they are.
                out.push('\\');
                out.push(other);
            }
        }
        Ok(())
    }

    fn hex_digits(&mut self, count: usize, at: usize) -> Result<u32, ParseError> {
        let end = self.pos + count;
        let digits = self
            .input
            .get(self.pos..end)
            .filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| self.error_at(at, format!("expected {count} hex digits in escape")))?;
        let value = u32::from_str_radix(digits, 16)
            .map_err(|_| self.error_at(at, "invalid hex escape"))?;
        self.pos = end;
        Ok(value)
    }

    /// Pushes a code point from a `\u`/`\U` escape. A UTF-16 surrogate pair
    /// written as two escapes is combined; a lone surrogate, which a Rust
    /// string cannot hold, becomes U+FFFD.
    fn push_code_point(&mut self, out: &mut String, value: u32, at: usize) -> Result<(), ParseError> {
        if (0xD800..0xDC00).contains(&value)
            && self.peek() == Some(b'\\')
            && self.peek_at(1) == Some(b'u')
        {
            let mark = self.pos;
            self.pos += 2;
            let low = self.hex_digits(4, mark)?;
            if (0xDC00..0xE000).contains(&low) {
                let combined = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00);
                out.push(char::from_u32(combined).unwrap_or(char::REPLACEMENT_CHARACTER));
                return Ok(());
            }
            self.pos = mark;
        }
        if value > 0x10_FFFF {
            return Err(self.error_at(at, "escape is beyond the Unicode range"));
        }
        out.push(char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER));
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Dict,
    List,
    Tuple,
}

impl Kind {
    fn open(self) -> u8 {
        match self {
            Kind::Dict => b'{',
            Kind::List => b'[',
            Kind::Tuple => b'(',
        }
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

fn write_value<W: fmt::Write>(out: &mut W, value: &PyValue) -> fmt::Result {
    match value {
        PyValue::None => out.write_str("None"),
        PyValue::Bool(true) => out.write_str("True"),
        PyValue::Bool(false) => out.write_str("False"),
        PyValue::Int(i) => write!(out, "{i}"),
        PyValue::Float(f) => write_float(out, *f),
        PyValue::Str(s) => write_str(out, s),
        PyValue::List(items) => {
            out.write_char('[')?;
            write_items(out, items)?;
            out.write_char(']')
        }
        PyValue::Tuple(items) => {
            out.write_char('(')?;
            write_items(out, items)?;
            if items.len() == 1 {
                out.write_char(',')?;
            }
            out.write_char(')')
        }
        PyValue::Dict(entries) => {
            out.write_char('{')?;
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.write_str(", ")?;
                }
                write_value(out, k)?;
                out.write_str(": ")?;
                write_value(out, v)?;
            }
            out.write_char('}')
        }
    }
}

fn write_items<W: fmt::Write>(out: &mut W, items: &[PyValue]) -> fmt::Result {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.write_str(", ")?;
        }
        write_value(out, item)?;
    }
    Ok(())
}

/// Writes a float the way Python's `repr` does: the shortest digits that read
/// back to the same value, positional notation for exponents from -4 to 15 and
/// scientific notation (`1e-05`, `1e+16`) otherwise.
fn write_float<W: fmt::Write>(out: &mut W, f: f64) -> fmt::Result {
    if f.is_nan() {
        return out.write_str("nan");
    }
    if f.is_infinite() {
        return out.write_str(if f > 0.0 { "1e999" } else { "-1e999" });
    }
    if f.is_sign_negative() {
        out.write_char('-')?;
    }
    // Rust's `{:e}` gives the shortest round-trip digits, for example `1.2345e3`.
    let sci = format!("{:e}", f.abs());
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = if digits.is_empty() { "0".to_owned() } else { digits };

    if (-4..16).contains(&exponent) {
        if exponent < 0 {
            out.write_str("0.")?;
            for _ in 0..(-exponent - 1) {
                out.write_char('0')?;
            }
            out.write_str(&digits)
        } else {
            let int_len = usize::try_from(exponent).unwrap_or(0) + 1;
            if digits.len() <= int_len {
                out.write_str(&digits)?;
                for _ in digits.len()..int_len {
                    out.write_char('0')?;
                }
                out.write_str(".0")
            } else {
                out.write_str(&digits[..int_len])?;
                out.write_char('.')?;
                out.write_str(&digits[int_len..])
            }
        }
    } else {
        out.write_str(&digits[..1])?;
        if digits.len() > 1 {
            out.write_char('.')?;
            out.write_str(&digits[1..])?;
        }
        let sign = if exponent < 0 { '-' } else { '+' };
        write!(out, "e{sign}{:02}", exponent.unsigned_abs())
    }
}

/// Writes a string literal the way Python's `repr` does for ASCII text: single
/// quotes unless the text contains a single quote and no double quote. Every
/// non-ASCII character is escaped, so the output is pure ASCII.
fn write_str<W: fmt::Write>(out: &mut W, s: &str) -> fmt::Result {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    out.write_char(quote)?;
    for c in s.chars() {
        match c {
            '\\' => out.write_str("\\\\")?,
            '\n' => out.write_str("\\n")?,
            '\r' => out.write_str("\\r")?,
            '\t' => out.write_str("\\t")?,
            c if c == quote => {
                out.write_char('\\')?;
                out.write_char(c)?;
            }
            ' '..='~' => out.write_char(c)?,
            c => {
                let code = u32::from(c);
                if code <= 0xff {
                    write!(out, "\\x{code:02x}")?;
                } else if code <= 0xffff {
                    write!(out, "\\u{code:04x}")?;
                } else {
                    write!(out, "\\U{code:08x}")?;
                }
            }
        }
    }
    out.write_char(quote)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PyValue {
        parse(s).unwrap_or_else(|e| panic!("{s:?}: {e}"))
    }

    #[test]
    fn scalars() {
        assert_eq!(p("True"), PyValue::Bool(true));
        assert_eq!(p("False"), PyValue::Bool(false));
        assert_eq!(p("None"), PyValue::None);
        assert_eq!(p("42"), PyValue::Int(42));
        assert_eq!(p("-1"), PyValue::Int(-1));
        assert_eq!(p("- 1"), PyValue::Int(-1));
        assert_eq!(p("+7"), PyValue::Int(7));
        assert_eq!(p("0"), PyValue::Int(0));
        assert_eq!(p("1_000"), PyValue::Int(1000));
        assert_eq!(p("0x1F"), PyValue::Int(31));
        assert_eq!(p("-0o17"), PyValue::Int(-15));
        assert_eq!(p("0b101"), PyValue::Int(5));
        assert_eq!(p("1.5"), PyValue::Float(1.5));
        assert_eq!(p("-1.0"), PyValue::Float(-1.0));
        assert_eq!(p("1."), PyValue::Float(1.0));
        assert_eq!(p(".5"), PyValue::Float(0.5));
        assert_eq!(p("6.288e-05"), PyValue::Float(6.288e-05));
        assert_eq!(p("1E3"), PyValue::Float(1000.0));
        assert_eq!(p("-2.5e+2"), PyValue::Float(-250.0));
        assert_eq!(p("1e999"), PyValue::Float(f64::INFINITY));
        assert_eq!(p("-inf"), PyValue::Float(f64::NEG_INFINITY));
        assert!(p("nan").as_f64().is_some_and(f64::is_nan));
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(p("'abc'"), PyValue::str("abc"));
        assert_eq!(p("\"a'b\""), PyValue::str("a'b"));
        assert_eq!(p(r"'a\'b'"), PyValue::str("a'b"));
        assert_eq!(p(r#""a\"b""#), PyValue::str("a\"b"));
        assert_eq!(p(r"'a\\b'"), PyValue::str("a\\b"));
        assert_eq!(p(r"'1\n2\t3\r'"), PyValue::str("1\n2\t3\r"));
        assert_eq!(p(r"'Catua\xed'"), PyValue::str("Catuaí"));
        assert_eq!(p(r"'酸质'"), PyValue::str("酸质"));
        assert_eq!(p(r"'\U0001F600'"), PyValue::str("😀"));
        assert_eq!(p(r"'😀'"), PyValue::str("😀"));
        assert_eq!(p(r"'\101\0'"), PyValue::str("A\0"));
        assert_eq!(p(r"'\q'"), PyValue::str("\\q"));
        assert_eq!(p("'grüße'"), PyValue::str("grüße"));
        assert_eq!(p("u'x'"), PyValue::str("x"));
        assert_eq!(p(r"r'\n'"), PyValue::str("\\n"));
        assert_eq!(p("'a' \"b\""), PyValue::str("ab"));
        assert_eq!(p("'''x\n'y'''"), PyValue::str("x\n'y"));
        assert_eq!(p("'a\\\nb'"), PyValue::str("ab"));
    }

    #[test]
    fn containers() {
        assert_eq!(p("[]"), PyValue::List(vec![]));
        assert_eq!(p("[1, 2,]"), PyValue::from(vec![1_i64, 2]));
        assert_eq!(p("()"), PyValue::Tuple(vec![]));
        assert_eq!(p("(1,)"), PyValue::Tuple(vec![PyValue::Int(1)]));
        assert_eq!(p("(1)"), PyValue::Int(1));
        assert_eq!(
            p("(1, 'a')"),
            PyValue::Tuple(vec![PyValue::Int(1), PyValue::str("a")])
        );
        let d = p("{'a': [1, -1.0], 'b': {'c': None}, 3: (True,)} # comment\n");
        assert_eq!(d.get("a"), Some(&PyValue::List(vec![PyValue::Int(1), PyValue::Float(-1.0)])));
        assert_eq!(d.get("b").and_then(|b| b.get("c")), Some(&PyValue::None));
        assert_eq!(d.as_dict().map(<[_]>::len), Some(3));
        assert_eq!(d.get("missing"), None);
    }

    #[test]
    fn errors_carry_the_offset() {
        let e = parse("{'a': 1,\n 'b' 2}").unwrap_err();
        assert_eq!(e.offset, 14);
        assert_eq!((e.line, e.column), (2, 6));
        assert!(e.to_string().contains("expected ':'"), "{e}");
        assert!(e.to_string().contains("at byte 14"), "{e}");

        assert_eq!(parse("[1, 2").unwrap_err().offset, 5);
        assert_eq!(parse("'abc").unwrap_err().offset, 0);
        assert_eq!(parse("{'a': foo}").unwrap_err().offset, 6);
        assert_eq!(parse("1 2").unwrap_err().offset, 2);
        assert_eq!(parse("").unwrap_err().offset, 0);
        assert!(parse("{1, 2}").unwrap_err().message.contains("set"));
        assert!(parse("99999999999999999999").unwrap_err().message.contains("range"));
        assert!(parse("1j").is_err());
        assert!(parse("b'x'").is_err());
        assert!(parse("012").is_err());
        assert!(parse("1abc").is_err());
        assert!(parse(&"[".repeat(MAX_DEPTH + 1)).unwrap_err().message.contains("nesting"));
    }

    #[test]
    fn floats_are_written_like_python_repr() {
        let cases = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (-1.0, "-1.0"),
            (200.1, "200.1"),
            (0.691741916, "0.691741916"),
            (1.5, "1.5"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (6.288e-05, "6.288e-05"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (1.5e300, "1.5e+300"),
            (123456.789, "123456.789"),
            (0.1 + 0.2, "0.30000000000000004"),
            (f64::INFINITY, "1e999"),
            (f64::NEG_INFINITY, "-1e999"),
        ];
        for (value, expected) in cases {
            assert_eq!(PyValue::Float(value).to_literal(), expected, "{value}");
        }
    }

    #[test]
    fn strings_are_written_like_python_repr() {
        let cases = [
            ("abc", "'abc'"),
            ("a'b", "\"a'b\""),
            ("a'b\"c", r#"'a\'b"c'"#),
            ("a\\nb", r"'a\\nb'"),
            ("1\n2", r"'1\n2'"),
            ("Catuaí", r"'Catua\xed'"),
            ("酸", r"'\u9178'"),
            ("😀", r"'\U0001f600'"),
            ("\x01", r"'\x01'"),
        ];
        for (value, expected) in cases {
            assert_eq!(PyValue::str(value).to_literal(), expected, "{value:?}");
        }
    }

    #[test]
    fn containers_are_written_like_python_repr() {
        let v = p("{'a': [1, 2.5, None], 'b': (1,), 'c': (), 'd': {}, 'e': True}");
        assert_eq!(
            v.to_literal(),
            "{'a': [1, 2.5, None], 'b': (1,), 'c': (), 'd': {}, 'e': True}"
        );
    }

    #[test]
    fn write_then_parse_round_trips() {
        let v = PyValue::Dict(vec![
            (PyValue::str("s"), PyValue::str("it's \"quoted\" \\ \n\t ü € 😀 \x7f")),
            (PyValue::str("f"), PyValue::from(vec![0.1, -3.25e-7, 1e22, 5e-324, f64::MAX])),
            (PyValue::str("i"), PyValue::from(vec![i64::MIN + 1, -1, 0, i64::MAX])),
            (PyValue::str("t"), PyValue::Tuple(vec![PyValue::None, PyValue::Bool(false)])),
        ]);
        assert_eq!(parse(&v.to_literal()).unwrap(), v);
    }

    #[test]
    fn dict_helpers() {
        let mut d = p("{'a': 1, 'plus_x': 2, 'b': 3}");
        d.insert("a", PyValue::Int(10));
        d.insert("c", PyValue::Int(4));
        d.remove_keys_where(|k| k.starts_with("plus_"));
        assert_eq!(d.to_literal(), "{'a': 10, 'b': 3, 'c': 4}");
        if let Some(v) = d.get_mut("b") {
            *v = PyValue::None;
        }
        assert_eq!(d.get("b"), Some(&PyValue::None));
    }
}
