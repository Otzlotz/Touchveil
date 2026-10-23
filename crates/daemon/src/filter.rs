//! Name filter for devices: substring or wildcard (`*`, `?`).
//! Comparisons ignore case.
//!
//! Without wildcard: substring match ("touchscreen").
//! With wildcard:    full match ("wch*touchscreen", "*touch*").

pub enum Filter {
    Contains(String),
    Glob(Vec<char>),
}

impl Filter {
    pub fn parse(pat: &str) -> Result<Self, String> {
        let lower = pat.trim().to_lowercase();
        if lower.is_empty() {
            return Err("must not be empty".into());
        }
        if lower.contains(['*', '?']) {
            Ok(Filter::Glob(lower.chars().collect()))
        } else {
            Ok(Filter::Contains(lower))
        }
    }

    pub fn matches(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        match self {
            Filter::Contains(s) => name.contains(s.as_str()),
            Filter::Glob(p) => glob(p, &name.chars().collect::<Vec<_>>()),
        }
    }
}

/// `*` = any number of characters (including none), `?` = exactly one character.
fn glob(p: &[char], t: &[char]) -> bool {
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pat: &str, name: &str) -> bool {
        Filter::parse(pat).unwrap().matches(name)
    }

    #[test]
    fn contains() {
        assert!(m("touchscreen", "wch.cn Touchscreen"));
        assert!(!m("mouse", "wch.cn touchscreen"));
    }

    #[test]
    fn wildcard() {
        assert!(m("wch*touchscreen", "wch.cn touchscreen"));
        assert!(m("*touch*", "wch.cn touchscreen"));
        assert!(!m("touch*", "wch.cn touchscreen")); // full match
        assert!(m("wch.cn ?ouchscreen", "wch.cn touchscreen"));
    }

    #[test]
    fn empty_is_error() {
        assert!(Filter::parse("  ").is_err());
    }
}
