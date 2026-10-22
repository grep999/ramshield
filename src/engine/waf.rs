//! Bounded deterministic HTTP/1 security inspection.
//!
//! This is intentionally a *security gate*, not a claim of full OWASP CRS
//! coverage. It enforces strict HTTP framing and bounded parsing first, then
//! applies cheap deterministic signatures. HTTPS/HTTP2/HTTP3 remain terminator
//! responsibilities unless a producer supplies decrypted HTTP telemetry.
//!
//! # The framing rules this gate enforces
//!
//! The parser is a trust boundary: it decides where one request ends. A
//! downstream parser that disagrees about that boundary is a request
//! smuggling primitive, so this gate accepts only the one spelling of each
//! construct and rejects everything else. The rules, in one place:
//!
//! - **Request line** (RFC 9112 3): `method SP request-target SP
//!   HTTP-version`. Exactly one SP between the components, no tab, no
//!   empty component, and no control character in the target.
//! - **Transfer-Encoding** (RFC 9112 6.1, RFC 9110 5.6.1): the only
//!   accepted coding is `chunked`, exactly once across all Transfer-Encoding
//!   header fields, and it may not appear with Content-Length. A coding
//!   token carries no interior whitespace.
//! - **Chunked body** (RFC 9112 7.1): the last chunk is followed by an
//!   optional trailer section of well-formed field lines and a final empty
//!   line, and the request ends there. Bytes after that are a second
//!   request in the same buffer, not part of this one.
//!
//! The input is one request per buffer, which is what the Content-Length
//! path already assumes: a body that does not exactly fill the buffer is
//! rejected. The chunked path now assumes the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finding {
    SqlInjection,
    Xss,
    PathTraversal,
    CommandInjection,
    Ssrf,
    Oversized,
    Malformed,
    HeaderSmuggling,
}

#[derive(Debug)]
struct RequestParts<'a> {
    target: &'a [u8],
    headers: &'a [u8],
    body: Vec<u8>,
}

const MAX_HEADERS: usize = 64;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_TARGET_BYTES: usize = 8192;
const MAX_HEADER_VALUE_BYTES: usize = 16 * 1024;

fn parse_request(request: &[u8], max_bytes: usize) -> Result<RequestParts<'_>, Finding> {
    if request.len() > max_bytes {
        return Err(Finding::Oversized);
    }
    let sep = request
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(Finding::Malformed)?;
    if sep > MAX_HEADER_BYTES {
        return Err(Finding::Oversized);
    }
    let head = &request[..sep];
    let lines: Vec<&[u8]> = head.split(|&b| b == b'\n').collect();
    let raw_request_line = *lines.first().ok_or(Finding::Malformed)?;
    // The delimiter search ends before the final CRLF, so a headerless
    // request line (and the final header line) is not present with its CR.
    // The delimiter itself proves the request line is CRLF-terminated.
    let request_line = raw_request_line
        .strip_suffix(b"\r")
        .unwrap_or(raw_request_line);
    // Exactly one SP between the components. A tab is not a separator, and
    // an empty component is not a component: accepting either would let
    // this parser and a stricter one disagree about the request target.
    let mut parts = request_line.split(|&b| b == b' ');
    let method = parts.next().ok_or(Finding::Malformed)?;
    let target = parts.next().ok_or(Finding::Malformed)?;
    let version = parts.next().ok_or(Finding::Malformed)?;
    if parts.next().is_some()
        || method.is_empty()
        || method.len() > 16
        || target.is_empty()
        || target.len() > MAX_TARGET_BYTES
        || (version != b"HTTP/1.1" && version != b"HTTP/1.0")
    {
        return Err(Finding::Malformed);
    }
    // A request target is a URI reference: no control character, no tab.
    if !method.iter().all(|b| b.is_ascii_alphabetic()) || target.iter().any(|b| *b < 0x21) {
        return Err(Finding::Malformed);
    }

    let headers_start = request_line.len() + 2;
    let headers = if headers_start <= sep {
        &request[headers_start..sep]
    } else {
        &[]
    };
    let mut content_length: Option<usize> = None;
    let mut transfer_chunked = false;
    let mut header_count = 0usize;
    let header_lines = &lines[1..];
    for (index, raw) in header_lines.iter().enumerate() {
        let raw = *raw;
        let line = match raw.strip_suffix(b"\r") {
            Some(line) => line,
            // Only the final header lacks CR here: `head` stops at the CR
            // that begins the final CRLFCRLF delimiter.
            None if index + 1 == header_lines.len() => raw,
            None => return Err(Finding::Malformed),
        };
        if line.is_empty() {
            continue;
        }
        header_count += 1;
        if header_count > MAX_HEADERS {
            return Err(Finding::Oversized);
        }
        let colon = line
            .iter()
            .position(|b| *b == b':')
            .ok_or(Finding::Malformed)?;
        let name = &line[..colon];
        let value = line[colon + 1..]
            .strip_prefix(b" ")
            .unwrap_or(&line[colon + 1..]);
        if name.is_empty()
            || name.len() > 128
            || !name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-')
            || value.len() > MAX_HEADER_VALUE_BYTES
        {
            return Err(Finding::Malformed);
        }
        if value
            .iter()
            .any(|b| *b == 0 || *b == b'\r' || *b == b'\n' || (*b < 0x20 && *b != b'\t'))
        {
            return Err(Finding::Malformed);
        }
        if eq_ci(name, b"content-length") {
            let len = parse_decimal(value).ok_or(Finding::Malformed)?;
            if let Some(previous) = content_length {
                if previous != len {
                    return Err(Finding::HeaderSmuggling);
                }
            } else {
                content_length = Some(len);
            }
        }
        if eq_ci(name, b"transfer-encoding") {
            // Security gate: reject unsupported transfer codings rather than
            // attempting to normalize an intermediary's framing semantics.
            // Only `chunked` is accepted, and exactly once across all
            // Transfer-Encoding header fields.
            //
            // A coding is a token (RFC 9110 5.6.1). Whitespace around a
            // comma is legal; whitespace *inside* a token is not, so the
            // token is trimmed at its ends only and the interior is
            // compared verbatim. Stripping interior whitespace would make
            // `chu nked` equal to `chunked`, which is a framing decision
            // this gate does not get to make.
            if transfer_chunked {
                // A second Transfer-Encoding header field. Its value is
                // combined with the first, and `chunked` may appear only
                // once in the combined list.
                return Err(Finding::HeaderSmuggling);
            }
            let mut token_count = 0usize;
            for token in value.split(|b| *b == b',') {
                let token = trim_ows(token);
                if token.is_empty() {
                    return Err(Finding::Malformed);
                }
                token_count += 1;
                if !eq_ci(token, b"chunked") {
                    return Err(Finding::HeaderSmuggling);
                }
            }
            if token_count != 1 {
                return Err(Finding::HeaderSmuggling);
            }
            transfer_chunked = true;
        }
    }
    if transfer_chunked && content_length.is_some() {
        return Err(Finding::HeaderSmuggling);
    }
    let raw_body = &request[sep + 4..];
    let body = if transfer_chunked {
        let (body, consumed) = decode_chunked(raw_body, max_bytes)?;
        // The request ends where the chunked body ends. Bytes after the
        // terminator are a second request in the same buffer: a producer
        // that hands this gate two requests at once must not get a verdict
        // on the first that covers the second.
        if consumed != raw_body.len() {
            return Err(Finding::Malformed);
        }
        body
    } else {
        if let Some(len) = content_length
            && (len != raw_body.len() || len > max_bytes)
        {
            return Err(Finding::Malformed);
        }
        raw_body.to_vec()
    };
    Ok(RequestParts {
        target,
        headers,
        body,
    })
}

/// Decode a chunked body, returning the body and the number of bytes
/// consumed from `raw`.
///
/// The consumed count is what lets the caller see that the request ended
/// at the terminator: a chunked body is followed by an optional trailer
/// section and a final empty line, and nothing else.
fn decode_chunked(raw: &[u8], max_bytes: usize) -> Result<(Vec<u8>, usize), Finding> {
    let mut pos = 0usize;
    let mut out = Vec::with_capacity(raw.len().min(max_bytes));
    loop {
        let rel = raw[pos..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or(Finding::Malformed)?;
        let end = pos + rel;
        let line = &raw[pos..end];
        let size_text = line.split(|b| *b == b';').next().unwrap_or(line);
        let size = usize::from_str_radix(
            std::str::from_utf8(size_text)
                .map_err(|_| Finding::Malformed)?
                .trim(),
            16,
        )
        .map_err(|_| Finding::Malformed)?;
        pos = end + 2;
        if size == 0 {
            // The last chunk is followed by an optional trailer section
            // (RFC 9112 7.1.2: field lines, then a final empty line) and
            // then the end of the request.
            let trailer_end = parse_chunk_trailers(raw, pos)?;
            return Ok((out, trailer_end));
        }
        if size > max_bytes || pos.checked_add(size + 2).is_none() || pos + size + 2 > raw.len() {
            return Err(Finding::Malformed);
        }
        if out.len().saturating_add(size) > max_bytes {
            return Err(Finding::Oversized);
        }
        out.extend_from_slice(&raw[pos..pos + size]);
        pos += size;
        if raw.get(pos..pos + 2) != Some(b"\r\n") {
            return Err(Finding::Malformed);
        }
        pos += 2;
    }
}

/// Parse the trailer section that follows the last chunk.
///
/// RFC 9112 7.1.2: zero or more trailer field lines, then a final empty
/// line. Returns the offset just past the end of the request.
///
/// A trailer field is a header field: it has a name, a colon, and a
/// value. A section that is not terminated by an empty line is malformed
/// rather than "bounded enough", because a parser that stops at a
/// different place than this one is a smuggling primitive.
fn parse_chunk_trailers(raw: &[u8], start: usize) -> Result<usize, Finding> {
    let mut pos = start;
    let mut field_count = 0usize;
    loop {
        let rel = raw[pos..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or(Finding::Malformed)?;
        let end = pos + rel;
        let line = &raw[pos..end];
        pos = end + 2;
        if line.is_empty() {
            // The final empty line ends the trailer section.
            return Ok(pos);
        }
        field_count += 1;
        if field_count > MAX_HEADERS {
            return Err(Finding::Oversized);
        }
        let colon = line
            .iter()
            .position(|b| *b == b':')
            .ok_or(Finding::Malformed)?;
        let name = &line[..colon];
        if name.is_empty()
            || name.len() > 128
            || !name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        {
            return Err(Finding::Malformed);
        }
        if line.len() > MAX_HEADER_BYTES {
            return Err(Finding::Oversized);
        }
    }
}

fn parse_decimal(v: &[u8]) -> Option<usize> {
    if v.is_empty() {
        return None;
    }
    let mut n = 0usize;
    for b in v.iter().copied() {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((b - b'0') as usize)?;
    }
    Some(n)
}

/// Trim optional whitespace (SP and HTAB) at both ends of a token.
///
/// RFC 9110 5.6.1: whitespace is allowed *around* a list element, never
/// inside one.
fn trim_ows(mut v: &[u8]) -> &[u8] {
    while let Some((first, rest)) = v.split_first() {
        if *first == b' ' || *first == b'\t' {
            v = rest;
        } else {
            break;
        }
    }
    while let Some((last, rest)) = v.split_last() {
        if *last == b' ' || *last == b'\t' {
            v = rest;
        } else {
            break;
        }
    }
    v
}

fn eq_ci(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}
fn contains_ci(a: &[u8], needle: &[u8]) -> bool {
    a.windows(needle.len()).any(|w| eq_ci(w, needle))
}

fn percent_decode(input: &[u8]) -> Vec<u8> {
    fn hex_nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%'
            && index + 2 < input.len()
            && let (Some(high), Some(low)) =
                (hex_nibble(input[index + 1]), hex_nibble(input[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        decoded.push(input[index]);
        index += 1;
    }
    decoded
}

pub fn inspect(request: &[u8], max_bytes: usize) -> Option<Finding> {
    let p = match parse_request(request, max_bytes.min(256 * 1024)) {
        Ok(p) => p,
        Err(f) => return Some(f),
    };
    // Decode valid percent triplets before signatures so encoded payloads
    // cannot bypass inspection. Input and decoded output remain size-bounded.
    let decoded_target = percent_decode(p.target);
    let decoded_body = percent_decode(&p.body);
    let mut hay = Vec::with_capacity(decoded_target.len() + p.headers.len() + decoded_body.len());
    hay.extend_from_slice(&decoded_target);
    hay.push(b'\n');
    hay.extend_from_slice(p.headers);
    hay.push(b'\n');
    hay.extend_from_slice(&decoded_body);
    let lower = hay
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect::<Vec<_>>();
    // SSRF detection must not inspect Host: because localhost/loopback are
    // legitimate authority values for local health checks and loopback APIs.
    let mut ssrf_hay = Vec::with_capacity(decoded_target.len() + decoded_body.len());
    ssrf_hay.extend_from_slice(&decoded_target);
    ssrf_hay.push(b'\n');
    ssrf_hay.extend_from_slice(&decoded_body);
    let ssrf_lower = ssrf_hay
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if lower.windows(3).any(|w| w == b"../")
        || lower.windows(6).any(|w| w == b"%2e%2e/")
        || lower.windows(9).any(|w| w == b"%2e%2e%2f")
    {
        return Some(Finding::PathTraversal);
    }
    if contains_ci(&lower, b"union select")
        || contains_ci(&lower, b" or 1=1")
        || contains_ci(&lower, b"' or '")
        || contains_ci(&lower, b"information_schema")
        || contains_ci(&lower, b"sleep(")
    {
        return Some(Finding::SqlInjection);
    }
    if contains_ci(&lower, b"<script")
        || contains_ci(&lower, b"javascript:")
        || contains_ci(&lower, b"onerror=")
        || contains_ci(&lower, b"onload=")
    {
        return Some(Finding::Xss);
    }
    if contains_ci(&lower, b"/bin/sh")
        || contains_ci(&lower, b"/bin/bash")
        || contains_ci(&lower, b"cmd.exe")
        || contains_ci(&lower, b"powershell -")
    {
        return Some(Finding::CommandInjection);
    }
    if contains_ci(&ssrf_lower, b"169.254.169.254")
        || contains_ci(&ssrf_lower, b"127.0.0.1")
        || contains_ci(&ssrf_lower, b"localhost")
        || contains_ci(&ssrf_lower, b"[::1]")
    {
        return Some(Finding::Ssrf);
    }
    None
}

pub fn reason(f: Finding) -> &'static str {
    match f {
        Finding::SqlInjection => "waf:sqli",
        Finding::Xss => "waf:xss",
        Finding::PathTraversal => "waf:path-traversal",
        Finding::CommandInjection => "waf:command-injection",
        Finding::Ssrf => "waf:ssrf",
        Finding::Oversized => "waf:oversized-request",
        Finding::Malformed => "waf:malformed-request",
        Finding::HeaderSmuggling => "waf:header-smuggling",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localhost_host_header_is_not_ssrf() {
        assert_eq!(
            inspect(
                b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1:9999\r\n\r\n",
                65536
            ),
            None
        );
    }
    #[test]
    fn detects_sqli_in_query() {
        assert_eq!(
            inspect(
                b"GET /?q=1%20OR%201=1 HTTP/1.1\r\nHost: example\r\n\r\n",
                65536
            ),
            Some(Finding::SqlInjection)
        );
    }
    #[test]
    fn detects_xss_in_body() {
        assert_eq!(
            inspect(
                b"POST / HTTP/1.1\r\nContent-Length: 25\r\n\r\n<script>alert(1)</script>",
                65536
            ),
            Some(Finding::Xss)
        );
    }
    #[test]
    fn detects_traversal() {
        assert_eq!(
            inspect(b"GET /../../etc/passwd HTTP/1.1\r\n\r\n", 65536),
            Some(Finding::PathTraversal)
        );
    }
    #[test]
    fn rejects_oversize() {
        assert_eq!(
            inspect(b"GET / HTTP/1.1\r\n\r\n", 4),
            Some(Finding::Oversized)
        );
    }
    #[test]
    fn rejects_lf_only_between_headers() {
        assert_eq!(
            inspect(
                b"GET / HTTP/1.1\r\nHost: example\nX-Test: value\r\n\r\n",
                65536
            ),
            Some(Finding::Malformed)
        );
    }
    #[test]
    fn rejects_bad_length() {
        assert_eq!(
            inspect(b"POST / HTTP/1.1\r\nContent-Length: 9\r\n\r\nabc", 65536),
            Some(Finding::Malformed)
        );
    }
    #[test]
    fn rejects_conflicting_content_lengths() {
        assert_eq!(
            inspect(
                b"POST / HTTP/1.1\r\nContent-Length: 3\r\nContent-Length: 4\r\n\r\nabc",
                65536
            ),
            Some(Finding::HeaderSmuggling)
        );
    }
    #[test]
    fn rejects_content_length_and_chunked_together() {
        assert_eq!(inspect(b"POST / HTTP/1.1\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n", 65536), Some(Finding::HeaderSmuggling));
    }
    #[test]
    fn parses_chunked_body() {
        assert_eq!(inspect(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n12\r\n<script>x</script>\r\n0\r\n\r\n", 65536), Some(Finding::Xss));
    }

    // --- the framing rules, each with a negative control ---

    #[test]
    fn rejects_a_tab_separated_request_line() {
        // A tab is not a separator. A parser that treats it as one sees a
        // different request target than a parser that does not.
        assert_eq!(
            inspect(b"GET\t/evil\tHTTP/1.1\r\nHost: example\r\n\r\n", 65536),
            Some(Finding::Malformed)
        );
    }

    #[test]
    fn rejects_an_empty_request_line_component() {
        // Two spaces, or a trailing space: an empty component is not a
        // component, and the components must be separated by exactly one SP.
        assert_eq!(
            inspect(b"GET  /evil HTTP/1.1\r\nHost: example\r\n\r\n", 65536),
            Some(Finding::Malformed)
        );
        assert_eq!(
            inspect(b"GET /evil HTTP/1.1 \r\nHost: example\r\n\r\n", 65536),
            Some(Finding::Malformed)
        );
    }

    #[test]
    fn rejects_a_transfer_encoding_token_with_interior_whitespace() {
        // `chu nked` is not `chunked`. Stripping the interior whitespace
        // would make this gate make a framing decision it does not own.
        assert_eq!(
            inspect(
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chun ked\r\n\r\n0\r\n\r\n",
                65536
            ),
            Some(Finding::HeaderSmuggling)
        );
    }

    #[test]
    fn rejects_repeated_transfer_encoding_headers() {
        // The values combine, and `chunked` may appear only once in the
        // combined list.
        assert_eq!(
            inspect(
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
                65536
            ),
            Some(Finding::HeaderSmuggling)
        );
    }

    #[test]
    fn accepts_a_transfer_encoding_coding_with_surrounding_whitespace() {
        // Whitespace around a list element is legal (RFC 9110 5.6.1).
        assert_eq!(
            inspect(
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked \r\n\r\n0\r\n\r\n",
                65536
            ),
            None
        );
    }

    #[test]
    fn rejects_a_malformed_chunked_trailer() {
        // A trailer field is a header field: it has a name and a colon.
        assert_eq!(
            inspect(
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nNOT-A-TRAILER\r\n\r\n",
                65536
            ),
            Some(Finding::Malformed)
        );
    }

    #[test]
    fn rejects_bytes_after_the_chunked_terminator() {
        // The request ends at the terminator. What follows is a second
        // request in the same buffer, and a verdict on the first must not
        // cover it.
        assert_eq!(
            inspect(
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\nGET /evil HTTP/1.1\r\nHost: example\r\n\r\n",
                65536
            ),
            Some(Finding::Malformed)
        );
    }

    #[test]
    fn accepts_a_well_formed_chunked_trailer() {
        assert_eq!(
            inspect(
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-Checksum: abc\r\n\r\n",
                65536
            ),
            None
        );
    }
}
