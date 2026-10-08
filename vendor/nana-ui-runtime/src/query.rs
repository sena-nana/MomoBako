//! Case-insensitive substring match for menu and palette queries.

pub(crate) fn query_matches(text: &str, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    text.to_lowercase().contains(&query.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::query_matches;

    #[test]
    fn empty_query_matches_everything() {
        assert!(query_matches("Rename", ""));
        assert!(query_matches("Rename", "   "));
    }

    #[test]
    fn match_is_case_insensitive() {
        assert!(query_matches("Rename File", "ren"));
        assert!(!query_matches("Rename File", "delete"));
    }
}
