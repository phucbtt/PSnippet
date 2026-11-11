use std::collections::HashSet;

use zs_core::{Snippet, Template};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("E_PARSER_INVALID {line}:{col}: {message}")]
pub struct ParseError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

fn perr(line: usize, col: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        col,
        message: message.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySnippet {
    pub code: String,
    /// Verbatim body, LF-normalized, `\)` unescaped. No per-line trimming.
    pub body: String,
    /// 1-based line of the `::code::` header.
    pub line: usize,
}

impl LegacySnippet {
    pub fn into_snippet(self) -> Snippet {
        Snippet::new(self.code.clone(), self.code, self.body)
    }
}

fn header_code(line: &str) -> Option<&str> {
    let t = line.trim();
    let inner = t.strip_prefix("::")?.strip_suffix("::")?;
    zs_core::is_valid_code(inner).then_some(inner)
}

pub fn parse_legacy(input: &str) -> Result<Vec<LegacySnippet>, ParseError> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let lines: Vec<&str> = input.lines().collect();
    let mut out: Vec<LegacySnippet> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut i = 0;

    while i < lines.len() {
        let raw = lines[i];
        let t = raw.trim();
        if t.is_empty() || t.starts_with(';') {
            i += 1;
            continue;
        }
        let header_line = i + 1;
        let Some(code) = header_code(raw) else {
            return Err(perr(
                header_line,
                1,
                "expected header like ::code:: (letters/digits/_/-, 2-32 chars)",
            ));
        };
        if !seen.insert(code.to_lowercase()) {
            return Err(perr(header_line, 1, format!("duplicate code '{code}'")));
        }
        i += 1;
        match lines.get(i) {
            Some(l) if l.trim() == "(" => i += 1,
            _ => return Err(perr(i + 1, 1, "expected '(' on the line after the header")),
        }
        let mut body_lines: Vec<String> = Vec::new();
        loop {
            let Some(l) = lines.get(i) else {
                return Err(perr(header_line, 1, "missing closing ')' for block"));
            };
            if l.trim() == ")" {
                i += 1;
                break;
            }
            // `\)` at line start => literal ')' line content.
            let content = match l.strip_prefix("\\)") {
                Some(rest) => format!("){rest}"),
                None => (*l).to_string(),
            };
            body_lines.push(content);
            i += 1;
        }
        match lines.get(i) {
            Some(l) if l.trim().eq_ignore_ascii_case("return") => i += 1,
            _ => return Err(perr(i + 1, 1, "missing 'return' after ')'")),
        }
        let body = body_lines.join("\n");
        if body.trim().is_empty() {
            return Err(perr(header_line, 1, "empty block"));
        }
        if let Err(e) = Template::parse(&body) {
            return Err(perr(
                header_line + 1 + e.line,
                e.col,
                format!("in body of '{code}': {}", e.kind),
            ));
        }
        out.push(LegacySnippet {
            code: code.to_string(),
            body,
            line: header_line,
        });
    }
    Ok(out)
}

/// Legacy export keeps trigger + body only; field metadata does not round-trip (UI must warn).
pub fn export_legacy<'a>(snippets: impl IntoIterator<Item = &'a Snippet>) -> String {
    let mut s = String::new();
    for sn in snippets {
        s.push_str(&format!("::{}::\n(\n", sn.code));
        for line in sn.body.split('\n') {
            if line.trim() == ")" {
                s.push_str("\\)"); // a lone ')' line would otherwise close the block
            } else {
                s.push_str(line);
            }
            s.push('\n');
        }
        s.push_str(")\nreturn\n\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_block_with_crlf_and_bom() {
        let src = "\u{feff}::ztn::\r\n(\r\nXin chào [Tên]\r\n\r\nHết\r\n)\r\nreturn\r\n";
        let r = parse_legacy(src).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].code, "ztn");
        assert_eq!(r[0].body, "Xin chào [Tên]\n\nHết");
    }

    #[test]
    fn body_is_verbatim_no_line_trim() {
        let r = parse_legacy("::ab::\n(\n  indented  \n)\nRETURN\n").unwrap();
        assert_eq!(r[0].body, "  indented  ");
    }

    #[test]
    fn escaped_close_paren_and_return_inside_block() {
        let r = parse_legacy("::ab::\n(\nreturn\n\\)\n\\) tiếp\n)\nreturn\n").unwrap();
        assert_eq!(r[0].body, "return\n)\n) tiếp");
    }

    #[test]
    fn multiple_blocks_comments_and_blank_lines() {
        let r =
            parse_legacy("; ghi chú\n::aa::\n(\nx\n)\nreturn\n\n::bb::\n(\ny\n)\nreturn").unwrap();
        assert_eq!(
            r.iter().map(|s| s.code.as_str()).collect::<Vec<_>>(),
            ["aa", "bb"]
        );
    }

    #[test]
    fn errors_with_positions() {
        assert_eq!(
            parse_legacy("::aa::\n(\nx\n)\n").unwrap_err().message,
            "missing 'return' after ')'"
        );
        assert!(
            parse_legacy("::aa::\n(\nx\n")
                .unwrap_err()
                .message
                .contains("missing closing")
        );
        assert!(
            parse_legacy("::a::\n(\nx\n)\nreturn")
                .unwrap_err()
                .message
                .contains("expected header")
        );
        assert!(
            parse_legacy("hello")
                .unwrap_err()
                .message
                .contains("expected header")
        );
        let e = parse_legacy("::aa::\n(\nx\n)\nreturn\n::AA::\n(\ny\n)\nreturn").unwrap_err();
        assert_eq!((e.line, e.message.as_str()), (6, "duplicate code 'AA'"));
        assert!(
            parse_legacy("::aa::\n(\n\n)\nreturn")
                .unwrap_err()
                .message
                .contains("empty")
        );
        let e = parse_legacy("::aa::\n(\nok\nbad [Tên\n)\nreturn").unwrap_err();
        assert!(e.message.contains("placeholder not closed"));
        assert_eq!(e.line, 4);
    }

    #[test]
    fn never_executes_ahk_commands() {
        let r = parse_legacy("::aa::\n(\nRun, calc.exe\nSend {Enter}\n)\nreturn").unwrap();
        assert_eq!(r[0].body, "Run, calc.exe\nSend {Enter}");
    }

    #[test]
    fn export_round_trips() {
        let s = Snippet::new("aa", "t", "dòng 1\n)\n\ndòng [X]");
        let text = export_legacy([&s]);
        let back = parse_legacy(&text).unwrap();
        assert_eq!(back[0].body, s.body);
    }
}
