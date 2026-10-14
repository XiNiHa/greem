//! Reading `multipart/mixed` incremental responses from any implementation:
//! the boundary comes from `Content-Type`, each part's JSON body follows its
//! blank line.

/// The `boundary` parameter of a `multipart/mixed` content type.
pub fn boundary(content_type: &str) -> Option<String> {
    let mut params = content_type.split(';').map(str::trim);
    if !params
        .next()
        .is_some_and(|ty| ty.eq_ignore_ascii_case("multipart/mixed"))
    {
        return None;
    }
    params.find_map(|param| {
        let (name, value) = param.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

/// The bodies of every part whose following delimiter has already arrived,
/// so a partially received response yields only its complete payloads. A
/// delimiter counts only at the start of a line, which a JSON body never
/// contains.
pub fn complete_parts<'a>(boundary: &str, body: &'a [u8]) -> Vec<&'a [u8]> {
    let delimiter = format!("--{boundary}").into_bytes();
    let mut positions = Vec::new();
    let mut from = 0;
    while let Some(offset) = find(&body[from..], &delimiter) {
        let at = from + offset;
        if at == 0 || body[..at].ends_with(b"\r\n") {
            positions.push(at);
        }
        from = at + delimiter.len();
    }
    let mut parts = Vec::new();
    for window in positions.windows(2) {
        let after = window[0] + delimiter.len();
        if body[after..].starts_with(b"--") {
            break;
        }
        let segment = &body[after..window[1]];
        let Some(blank) = find(segment, b"\r\n\r\n") else {
            continue;
        };
        let content = &segment[blank + 4..];
        parts.push(content.strip_suffix(b"\r\n").unwrap_or(content));
    }
    parts
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boundary_is_read_from_the_content_type() {
        assert_eq!(
            boundary("multipart/mixed; boundary=\"-\"; incrementalSpec=v0.2"),
            Some("-".to_owned())
        );
        assert_eq!(
            boundary("multipart/mixed;boundary=graphql"),
            Some("graphql".to_owned())
        );
        assert_eq!(boundary("application/json"), None);
    }

    #[test]
    fn greem_framing_yields_one_body_per_part() {
        let body = b"\r\n---\r\nContent-Type: application/json; charset=utf-8\r\n\r\n{\"hasNext\":true}\r\n---\r\nContent-Type: application/json; charset=utf-8\r\n\r\n{\"hasNext\":false}\r\n-----\r\n";
        let parts = complete_parts("-", body);
        assert_eq!(
            parts,
            vec![&b"{\"hasNext\":true}"[..], &b"{\"hasNext\":false}"[..]]
        );
    }

    #[test]
    fn yoga_framing_without_the_leading_crlf_yields_the_same() {
        let body = b"--graphql\r\nContent-Type: application/json\r\n\r\n{\"a\":1}\r\n--graphql\r\nContent-Type: application/json\r\n\r\n{\"a\":2}\r\n--graphql--\r\n";
        let parts = complete_parts("graphql", body);
        assert_eq!(parts, vec![&b"{\"a\":1}"[..], &b"{\"a\":2}"[..]]);
    }

    #[test]
    fn only_parts_whose_closing_delimiter_arrived_count() {
        let body = b"\r\n---\r\nContent-Type: application/json\r\n\r\n{\"hasNext\":true}\r\n---\r\nContent-Type: application/json\r\n\r\n{\"has";
        assert_eq!(complete_parts("-", body).len(), 1);
    }
}
