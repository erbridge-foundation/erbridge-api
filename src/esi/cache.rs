/// Parses `max-age=N` from a `Cache-Control` header value.
pub fn parse_max_age(header: &str) -> Option<u64> {
    header.split(',').find_map(|part| {
        let part = part.trim();
        let rest = part.strip_prefix("max-age")?;
        let rest = rest.trim().strip_prefix('=')?;
        rest.trim().parse::<u64>().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_max_age_standard() {
        assert_eq!(parse_max_age("public, max-age=60"), Some(60));
    }

    #[test]
    fn parse_max_age_only() {
        assert_eq!(parse_max_age("max-age=30"), Some(30));
    }

    #[test]
    fn parse_max_age_missing() {
        assert_eq!(parse_max_age("no-cache"), None);
    }

    #[test]
    fn parse_max_age_with_spaces() {
        assert_eq!(parse_max_age("public, max-age = 120"), Some(120));
    }

    #[test]
    fn parse_max_age_zero() {
        assert_eq!(parse_max_age("max-age=0"), Some(0));
    }
}
