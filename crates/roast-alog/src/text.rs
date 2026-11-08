//! The second escaping layer of Artisan's free-text fields.
//!
//! In real `.alog` files, free text such as `beans`, `roastingnotes` or
//! `flavorlabels` is stored with its own backslash escapes *inside* the
//! Python string: a line break typed into a note appears as the two characters
//! `\n`, `í` as `\xed` and `酸` as `酸` (written in the file as `'\\n'`,
//! `'\\xed'`, `'\\u9178'`). The scheme is the one of Python's `unicode_escape`
//! codec. These helpers convert between that form and plain text.

use std::fmt::Write as _;

/// Decodes Artisan's text escapes (`\\`, `\n`, `\r`, `\t`, `\xXX`, `\uXXXX`,
/// `\UXXXXXXXX`, `\'`, `\"`). Unknown or malformed escapes are kept as they
/// are.
#[must_use]
pub fn decode_text(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let (decoded, used) = decode_escape(rest);
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..used]),
        }
        rest = &rest[used..];
    }
    out.push_str(rest);
    out
}

/// Decodes one escape at the start of `s` (which starts with a backslash).
/// Returns the character, if the escape is valid, and the bytes consumed.
fn decode_escape(s: &str) -> (Option<char>, usize) {
    let mut chars = s[1..].chars();
    let Some(c) = chars.next() else {
        return (None, 1);
    };
    let simple = match c {
        '\\' => Some('\\'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        '\'' => Some('\''),
        '"' => Some('"'),
        _ => None,
    };
    if let Some(decoded) = simple {
        return (Some(decoded), 2);
    }
    let digits = match c {
        'x' => 2,
        'u' => 4,
        'U' => 8,
        _ => return (None, 1 + c.len_utf8()),
    };
    let hex = s.get(2..2 + digits);
    match hex
        .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .and_then(char::from_u32)
    {
        Some(decoded) => (Some(decoded), 2 + digits),
        None => (None, 2),
    }
}

/// Encodes text with Artisan's text escapes, the inverse of [`decode_text`]:
/// backslashes and control characters are escaped and every non-ASCII
/// character becomes `\xXX`, `\uXXXX` or `\UXXXXXXXX`.
#[must_use]
pub fn encode_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ' '..='~' => out.push(c),
            c => {
                let code = u32::from(c);
                // Writing into a String cannot fail.
                let _ = if code <= 0xff {
                    write!(out, "\\x{code:02x}")
                } else if code <= 0xffff {
                    write!(out, "\\u{code:04x}")
                } else {
                    write!(out, "\\U{code:08x}")
                };
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_what_artisan_writes() {
        assert_eq!(decode_text("Catua\\xed"), "Catuaí");
        assert_eq!(decode_text("line 1\\nline 2"), "line 1\nline 2");
        assert_eq!(decode_text("\\u9178\\u8d28"), "酸质");
        assert_eq!(decode_text("\\U0001f600"), "😀");
        assert_eq!(decode_text("a\\\\b"), "a\\b");
        assert_eq!(decode_text("plain"), "plain");
    }

    #[test]
    fn keeps_malformed_escapes() {
        assert_eq!(decode_text("\\q"), "\\q");
        assert_eq!(decode_text("\\x4"), "\\x4");
        assert_eq!(decode_text("\\uZZZZ"), "\\uZZZZ");
        assert_eq!(decode_text("end\\"), "end\\");
        assert_eq!(decode_text("\\ud800"), "\\ud800");
    }

    #[test]
    fn encode_is_the_inverse() {
        for s in ["Catuaí", "a\nb\tc\\d", "酸质 😀", "it's \"x\"", "\u{1}"] {
            let encoded = encode_text(s);
            assert!(encoded.is_ascii(), "{encoded}");
            assert_eq!(decode_text(&encoded), s);
        }
        assert_eq!(encode_text("Catuaí\n"), "Catua\\xed\\n");
    }
}
