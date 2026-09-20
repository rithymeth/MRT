//! MRT-Net: an HTTP/1.1 client, hand-written, with no dependencies.
//!
//! The split here is the one `mrt-audio` and `mrt-speaker` already make.
//! Everything that is *arithmetic on bytes* lives in this module -- building a
//! request, parsing a status line, decoding a chunked body -- and is tested
//! with no socket anywhere in sight. [`client`] is the part that opens a
//! connection, and it needs nothing but `std::net`.
//!
//! # HTTPS is deliberately absent
//!
//! Plain HTTP over TCP is free: `std::net` is in the standard library, so this
//! crate keeps the workspace's zero-dependency promise. TLS is not free. It
//! means `rustls` and the tree underneath it, or platform FFI, and that is a
//! decision about what this project is willing to depend on rather than
//! something to slip in under a feature flag. Until it is made, an `https://`
//! URL is refused by name and says why, because silently fetching it over
//! plain HTTP would be worse than not fetching it at all.
//!
//! # What is not here
//!
//! No server, no sockets exposed to the language, no async. A one-shot
//! request needs none of them, and each would need a concurrency story the
//! language does not have.

pub mod client;

use std::fmt;

/// Everything that can go wrong, kept apart so the bridge can phrase each one
/// for the language rather than leaking Rust's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// The URL could not be read as one.
    BadUrl(String),
    /// The request could not be built -- almost always a header that would
    /// have injected something into the wire format.
    BadRequest(String),
    /// The server answered with something that is not HTTP/1.x.
    BadResponse(String),
    /// A scheme, or anything else, this build does not do.
    Unsupported(String),
    /// The connection itself failed.
    Io(String),
    /// The whole exchange outran its deadline.
    Timeout,
    /// The response was larger than [`client::MAX_RESPONSE`].
    TooLarge(usize),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::BadUrl(m) => write!(f, "{m}"),
            NetError::BadRequest(m) => write!(f, "{m}"),
            NetError::BadResponse(m) => write!(f, "{m}"),
            NetError::Unsupported(m) => write!(f, "{m}"),
            NetError::Io(m) => write!(f, "{m}"),
            NetError::Timeout => write!(f, "the request timed out"),
            NetError::TooLarge(max) => write!(f, "the response was larger than {max} bytes"),
        }
    }
}

/// A URL, taken apart far enough to connect and to write a request line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    /// Path and query together: what goes on the request line verbatim.
    pub path: String,
}

impl Url {
    /// What the `Host` header says: the port is named only when it is not the
    /// scheme's default, which is what every other client does and what some
    /// virtual hosts match on.
    pub fn authority(&self) -> String {
        let default = if self.scheme == "https" { 443 } else { 80 };
        if self.port == default {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// Parse an absolute `http://` or `https://` URL.
pub fn parse_url(raw: &str) -> Result<Url, NetError> {
    let Some((scheme, rest)) = raw.split_once("://") else {
        return Err(NetError::BadUrl(format!(
            "{raw} is not an absolute URL; it needs to start with http://"
        )));
    };

    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(NetError::BadUrl(format!(
            "{scheme}:// is not a scheme this understands; only http and https"
        )));
    }

    let (authority, path) = match rest.find('/') {
        Some(cut) => (&rest[..cut], rest[cut..].to_string()),
        None => (rest, "/".to_string()),
    };

    // A URL carrying credentials would put them on the wire in a header this
    // crate does not build. Refusing is better than dropping them silently.
    if authority.contains('@') {
        return Err(NetError::BadUrl(
            "a URL with credentials in it is not supported".to_string(),
        ));
    }

    let default_port = if scheme == "https" { 443 } else { 80 };
    let (host, port) = split_authority(authority, default_port)?;

    if host.is_empty() {
        return Err(NetError::BadUrl(format!("{raw} has no host")));
    }
    // The request line is the first thing on the wire. A space or a control
    // character in it is a second request as far as some servers are
    // concerned, so neither gets that far.
    if let Some(bad) = path.chars().find(|c| c.is_whitespace() || c.is_control()) {
        return Err(NetError::BadUrl(format!(
            "the path contains {bad:?}, which cannot go in a request line"
        )));
    }
    if host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(NetError::BadUrl(
            "the host contains whitespace or a control character".to_string(),
        ));
    }

    Ok(Url {
        scheme,
        host: host.to_ascii_lowercase(),
        port,
        path,
    })
}

/// Split `host:port`, `host`, or an IPv6 literal in brackets.
fn split_authority(authority: &str, default_port: u16) -> Result<(&str, u16), NetError> {
    if let Some(close) = authority
        .strip_prefix('[')
        .and_then(|_| authority.find(']'))
    {
        let host = &authority[1..close];
        let rest = &authority[close + 1..];
        let port = match rest.strip_prefix(':') {
            Some(digits) => parse_port(digits)?,
            None if rest.is_empty() => default_port,
            None => {
                return Err(NetError::BadUrl(format!(
                    "{authority} is not a host and port"
                )))
            }
        };
        return Ok((host, port));
    }

    match authority.rsplit_once(':') {
        Some((host, digits)) => Ok((host, parse_port(digits)?)),
        None => Ok((authority, default_port)),
    }
}

fn parse_port(digits: &str) -> Result<u16, NetError> {
    digits
        .parse::<u16>()
        .map_err(|_| NetError::BadUrl(format!("{digits} is not a port number")))
}

/// Headers the caller may not set, because this crate sets them and two of
/// either is how a request gets read twice by two different machines.
const RESERVED: &[&str] = &["host", "content-length", "connection", "transfer-encoding"];

/// One request, ready to be turned into bytes.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn new(method: &str, url: Url, body: Vec<u8>) -> Self {
        Request {
            method: method.to_string(),
            url,
            headers: Vec::new(),
            body,
        }
    }

    /// The bytes that go on the wire.
    ///
    /// `Connection: close` is not a detail: it is what lets the reader stop at
    /// end of stream instead of tracking where one response ends and the next
    /// begins. One request per connection, and the server says when it is done.
    pub fn to_bytes(&self) -> Result<Vec<u8>, NetError> {
        check_token("method", &self.method)?;

        let mut head = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
            self.method,
            self.url.path,
            self.url.authority()
        );

        if !self.body.is_empty() {
            head.push_str(&format!("Content-Length: {}\r\n", self.body.len()));
        }

        for (name, value) in &self.headers {
            check_token("header name", name)?;
            if RESERVED.contains(&name.to_ascii_lowercase().as_str()) {
                return Err(NetError::BadRequest(format!(
                    "{name} is set by the client and cannot be overridden"
                )));
            }
            if value.contains(['\r', '\n']) || value.chars().any(|c| c.is_control()) {
                return Err(NetError::BadRequest(format!(
                    "the value of {name} contains a control character"
                )));
            }
            head.push_str(&format!("{name}: {value}\r\n"));
        }

        head.push_str("\r\n");
        let mut out = head.into_bytes();
        out.extend_from_slice(&self.body);
        Ok(out)
    }
}

/// HTTP tokens: no spaces, no separators, nothing that ends a line.
fn check_token(what: &str, text: &str) -> Result<(), NetError> {
    if text.is_empty() {
        return Err(NetError::BadRequest(format!("the {what} is empty")));
    }
    let ok = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c);
    match text.chars().find(|c| !ok(*c)) {
        Some(bad) => Err(NetError::BadRequest(format!(
            "the {what} contains {bad:?}, which is not allowed there"
        ))),
        None => Ok(()),
    }
}

/// One response, taken apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    /// Header lookup, case-insensitive as the protocol requires.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Parse a whole response: everything the server sent before closing.
pub fn parse_response(raw: &[u8]) -> Result<Response, NetError> {
    let Some(split) = find(raw, b"\r\n\r\n") else {
        if raw.is_empty() {
            return Err(NetError::BadResponse(
                "the server closed the connection without answering".to_string(),
            ));
        }
        return Err(NetError::BadResponse(
            "the response headers never ended".to_string(),
        ));
    };

    let head = std::str::from_utf8(&raw[..split])
        .map_err(|_| NetError::BadResponse("the response headers are not text".to_string()))?;
    let body = &raw[split + 4..];

    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let (status, reason) = parse_status_line(status_line)?;

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        // Obsolete line folding: legal in 1997, a request-smuggling trick now.
        if line.starts_with(' ') || line.starts_with('\t') {
            return Err(NetError::BadResponse(
                "the response folds a header across lines".to_string(),
            ));
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(NetError::BadResponse(format!("{line} is not a header")));
        };
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }

    let lookup = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };

    let body = if lookup("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        decode_chunked(body)?
    } else if let Some(len) = lookup("content-length") {
        let len: usize = len
            .trim()
            .parse()
            .map_err(|_| NetError::BadResponse(format!("{len} is not a Content-Length")))?;
        if body.len() < len {
            return Err(NetError::BadResponse(format!(
                "the body stopped after {} of {len} bytes",
                body.len()
            )));
        }
        body[..len].to_vec()
    } else {
        // No length and no chunking: the body is whatever arrived before the
        // close, which is exactly what Connection: close makes unambiguous.
        body.to_vec()
    };

    Ok(Response {
        status,
        reason,
        headers,
        body,
    })
}

fn parse_status_line(line: &str) -> Result<(u16, String), NetError> {
    let mut parts = line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err(NetError::BadResponse(format!(
            "{line} is not an HTTP/1.x status line"
        )));
    }
    let code = parts.next().unwrap_or_default();
    let status: u16 = code
        .parse()
        .map_err(|_| NetError::BadResponse(format!("{code} is not a status code")))?;
    if !(100..=599).contains(&status) {
        return Err(NetError::BadResponse(format!(
            "{status} is not a status code"
        )));
    }
    Ok((status, parts.next().unwrap_or_default().to_string()))
}

/// Chunked transfer coding: a hex length, the bytes, repeat, zero to finish.
fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, NetError> {
    let mut out = Vec::new();
    loop {
        let Some(end) = find(data, b"\r\n") else {
            return Err(NetError::BadResponse(
                "a chunk header never ended".to_string(),
            ));
        };

        // Chunk extensions hang off a semicolon and nothing here wants them.
        let header = &data[..end];
        let header = header.split(|b| *b == b';').next().unwrap_or(header);
        let text = std::str::from_utf8(header)
            .map_err(|_| NetError::BadResponse("a chunk length is not text".to_string()))?;
        let size = usize::from_str_radix(text.trim(), 16)
            .map_err(|_| NetError::BadResponse(format!("{text} is not a chunk length")))?;

        data = &data[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            return Err(NetError::BadResponse("a chunk ended early".to_string()));
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        parse_url(raw).expect("url")
    }

    #[test]
    fn a_url_comes_apart_into_its_pieces() {
        assert_eq!(
            url("http://example.com/a/b?c=1"),
            Url {
                scheme: "http".into(),
                host: "example.com".into(),
                port: 80,
                path: "/a/b?c=1".into(),
            }
        );
        assert_eq!(url("http://example.com").path, "/");
        assert_eq!(url("http://example.com:8080/").port, 8080);
        assert_eq!(url("https://example.com/").port, 443);
        assert_eq!(url("HTTP://Example.COM/").host, "example.com");
    }

    #[test]
    fn an_ipv6_literal_keeps_its_brackets_out_of_the_host() {
        assert_eq!(url("http://[::1]:8080/x").host, "::1");
        assert_eq!(url("http://[::1]:8080/x").port, 8080);
        assert_eq!(url("http://[::1]/").port, 80);
    }

    #[test]
    fn a_url_that_is_not_one_says_which_part_failed() {
        for (raw, wanted) in [
            ("example.com/x", "absolute"),
            ("ftp://example.com/", "scheme"),
            ("http://user:pass@example.com/", "credentials"),
            ("http://example.com:70000/", "port"),
            ("http:///x", "host"),
        ] {
            let message = parse_url(raw).expect_err(raw).to_string();
            assert!(message.contains(wanted), "{raw}: {message}");
        }
    }

    #[test]
    fn a_request_line_cannot_be_split_in_two() {
        // The whole point: a path carrying CRLF would be a second request.
        let message = parse_url("http://example.com/a\r\nX-Evil: 1")
            .expect_err("should refuse")
            .to_string();
        assert!(message.contains("request line"), "{message}");
    }

    #[test]
    fn a_get_is_the_bytes_a_server_expects() {
        let request = Request::new("GET", url("http://example.com/x"), Vec::new());
        assert_eq!(
            String::from_utf8(request.to_bytes().expect("bytes")).expect("utf8"),
            "GET /x HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn a_body_brings_its_own_length() {
        let request = Request::new("POST", url("http://example.com:9/x"), b"hi".to_vec());
        let wire = String::from_utf8(request.to_bytes().expect("bytes")).expect("utf8");
        assert!(wire.contains("Host: example.com:9\r\n"), "{wire}");
        assert!(wire.contains("Content-Length: 2\r\n"), "{wire}");
        assert!(wire.ends_with("\r\n\r\nhi"), "{wire}");
    }

    #[test]
    fn a_header_cannot_smuggle_a_second_one() {
        let mut request = Request::new("GET", url("http://example.com/"), Vec::new());
        request
            .headers
            .push(("X-Thing".into(), "a\r\nX-Evil: 1".into()));
        let message = request.to_bytes().expect_err("should refuse").to_string();
        assert!(message.contains("control character"), "{message}");
    }

    #[test]
    fn the_headers_this_crate_owns_cannot_be_overridden() {
        for name in ["Host", "content-length", "Connection", "Transfer-Encoding"] {
            let mut request = Request::new("GET", url("http://example.com/"), Vec::new());
            request.headers.push((name.into(), "x".into()));
            let message = request.to_bytes().expect_err(name).to_string();
            assert!(
                message.contains("cannot be overridden"),
                "{name}: {message}"
            );
        }
    }

    #[test]
    fn a_response_comes_apart_into_its_pieces() {
        let raw = b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\nX-A: 1\r\n\r\nok";
        let response = parse_response(raw).expect("parse");
        assert_eq!(response.status, 201);
        assert_eq!(response.reason, "Created");
        assert_eq!(response.body, b"ok");
        assert_eq!(response.header("X-a"), Some("1"));
        assert_eq!(response.header("missing"), None);
    }

    #[test]
    fn a_chunked_body_is_put_back_together() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n1;ext=x\r\nd\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).expect("parse").body, b"abcd");
    }

    #[test]
    fn a_truncated_body_is_an_error_rather_than_a_short_one() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort";
        let message = parse_response(raw).expect_err("should refuse").to_string();
        assert!(message.contains("stopped after 5 of 10"), "{message}");
    }

    #[test]
    fn a_response_that_is_not_http_says_so() {
        for (raw, wanted) in [
            (&b""[..], "without answering"),
            (&b"hello"[..], "never ended"),
            (&b"GARBAGE\r\n\r\n"[..], "status line"),
            (&b"HTTP/1.1 twenty OK\r\n\r\n"[..], "status code"),
            (
                &b"HTTP/1.1 200 OK\r\nnot-a-header\r\n\r\n"[..],
                "not a header",
            ),
            (
                &b"HTTP/1.1 200 OK\r\nX: 1\r\n  folded\r\n\r\n"[..],
                "folds a header",
            ),
        ] {
            let message = parse_response(raw).expect_err("should refuse").to_string();
            assert!(message.contains(wanted), "{message}");
        }
    }
}
