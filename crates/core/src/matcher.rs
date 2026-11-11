//! Hotstring trigger matcher (spec §3.1). Pure state machine over normalized key events.
//! Keeps only a short rolling window of characters in RAM; never persisted.

use std::collections::{HashMap, VecDeque};

const WINDOW: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delim {
    Space,
    Tab,
    Enter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyInput {
    Char(char),
    Delimiter(Delim),
    Backspace,
    /// Focus change, cursor movement, Ctrl/Alt/Win chords, modal open, IME composition...
    Reset,
}

#[derive(Clone, Debug)]
pub struct MatcherConfig {
    pub delimiters: Vec<Delim>,
    pub case_sensitive: bool,
    pub literal_mode: bool,
    pub require_word_boundary: bool,
}

impl Default for MatcherConfig {
    fn default() -> Self {
        Self {
            delimiters: vec![Delim::Space],
            case_sensitive: false,
            literal_mode: false,
            require_word_boundary: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// Code as registered (original casing).
    pub code: String,
    /// Number of characters the user typed that form the trigger (to be removed later).
    pub trigger_len: usize,
    /// The delimiter that fired the match (`None` for literal `::code::` mode).
    pub delimiter: Option<Delim>,
    pub literal: bool,
}

pub struct TriggerMatcher {
    cfg: MatcherConfig,
    codes: HashMap<String, String>,
    buf: VecDeque<char>,
    truncated: bool,
}

fn is_code_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

impl TriggerMatcher {
    pub fn new(cfg: MatcherConfig) -> Self {
        Self {
            cfg,
            codes: HashMap::new(),
            buf: VecDeque::new(),
            truncated: false,
        }
    }

    pub fn set_config(&mut self, cfg: MatcherConfig) {
        self.cfg = cfg;
        self.reset();
    }

    fn norm(&self, s: &str) -> String {
        if self.cfg.case_sensitive {
            s.to_string()
        } else {
            s.to_lowercase()
        }
    }

    /// Replace the active set of codes (enabled snippets only).
    pub fn set_codes<'a>(&mut self, codes: impl IntoIterator<Item = &'a str>) {
        self.codes = codes
            .into_iter()
            .map(|c| (self.norm(c), c.to_string()))
            .collect();
        self.reset();
    }

    pub fn reset(&mut self) {
        self.buf.clear();
        self.truncated = false;
    }

    fn push(&mut self, c: char) {
        if self.buf.len() == WINDOW {
            self.buf.pop_front();
            self.truncated = true;
        }
        self.buf.push_back(c);
    }

    pub fn feed(&mut self, input: KeyInput) -> Option<Match> {
        match input {
            KeyInput::Reset => {
                self.reset();
                None
            }
            KeyInput::Backspace => {
                self.buf.pop_back();
                None
            }
            KeyInput::Char(c) => {
                self.push(c);
                if self.cfg.literal_mode
                    && c == ':'
                    && let Some(m) = self.match_literal()
                {
                    self.reset();
                    return Some(m);
                }
                None
            }
            // Enter is never a trigger delimiter (it would send the chat message) — spec §3.1.
            KeyInput::Delimiter(Delim::Enter) => {
                self.reset();
                None
            }
            KeyInput::Delimiter(d) => {
                if self.cfg.delimiters.contains(&d)
                    && let Some(m) = self.match_boundary(d)
                {
                    self.reset();
                    return Some(m);
                }
                self.push(if d == Delim::Tab { '\t' } else { ' ' });
                None
            }
        }
    }

    fn match_boundary(&self, d: Delim) -> Option<Match> {
        let chars: Vec<char> = self.buf.iter().copied().collect();
        let end = chars.len();
        let mut start = end;
        while start > 0 && is_code_char(chars[start - 1]) {
            start -= 1;
        }
        if start == end {
            return None;
        }
        let try_len = |from: usize| -> Option<Match> {
            let word: String = chars[from..end].iter().collect();
            let code = self.codes.get(&self.norm(&word))?;
            Some(Match {
                code: code.clone(),
                trigger_len: end - from,
                delimiter: Some(d),
                literal: false,
            })
        };
        let boundary_ok = if start == 0 {
            !self.truncated
        } else {
            !is_word_char(chars[start - 1])
        };
        if boundary_ok && let Some(m) = try_len(start) {
            return Some(m);
        }
        if !self.cfg.require_word_boundary {
            for from in start..end {
                if let Some(m) = try_len(from) {
                    return Some(m);
                }
            }
        }
        None
    }

    fn match_literal(&self) -> Option<Match> {
        let chars: Vec<char> = self.buf.iter().copied().collect();
        let n = chars.len();
        if n < 6 || chars[n - 2] != ':' || chars[n - 1] != ':' {
            return None;
        }
        let mut s = n - 2;
        while s > 0 && is_code_char(chars[s - 1]) {
            s -= 1;
        }
        if s < 2 || chars[s - 1] != ':' || chars[s - 2] != ':' || s == n - 2 {
            return None;
        }
        let word: String = chars[s..n - 2].iter().collect();
        let code = self.codes.get(&self.norm(&word))?;
        Some(Match {
            code: code.clone(),
            trigger_len: word.chars().count() + 4,
            delimiter: None,
            literal: true,
        })
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(m: &mut TriggerMatcher, s: &str) -> Option<Match> {
        let mut last = None;
        for c in s.chars() {
            let k = match c {
                ' ' => KeyInput::Delimiter(Delim::Space),
                '\t' => KeyInput::Delimiter(Delim::Tab),
                '\n' => KeyInput::Delimiter(Delim::Enter),
                c => KeyInput::Char(c),
            };
            if let Some(x) = m.feed(k) {
                last = Some(x);
            }
        }
        last
    }

    fn matcher() -> TriggerMatcher {
        let mut m = TriggerMatcher::new(MatcherConfig::default());
        m.set_codes(["ztn", "z", "zbackup"]);
        m
    }

    #[test]
    fn code_plus_space_matches() {
        let r = type_str(&mut matcher(), "ztn ").unwrap();
        assert_eq!(
            (r.code.as_str(), r.trigger_len, r.delimiter),
            ("ztn", 3, Some(Delim::Space))
        );
    }

    #[test]
    fn case_insensitive_by_default() {
        assert_eq!(type_str(&mut matcher(), "ZTN ").unwrap().code, "ztn");
    }

    #[test]
    fn case_sensitive_when_configured() {
        let mut m = TriggerMatcher::new(MatcherConfig {
            case_sensitive: true,
            ..Default::default()
        });
        m.set_codes(["ztn"]);
        assert!(type_str(&mut m, "ZTN ").is_none());
        assert!(type_str(&mut m, "ztn ").is_some());
    }

    #[test]
    fn short_code_does_not_fire_inside_longer_word() {
        let mut m = matcher();
        assert!(type_str(&mut m, "zt ").is_none());
        assert_eq!(type_str(&mut m, "z ").unwrap().code, "z");
        assert!(type_str(&mut m, "abc_ztn ").is_none());
        assert!(type_str(&mut m, "xztn ").is_none());
        assert!(type_str(&mut m, "êztn ").is_none());
    }

    #[test]
    fn boundary_after_space_or_punct() {
        let mut m = matcher();
        assert!(type_str(&mut m, "chào ztn ").is_some());
        assert!(type_str(&mut m, "chào,ztn ").is_some());
    }

    #[test]
    fn enter_never_triggers_and_resets() {
        let mut m = matcher();
        assert!(type_str(&mut m, "ztn\n").is_none());
        assert!(type_str(&mut m, " ").is_none());
        let mut m2 = TriggerMatcher::new(MatcherConfig {
            delimiters: vec![Delim::Space, Delim::Enter],
            ..Default::default()
        });
        m2.set_codes(["ztn"]);
        assert!(type_str(&mut m2, "ztn\n").is_none());
    }

    #[test]
    fn tab_only_when_configured() {
        let mut m = matcher();
        assert!(type_str(&mut m, "ztn\t").is_none());
        let mut m2 = TriggerMatcher::new(MatcherConfig {
            delimiters: vec![Delim::Space, Delim::Tab],
            ..Default::default()
        });
        m2.set_codes(["ztn"]);
        assert!(type_str(&mut m2, "ztn\t").is_some());
    }

    #[test]
    fn backspace_and_reset_edit_window() {
        let mut m = matcher();
        type_str(&mut m, "ztnx");
        m.feed(KeyInput::Backspace);
        assert!(type_str(&mut m, " ").is_some());
        type_str(&mut m, "zt");
        m.feed(KeyInput::Reset);
        assert!(type_str(&mut m, "n ").is_none());
    }

    #[test]
    fn literal_mode() {
        let mut m = TriggerMatcher::new(MatcherConfig {
            literal_mode: true,
            ..Default::default()
        });
        m.set_codes(["ztn"]);
        let r = type_str(&mut m, "::ztn::").unwrap();
        assert_eq!((r.trigger_len, r.literal, r.delimiter), (7, true, None));
        assert!(type_str(&mut m, "::nope::").is_none());
    }

    #[test]
    fn window_truncation_is_not_start_of_text() {
        let mut m = matcher();
        let pad: String = "a".repeat(100);
        type_str(&mut m, &pad);
        // buffer is full of 'a'; the run is longer than any code -> no match
        assert!(type_str(&mut m, " ").is_none());
    }

    #[test]
    fn consecutive_triggers() {
        let mut m = matcher();
        assert!(type_str(&mut m, "ztn ").is_some());
        assert!(type_str(&mut m, "zbackup ").is_some());
    }
}
