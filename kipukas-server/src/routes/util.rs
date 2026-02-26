//! Shared URL/form parsing utilities for route handlers.

/// Parse URL-encoded form body into key-value pairs.
/// Handles `key=value&key2=value2` format (from HTMX POST bodies).
pub fn parse_form_body(body: &str) -> Vec<(String, String)> {
    if body.is_empty() {
        return Vec::new();
    }
    body.split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?;
            let val = parts.next().unwrap_or("");
            Some((percent_decode(key), percent_decode(val)))
        })
        .collect()
}

/// Percent-decode a URL-encoded value.
pub fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let hex = [hi, lo];
            if let Ok(s) = core::str::from_utf8(&hex) {
                if let Ok(val) = u8::from_str_radix(s, 16) {
                    result.push(val as char);
                    continue;
                }
            }
            result.push('%');
            result.push(hi as char);
            result.push(lo as char);
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

/// Parse a query string into key-value pairs.
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    let q = query.strip_prefix('?').unwrap_or(query);
    parse_form_body(q)
}

/// Helper to get a value by key from a list of key-value pairs.
pub fn get_param<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_form_body_works() {
        let pairs = parse_form_body("card=brox&slot=2&action=toggle");
        assert_eq!(pairs.len(), 3);
        assert_eq!(get_param(&pairs, "card"), Some("brox"));
        assert_eq!(get_param(&pairs, "slot"), Some("2"));
    }

    #[test]
    fn parse_form_body_empty() {
        let pairs = parse_form_body("");
        assert!(pairs.is_empty());
    }

    #[test]
    fn percent_decode_plus_as_space() {
        assert_eq!(percent_decode("hello+world"), "hello world");
    }

    #[test]
    fn percent_decode_hex() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
    }

    #[test]
    fn parse_query_strips_prefix() {
        let pairs = parse_query("?foo=bar");
        assert_eq!(get_param(&pairs, "foo"), Some("bar"));
    }
}
