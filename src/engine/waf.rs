//! Bounded deterministic HTTP/1 security inspection.
//!
//! This is intentionally a *security gate*, not a claim of full OWASP CRS
//! coverage. It enforces strict HTTP framing and bounded parsing first, then
//! applies cheap deterministic signatures. HTTPS/HTTP2/HTTP3 remain terminator
//! responsibilities unless a producer supplies decrypted HTTP telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finding { SqlInjection, Xss, PathTraversal, CommandInjection, Ssrf, Oversized, Malformed, HeaderSmuggling }

#[derive(Debug)]
struct RequestParts<'a> { target: &'a [u8], headers: &'a [u8], body: Vec<u8> }

const MAX_HEADERS: usize = 64;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_TARGET_BYTES: usize = 8192;
const MAX_HEADER_VALUE_BYTES: usize = 16 * 1024;

fn parse_request(request: &[u8], max_bytes: usize) -> Result<RequestParts<'_>, Finding> {
    if request.len() > max_bytes { return Err(Finding::Oversized); }
    let sep = request.windows(4).position(|w| w == b"\r\n\r\n").ok_or(Finding::Malformed)?;
    if sep > MAX_HEADER_BYTES { return Err(Finding::Oversized); }
    let head = &request[..sep];
    let lines: Vec<&[u8]> = head.split(|&b| b == b'\n').collect();
    let raw_request_line = *lines.first().ok_or(Finding::Malformed)?;
    // The delimiter search ends before the final CRLF, so a headerless
    // request line (and the final header line) is not present with its CR.
    // The delimiter itself proves the request line is CRLF-terminated.
    let request_line = raw_request_line.strip_suffix(b"\r").unwrap_or(raw_request_line);
    let mut parts = request_line.split(|&b| b == b' ' || b == b'\t').filter(|p| !p.is_empty());
    let method = parts.next().ok_or(Finding::Malformed)?;
    let target = parts.next().ok_or(Finding::Malformed)?;
    let version = parts.next().ok_or(Finding::Malformed)?;
    if parts.next().is_some() || method.len() > 16 || target.len() > MAX_TARGET_BYTES || (version != b"HTTP/1.1" && version != b"HTTP/1.0") { return Err(Finding::Malformed); }
    if !method.iter().all(|b| b.is_ascii_alphabetic()) || target.iter().any(|b| *b == 0 || *b == b'\r' || *b == b'\n') { return Err(Finding::Malformed); }

    let headers_start = request_line.len() + 2;
    let headers = if headers_start <= sep { &request[headers_start..sep] } else { &[] };
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
        if line.is_empty() { continue; }
        header_count += 1;
        if header_count > MAX_HEADERS { return Err(Finding::Oversized); }
        let colon = line.iter().position(|b| *b == b':').ok_or(Finding::Malformed)?;
        let name = &line[..colon];
        let value = line[colon+1..].strip_prefix(b" ").unwrap_or(&line[colon+1..]);
        if name.is_empty() || name.len() > 128 || !name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-') || value.len() > MAX_HEADER_VALUE_BYTES { return Err(Finding::Malformed); }
        if value.iter().any(|b| *b == 0 || *b == b'\r' || *b == b'\n' || (*b < 0x20 && *b != b'\t')) { return Err(Finding::Malformed); }
        if eq_ci(name, b"content-length") {
            let len = parse_decimal(value).ok_or(Finding::Malformed)?;
            if let Some(previous) = content_length { if previous != len { return Err(Finding::HeaderSmuggling); } } else { content_length = Some(len); }
        }
        if eq_ci(name, b"transfer-encoding") {
            // Security gate: reject unsupported transfer codings rather than
            // attempting to normalize an intermediary's framing semantics.
            // If chunked is present it must be the final coding.
            let tokens = value.split(|b| *b == b',')
                .map(|t| t.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect::<Vec<_>>());
            let mut token_count = 0usize;
            let mut chunked = false;
            for token in tokens {
                if token.is_empty() { return Err(Finding::Malformed); }
                token_count += 1;
                if eq_ci(&token, b"chunked") {
                    // chunked must be unique and final.
                    if chunked { return Err(Finding::HeaderSmuggling); }
                    chunked = true;
                } else if chunked {
                    return Err(Finding::HeaderSmuggling);
                } else {
                    return Err(Finding::HeaderSmuggling);
                }
            }
            if token_count == 0 || !chunked { return Err(Finding::HeaderSmuggling); }
            transfer_chunked = true;
        }
    }
    if transfer_chunked && content_length.is_some() { return Err(Finding::HeaderSmuggling); }
    let raw_body = &request[sep + 4..];
    let body = if transfer_chunked { decode_chunked(raw_body, max_bytes)? } else {
        if let Some(len) = content_length { if len != raw_body.len() || len > max_bytes { return Err(Finding::Malformed); } }
        raw_body.to_vec()
    };
    Ok(RequestParts { target, headers, body })
}

fn decode_chunked(raw: &[u8], max_bytes: usize) -> Result<Vec<u8>, Finding> {
    let mut pos = 0usize;
    let mut out = Vec::with_capacity(raw.len().min(max_bytes));
    loop {
        let rel = raw[pos..].windows(2).position(|w| w == b"\r\n").ok_or(Finding::Malformed)?;
        let end = pos + rel;
        let line = &raw[pos..end];
        let size_text = line.split(|b| *b == b';').next().unwrap_or(line);
        let size = usize::from_str_radix(std::str::from_utf8(size_text).map_err(|_| Finding::Malformed)?.trim(), 16).map_err(|_| Finding::Malformed)?;
        pos = end + 2;
        if size == 0 {
            if raw.get(pos..pos+2) == Some(b"\r\n") { return Ok(out); }
            // Trailer fields are permitted; parse them only as bounded syntax.
            let tail = &raw[pos..];
            if tail.len() > MAX_HEADER_BYTES { return Err(Finding::Oversized); }
            return if tail.windows(2).any(|w| w == b"\r\n") { Ok(out) } else { Err(Finding::Malformed) };
        }
        if size > max_bytes || pos.checked_add(size + 2).is_none() || pos + size + 2 > raw.len() { return Err(Finding::Malformed); }
        if out.len().saturating_add(size) > max_bytes { return Err(Finding::Oversized); }
        out.extend_from_slice(&raw[pos..pos+size]);
        pos += size;
        if raw.get(pos..pos+2) != Some(b"\r\n") { return Err(Finding::Malformed); }
        pos += 2;
    }
}

fn parse_decimal(v: &[u8]) -> Option<usize> {
    if v.is_empty() { return None; }
    let mut n = 0usize;
    for b in v.iter().copied() {
        if !b.is_ascii_digit() { return None; }
        n = n.checked_mul(10)?.checked_add((b - b'0') as usize)?;
    }
    Some(n)
}

fn eq_ci(a: &[u8], b: &[u8]) -> bool { a.len() == b.len() && a.iter().zip(b).all(|(x,y)| x.to_ascii_lowercase() == y.to_ascii_lowercase()) }
fn contains_ci(a: &[u8], needle: &[u8]) -> bool { a.windows(needle.len()).any(|w| eq_ci(w, needle)) }

pub fn inspect(request: &[u8], max_bytes: usize) -> Option<Finding> {
    let p = match parse_request(request, max_bytes.min(256 * 1024)) { Ok(p) => p, Err(f) => return Some(f) };
    let mut hay = Vec::with_capacity(p.target.len() + p.headers.len() + p.body.len());
    hay.extend_from_slice(p.target); hay.push(b'\n'); hay.extend_from_slice(p.headers); hay.push(b'\n'); hay.extend_from_slice(&p.body);
    let lower = hay.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<_>>();
    // SSRF detection must not inspect Host: because localhost/loopback are
    // legitimate authority values for local health checks and loopback APIs.
    let mut ssrf_hay = Vec::with_capacity(p.target.len() + p.body.len());
    ssrf_hay.extend_from_slice(p.target); ssrf_hay.push(b'\n'); ssrf_hay.extend_from_slice(&p.body);
    let ssrf_lower = ssrf_hay.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<_>>();
    if lower.windows(3).any(|w| w == b"../") || lower.windows(6).any(|w| w == b"%2e%2e/") || lower.windows(9).any(|w| w == b"%2e%2e%2f") { return Some(Finding::PathTraversal); }
    if contains_ci(&lower, b"union select") || contains_ci(&lower, b" or 1=1") || contains_ci(&lower, b"' or '") || contains_ci(&lower, b"information_schema") || contains_ci(&lower, b"sleep(") { return Some(Finding::SqlInjection); }
    if contains_ci(&lower, b"<script") || contains_ci(&lower, b"javascript:") || contains_ci(&lower, b"onerror=") || contains_ci(&lower, b"onload=") { return Some(Finding::Xss); }
    if contains_ci(&lower, b"/bin/sh") || contains_ci(&lower, b"/bin/bash") || contains_ci(&lower, b"cmd.exe") || contains_ci(&lower, b"powershell -") { return Some(Finding::CommandInjection); }
    if contains_ci(&ssrf_lower, b"169.254.169.254") || contains_ci(&ssrf_lower, b"127.0.0.1") || contains_ci(&ssrf_lower, b"localhost") || contains_ci(&ssrf_lower, b"[::1]") { return Some(Finding::Ssrf); }
    None
}

pub fn reason(f: Finding) -> &'static str { match f { Finding::SqlInjection=>"waf:sqli", Finding::Xss=>"waf:xss", Finding::PathTraversal=>"waf:path-traversal", Finding::CommandInjection=>"waf:command-injection", Finding::Ssrf=>"waf:ssrf", Finding::Oversized=>"waf:oversized-request", Finding::Malformed=>"waf:malformed-request", Finding::HeaderSmuggling=>"waf:header-smuggling" } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn localhost_host_header_is_not_ssrf() {
        assert_eq!(inspect(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1:9999\r\n\r\n", 65536), None);
    }
    #[test] fn detects_sqli_in_query() { assert_eq!(inspect(b"GET /?q=1%20OR%201=1 HTTP/1.1\r\nHost: example\r\n\r\n", 65536), Some(Finding::SqlInjection)); }
    #[test] fn detects_xss_in_body() { assert_eq!(inspect(b"POST / HTTP/1.1\r\nContent-Length: 25\r\n\r\n<script>alert(1)</script>", 65536), Some(Finding::Xss)); }
    #[test] fn detects_traversal() { assert_eq!(inspect(b"GET /../../etc/passwd HTTP/1.1\r\n\r\n", 65536), Some(Finding::PathTraversal)); }
    #[test] fn rejects_oversize() { assert_eq!(inspect(b"GET / HTTP/1.1\r\n\r\n", 4), Some(Finding::Oversized)); }
    #[test] fn rejects_lf_only_between_headers() { assert_eq!(inspect(b"GET / HTTP/1.1\r\nHost: example\nX-Test: value\r\n\r\n", 65536), Some(Finding::Malformed)); }
    #[test] fn rejects_bad_length() { assert_eq!(inspect(b"POST / HTTP/1.1\r\nContent-Length: 9\r\n\r\nabc", 65536), Some(Finding::Malformed)); }
    #[test] fn rejects_conflicting_content_lengths() { assert_eq!(inspect(b"POST / HTTP/1.1\r\nContent-Length: 3\r\nContent-Length: 4\r\n\r\nabc", 65536), Some(Finding::HeaderSmuggling)); }
    #[test] fn rejects_content_length_and_chunked_together() { assert_eq!(inspect(b"POST / HTTP/1.1\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n", 65536), Some(Finding::HeaderSmuggling)); }
    #[test] fn parses_chunked_body() { assert_eq!(inspect(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n9\r\n<script>x</script>\r\n0\r\n\r\n", 65536), Some(Finding::Xss)); }
}
