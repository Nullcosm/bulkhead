//! Minimal wildcard matching for tool patterns (`*` = any run, `?` = one
//! char). Deliberately dependency-free; the policy format only needs `*`/`?`.

/// True when `pattern` matches `text` with `*` (any sequence) and `?`
/// (exactly one character) wildcards.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // dp[i][j] = pattern[..i] matches text[..j]
    let mut dp = vec![vec![false; t.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        } else {
            break;
        }
    }
    for i in 1..=p.len() {
        for j in 1..=t.len() {
            dp[i][j] = match p[i - 1] {
                '*' => dp[i - 1][j] || dp[i][j - 1],
                '?' => dp[i - 1][j - 1],
                c => c == t[j - 1] && dp[i - 1][j - 1],
            };
        }
    }
    dp[p.len()][t.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_wildcards() {
        assert!(wildcard_match("http.get", "http.get"));
        assert!(!wildcard_match("http.get", "http.post"));
        assert!(wildcard_match("http.*", "http.get"));
        assert!(wildcard_match("http.*", "http.post.delete"));
        assert!(wildcard_match("*", "anything.at.all"));
        assert!(wildcard_match("http.??t", "http.get"));
        assert!(!wildcard_match("http.??t", "http.post"));
        assert!(wildcard_match("*.query", "db.query"));
        assert!(!wildcard_match("db.*", "http.get"));
    }

    #[test]
    fn star_edges() {
        assert!(wildcard_match("", ""));
        assert!(!wildcard_match("", "x"));
        assert!(wildcard_match("x*", "x"));
        assert!(wildcard_match("*x", "x"));
        assert!(wildcard_match("a*b*c", "axxbyyc"));
    }
}
