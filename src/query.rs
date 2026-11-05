/// Quotes group a phrase; an unfinished quote extends through end of input.
/// Everything else, including FTS operators, is literal text.
pub(crate) fn terms(input: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in input.chars() {
        if ch == '"' || (!quoted && ch.is_whitespace()) {
            let term = current.trim();
            if !term.is_empty() {
                terms.push(term.to_lowercase());
            }
            current.clear();
            if ch == '"' {
                quoted = !quoted;
            }
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        terms.push(current.trim().to_lowercase());
    }
    terms
}

pub(crate) fn fts(terms: &[String]) -> String {
    terms
        .iter()
        .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}
