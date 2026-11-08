//! A tolerant reader for the INI dialect Qt's `QSettings` writes, which is the
//! format of Artisan `.aset` files.
//!
//! What it handles:
//! - `[Section]` headers and `key=value` lines; keys outside any section go to
//!   `General`. Section names and keys are percent-decoded (`pv%3F` → `pv?`).
//! - Lines starting with `;` or `#` are comments; blank lines are skipped.
//! - Values: a top-level comma makes a list (`a, b, c`, items trimmed);
//!   `"…"` quotes protect commas and spaces (`"OT1,{}", "OT2,{}"`);
//!   backslash escapes (`\n`, `\t`, `\\`, `\"`, `\0`, `\xHH`, …) are decoded.
//! - Typed values `@Variant(…)`, `@ByteArray(…)`, `@Invalid()` and the like are
//!   kept as [`IniValue::Opaque`] and never decoded; `@@` is a literal `@`.
//! - A line ending in an odd number of backslashes continues on the next line.
//!
//! Malformed lines are skipped and counted, never fatal.

use std::collections::BTreeMap;

/// A parsed INI file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QtIni {
    sections: BTreeMap<String, BTreeMap<String, IniValue>>,
    skipped_lines: Vec<usize>,
}

/// A value of a `QSettings` INI file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IniValue {
    /// A single string.
    Text(String),
    /// A comma-separated list.
    List(Vec<String>),
    /// A typed binary or special value, identified by its type name (for
    /// example `Variant`, `ByteArray`, `Invalid`). Not decoded.
    Opaque(String),
}

impl IniValue {
    /// The value as a single string; `None` for lists and opaque values.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            IniValue::Text(s) => Some(s),
            _ => None,
        }
    }

    /// The value as a list. A single string is a one-item list, an empty string
    /// or an opaque value an empty list.
    #[must_use]
    pub fn to_list(&self) -> Vec<String> {
        match self {
            IniValue::Text(s) if s.is_empty() => Vec::new(),
            IniValue::Text(s) => vec![s.clone()],
            IniValue::List(items) => items.clone(),
            IniValue::Opaque(_) => Vec::new(),
        }
    }
}

impl QtIni {
    /// Parses INI text. Never fails; lines that are neither a section header,
    /// a key/value pair, a comment nor blank are skipped and recorded in
    /// [`QtIni::skipped_lines`].
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut ini = QtIni::default();
        let mut section = String::from("General");
        let mut lines = text.lines().enumerate();

        while let Some((index, first)) = lines.next() {
            let mut line = first.to_string();
            while ends_with_odd_backslashes(&line) {
                line.pop();
                match lines.next() {
                    Some((_, next)) => line.push_str(next),
                    None => break,
                }
            }
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
                continue;
            }
            if let Some(name) = trimmed.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                section = percent_decode(name.trim());
                continue;
            }
            match trimmed.split_once('=') {
                Some((key, raw)) if !key.trim().is_empty() => {
                    ini.sections
                        .entry(section.clone())
                        .or_default()
                        .insert(percent_decode(key.trim()), parse_value(raw));
                }
                _ => ini.skipped_lines.push(index + 1),
            }
        }
        ini
    }

    /// The value of `key` in `section`.
    #[must_use]
    pub fn get(&self, section: &str, key: &str) -> Option<&IniValue> {
        self.sections.get(section).and_then(|s| s.get(key))
    }

    /// The value of `key` in `section` as a single string.
    #[must_use]
    pub fn text(&self, section: &str, key: &str) -> Option<&str> {
        self.get(section, key).and_then(IniValue::as_text)
    }

    /// The value of `key` in `section` as a list (empty if absent).
    #[must_use]
    pub fn list(&self, section: &str, key: &str) -> Vec<String> {
        self.get(section, key).map(IniValue::to_list).unwrap_or_default()
    }

    /// The names of all sections, sorted.
    pub fn sections(&self) -> impl Iterator<Item = &str> {
        self.sections.keys().map(String::as_str)
    }

    /// The keys of a section, sorted.
    pub fn keys(&self, section: &str) -> impl Iterator<Item = &str> {
        self.sections
            .get(section)
            .into_iter()
            .flat_map(|s| s.keys().map(String::as_str))
    }

    /// One-based numbers of the lines that could not be understood.
    #[must_use]
    pub fn skipped_lines(&self) -> &[usize] {
        &self.skipped_lines
    }
}

fn ends_with_odd_backslashes(line: &str) -> bool {
    line.chars().rev().take_while(|c| *c == '\\').count() % 2 == 1
}

/// Decodes `%XX` byte escapes and `%UXXXX` code point escapes in keys.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if bytes.get(i + 1) == Some(&b'U') {
                if let Some(c) = s
                    .get(i + 2..i + 6)
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .and_then(char::from_u32)
                {
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    i += 6;
                    continue;
                }
            } else if let Some(b) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses the raw text after `=`.
fn parse_value(raw: &str) -> IniValue {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix('@') {
        if !rest.starts_with('@') {
            let type_name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if !type_name.is_empty() && rest[type_name.len()..].starts_with('(') {
                return IniValue::Opaque(type_name);
            }
        }
    }
    let raw = raw.strip_prefix("@@").map_or_else(|| raw.to_string(), |r| format!("@{r}"));

    let mut items: Vec<String> = Vec::new();
    let mut item = Item::default();
    let mut saw_comma = false;
    let mut in_quotes = false;
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                item.mark_significant();
            }
            '\\' => {
                if let Some(decoded) = unescape(&mut chars) {
                    item.push_significant(decoded);
                }
            }
            ',' if !in_quotes => {
                items.push(item.finish());
                item = Item::default();
                saw_comma = true;
            }
            c if c.is_whitespace() && !in_quotes => item.push_space(c),
            c => item.push_significant(c),
        }
    }
    items.push(item.finish());

    if saw_comma {
        IniValue::List(items)
    } else {
        IniValue::Text(items.pop().unwrap_or_default())
    }
}

/// Decodes the escape after a backslash. Returns `None` for an escaped line
/// break (continuation) or a trailing backslash.
fn unescape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<char> {
    let c = chars.next()?;
    Some(match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        'a' => '\u{7}',
        'b' => '\u{8}',
        'f' => '\u{c}',
        'v' => '\u{b}',
        'x' => {
            let mut code = 0u32;
            let mut digits = 0;
            while let Some(d) = chars.peek().and_then(|d| d.to_digit(16)) {
                if digits == 4 {
                    break;
                }
                code = code * 16 + d;
                digits += 1;
                chars.next();
            }
            char::from_u32(code).unwrap_or('\u{fffd}')
        }
        '0'..='7' => {
            let mut code = c.to_digit(8).unwrap_or(0);
            let mut digits = 1;
            while let Some(d) = chars.peek().and_then(|d| d.to_digit(8)) {
                if digits == 3 {
                    break;
                }
                code = code * 8 + d;
                digits += 1;
                chars.next();
            }
            char::from_u32(code).unwrap_or('\u{fffd}')
        }
        '\n' | '\r' => return None,
        other => other,
    })
}

/// A list item being built: unquoted leading and trailing whitespace is
/// dropped, everything quoted or escaped is kept.
#[derive(Default)]
struct Item {
    text: String,
    significant_len: usize,
    started: bool,
}

impl Item {
    fn push_significant(&mut self, c: char) {
        self.text.push(c);
        self.significant_len = self.text.len();
        self.started = true;
    }

    fn push_space(&mut self, c: char) {
        if self.started {
            self.text.push(c);
        }
    }

    fn mark_significant(&mut self) {
        self.significant_len = self.text.len();
        self.started = true;
    }

    fn finish(mut self) -> String {
        self.text.truncate(self.significant_len);
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(raw: &str) -> IniValue {
        parse_value(raw)
    }

    #[test]
    fn sections_keys_and_general() {
        let ini = QtIni::parse("top=1\n[Device]\nid=19\n\n; comment\n# other\n[Serial%20Port]\nbaudrate = 115200\n");
        assert_eq!(ini.text("General", "top"), Some("1"));
        assert_eq!(ini.text("Device", "id"), Some("19"));
        assert_eq!(ini.text("Serial Port", "baudrate"), Some("115200"));
        assert!(ini.skipped_lines().is_empty());
        assert_eq!(ini.sections().collect::<Vec<_>>(), ["Device", "General", "Serial Port"]);
    }

    #[test]
    fn crlf_bom_and_percent_keys() {
        let ini = QtIni::parse("\u{feff}[PXG4]\r\npv%3F=0\r\nname%U00e4=x\r\n");
        assert_eq!(ini.text("PXG4", "pv?"), Some("0"));
        assert_eq!(ini.text("PXG4", "nameä"), Some("x"));
    }

    #[test]
    fn plain_text_keeps_inner_spaces() {
        assert_eq!(
            value("Skywalker Series - Cubean +"),
            IniValue::Text("Skywalker Series - Cubean +".into())
        );
        assert_eq!(value(""), IniValue::Text(String::new()));
        assert_eq!(value("  padded  "), IniValue::Text("padded".into()));
    }

    #[test]
    fn lists_and_empty_items() {
        assert_eq!(
            value("95, 95, 95, 95"),
            IniValue::List(vec!["95".into(), "95".into(), "95".into(), "95".into()])
        );
        assert_eq!(
            value("{3}, SV, "),
            IniValue::List(vec!["{3}".into(), "SV".into(), String::new()])
        );
        assert_eq!(value(", , ").to_list(), vec![String::new(); 3]);
    }

    #[test]
    fn quoted_items_protect_commas() {
        assert_eq!(
            value(r#""OT2,{}", "OT3,{}", , "OT1,{}""#),
            IniValue::List(vec![
                "OT2,{}".into(),
                "OT3,{}".into(),
                String::new(),
                "OT1,{}".into()
            ])
        );
        assert_eq!(value(r#""OT1,{}""#), IniValue::Text("OT1,{}".into()));
        assert_eq!(value(r#"" spaced ""#), IniValue::Text(" spaced ".into()));
    }

    #[test]
    fn escapes() {
        assert_eq!(
            value(r"+5%\n\\t, \\1\nBeans Cooling").to_list(),
            vec!["+5%\n\\t".to_string(), "\\1\nBeans Cooling".to_string()]
        );
        assert_eq!(value(r#"say \"hi\""#), IniValue::Text("say \"hi\"".into()));
        assert_eq!(value(r"\x41\x3b"), IniValue::Text("A;".into()));
        assert_eq!(value(r"a\0b"), IniValue::Text("a\0b".into()));
    }

    #[test]
    fn opaque_values() {
        assert_eq!(
            value(r"@Variant(\0\0\0\t\0\0\0\x1c, x)"),
            IniValue::Opaque("Variant".into())
        );
        assert_eq!(value("@Invalid()"), IniValue::Opaque("Invalid".into()));
        assert_eq!(value("@ByteArray(abc)").to_list(), Vec::<String>::new());
        assert_eq!(value("@@literal"), IniValue::Text("@literal".into()));
    }

    #[test]
    fn continuation_and_skipped_lines() {
        let ini = QtIni::parse("[A]\nlong=one, \\\ntwo\nnot a pair\n=novalue\n");
        assert_eq!(ini.list("A", "long"), vec!["one".to_string(), "two".to_string()]);
        assert_eq!(ini.skipped_lines(), &[4, 5]);
    }

    #[test]
    fn missing_values() {
        let ini = QtIni::parse("[A]\nx=1\n");
        assert_eq!(ini.get("A", "y"), None);
        assert_eq!(ini.get("B", "x"), None);
        assert!(ini.list("A", "y").is_empty());
        assert_eq!(ini.keys("A").collect::<Vec<_>>(), ["x"]);
    }
}
