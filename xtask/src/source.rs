//! Just enough Rust lexing to avoid false positives.
//!
//! The cfg-leak check needs to find `#[cfg(target_os = "…")]` in source files.
//! Naively grepping for `target_os` flags every doc comment that *mentions* the
//! rule — including the ones in this repository that explain it. A check that
//! fires on its own documentation gets switched off within the week.
//!
//! So: blank out comments and string literals first, keeping byte offsets
//! intact so line numbers still point at the right place.

/// Replace every comment and string literal in `src` with spaces, preserving
/// length, line breaks and therefore byte offsets.
///
/// Handles line comments (including `///` and `//!`), nested block comments,
/// normal strings with escapes, and raw strings with any number of hashes.
/// Character literals are recognised well enough not to be confused with
/// lifetime ticks.
#[must_use]
pub fn blank_comments_and_strings(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out: Vec<u8> = bytes.to_vec();
    let mut i = 0usize;

    // Blank a range, but keep newlines so line numbering survives.
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for byte in &mut out[from..to] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    };

    while i < bytes.len() {
        match bytes[i] {
            // Line comment
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                blank(&mut out, start, i);
            }
            // Block comment, which Rust allows to nest.
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let start = i;
                let mut depth = 1usize;
                i += 2;
                while i < bytes.len() && depth > 0 {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                blank(&mut out, start, i);
            }
            // Raw string: r"…", r#"…"#, r##"…"##, and the b/c-prefixed forms.
            b'r' | b'b' | b'c' if raw_string_hashes(bytes, i).is_some() => {
                let (quote_at, hashes) = raw_string_hashes(bytes, i).expect("checked above");
                let start = i;
                i = quote_at + 1;
                loop {
                    if i >= bytes.len() {
                        break;
                    }
                    if bytes[i] == b'"' {
                        let closing = i + 1;
                        let enough = bytes[closing..]
                            .iter()
                            .take(hashes)
                            .filter(|byte| **byte == b'#')
                            .count()
                            == hashes;
                        if enough {
                            i = closing + hashes;
                            break;
                        }
                    }
                    i += 1;
                }
                blank(&mut out, start, i);
            }
            // Ordinary string literal.
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                blank(&mut out, start, i.min(bytes.len()));
            }
            // Character literal, but not a lifetime. `'a'` is a literal;
            // `'a` in `&'a str` is not, and blanking it would corrupt the file.
            b'\'' if char_literal_len(bytes, i).is_some() => {
                let len = char_literal_len(bytes, i).expect("checked above");
                blank(&mut out, i, i + len);
                i += len;
            }
            _ => i += 1,
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| src.to_owned())
}

/// If a raw-string prefix starts at `i`, return `(index of the opening quote,
/// hash count)`.
fn raw_string_hashes(bytes: &[u8], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    // Optional byte/C-string prefix before the `r`.
    if matches!(bytes.get(j), Some(b'b' | b'c')) {
        j += 1;
    }
    if bytes.get(j) != Some(&b'r') {
        return None;
    }
    // A bare `r` must not be part of a longer identifier (`str`, `for`).
    if i > 0 && is_ident_byte(bytes[i - 1]) {
        return None;
    }
    j += 1;
    let hash_start = j;
    while bytes.get(j) == Some(&b'#') {
        j += 1;
    }
    if bytes.get(j) == Some(&b'"') {
        Some((j, j - hash_start))
    } else {
        None
    }
}

/// Length of the character literal starting at `i`, if this really is one.
fn char_literal_len(bytes: &[u8], i: usize) -> Option<usize> {
    debug_assert_eq!(bytes[i], b'\'');
    // `'\n'`, `'\''`, `'\\'` — escape plus closing quote.
    if bytes.get(i + 1) == Some(&b'\\') {
        let mut j = i + 2;
        while j < bytes.len() && bytes[j] != b'\'' {
            j += 1;
        }
        return (j < bytes.len()).then_some(j - i + 1);
    }
    // `'x'` — one character then a closing quote. Anything else (`'a` followed
    // by a non-quote) is a lifetime.
    let next = char_end(bytes, i + 1)?;
    (bytes.get(next) == Some(&b'\'')).then_some(next - i + 1)
}

/// Offset just past the UTF-8 character starting at `from`.
fn char_end(bytes: &[u8], from: usize) -> Option<usize> {
    let width = match bytes.get(from)? {
        byte if *byte < 0x80 => 1,
        byte if byte >> 5 == 0b110 => 2,
        byte if byte >> 4 == 0b1110 => 3,
        _ => 4,
    };
    Some(from + width)
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// 1-indexed line number for a byte offset.
#[must_use]
pub fn line_of(src: &str, offset: usize) -> usize {
    src.as_bytes()[..offset.min(src.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_comments_mentioning_the_rule_are_blanked() {
        let src = "//! No `#[cfg(target_os = \"macos\")]` here.\nfn a() {}\n";
        let blanked = blank_comments_and_strings(src);
        assert!(!blanked.contains("target_os"));
        assert!(blanked.contains("fn a() {}"));
    }

    #[test]
    fn real_code_survives() {
        let src = "#[cfg(target_os = \"windows\")]\nfn a() {}\n";
        let blanked = blank_comments_and_strings(src);
        assert!(blanked.contains("cfg(target_os ="));
        // The string literal's contents go, the structure stays.
        assert!(!blanked.contains("windows"));
    }

    #[test]
    fn offsets_and_line_numbers_are_preserved() {
        let src = "// target_os\nlet x = 1;\n#[cfg(target_os = \"macos\")]\n";
        let blanked = blank_comments_and_strings(src);

        assert_eq!(blanked.len(), src.len());
        let offset = blanked.find("target_os").unwrap();
        assert_eq!(line_of(&blanked, offset), 3);
    }

    #[test]
    fn nested_block_comments_close_correctly() {
        let src = "/* outer /* inner target_os */ still comment */ fn a() {}";
        let blanked = blank_comments_and_strings(src);
        assert!(!blanked.contains("target_os"));
        assert!(blanked.contains("fn a() {}"));
    }

    #[test]
    fn raw_strings_do_not_swallow_the_rest_of_the_file() {
        let src = "let j = r#\"{\"a\": \"//\"}\"#;\n#[cfg(target_os = \"macos\")]\n";
        let blanked = blank_comments_and_strings(src);
        assert!(blanked.contains("cfg(target_os ="), "got: {blanked}");
    }

    #[test]
    fn lifetimes_are_not_mistaken_for_character_literals() {
        let src = "fn f<'a>(s: &'a str) -> &'a str { s }\n#[cfg(target_os = \"macos\")]\n";
        let blanked = blank_comments_and_strings(src);
        assert!(blanked.contains("fn f<'a>(s: &'a str)"), "got: {blanked}");
        assert!(blanked.contains("cfg(target_os ="));
    }

    #[test]
    fn a_quote_inside_a_char_literal_does_not_start_a_string() {
        let src = "let q = '\"';\n#[cfg(target_os = \"macos\")]\n";
        let blanked = blank_comments_and_strings(src);
        assert!(blanked.contains("cfg(target_os ="), "got: {blanked}");
    }

    #[test]
    fn a_url_in_a_string_is_not_a_line_comment() {
        let src = "let u = \"https://example.com\";\nfn a() {}\n";
        let blanked = blank_comments_and_strings(src);
        assert!(blanked.contains("fn a() {}"));
    }
}
