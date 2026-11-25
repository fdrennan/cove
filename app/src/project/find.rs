//! Find, as RStudio has it (operator, 2026-10-20, a referral from
//! biomarker.ndexr.io: "Find in file (Ctrl/Cmd+F) ... Find in all files
//! (Ctrl/Cmd+Shift+F)"): one matcher for the editor's find bar (ide.rs) and
//! the host's search of a domain's files (host.rs `grep`) - plain text or a
//! regular expression, with or without case.

/// What is looked for.
pub struct Matcher {
    re: regex_lite::Regex,
}

impl Matcher {
    /// `query` as typed: a regular expression when `regex`, else the text
    /// itself; `case` matches case exactly.
    pub fn new(query: &str, case: bool, regex: bool) -> Result<Matcher, String> {
        if query.is_empty() {
            return Err("nothing to find".into());
        }
        let pat = if regex { query.to_owned() } else { regex_lite::escape(query) };
        // ^ and $ at each line, as a find in an editor means them.
        let pat = if case { format!("(?m){pat}") } else { format!("(?mi){pat}") };
        regex_lite::Regex::new(&pat).map(|re| Matcher { re }).map_err(|e| format!("not a regular expression: {e}"))
    }

    /// Each match in `text` as byte ranges, at most `limit` (empty matches,
    /// which a pattern like `x*` makes everywhere, are left out).
    pub fn find_all(&self, text: &str, limit: usize) -> Vec<(usize, usize)> {
        self.re.find_iter(text).filter(|m| m.end() > m.start()).take(limit).map(|m| (m.start(), m.end())).collect()
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.re.find_iter(text).any(|m| m.end() > m.start())
    }
}

#[cfg(test)]
mod tests {
    use super::Matcher;

    #[test]
    fn plain_text_is_literal_and_case_is_a_choice() {
        let m = Matcher::new("a.b", false, false).unwrap();
        assert_eq!(m.find_all("A.B axb a.b", 10), vec![(0, 3), (8, 11)]);
        let m = Matcher::new("a.b", true, false).unwrap();
        assert_eq!(m.find_all("A.B a.b", 10), vec![(4, 7)]);
        let m = Matcher::new("a.b", true, true).unwrap();
        assert_eq!(m.find_all("axb a.b", 10).len(), 2);
        assert!(Matcher::new("(", false, true).is_err());
        assert!(Matcher::new("x*", false, true).unwrap().find_all("abc", 10).is_empty());
    }
}
