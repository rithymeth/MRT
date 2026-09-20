//! The MRT-facing side of the network extension.
//!
//! -- A response is ordinary MRT data --
//!
//! The same rule MRT-AI follows, for the same reason. The obvious design
//! hands the language a connection handle and a set of methods to poke it
//! with; this one hands back an object:
//!
//! ```text
//! {status: 200, ok: true, headers: {content-type: "application/json"}, body: "..."}
//! ```
//!
//! So `print(reply)` shows the whole thing, `reply.headers` is a map anyone
//! can iterate, and it all survives `toString` and comparison. No value enters
//! the language that the language cannot already describe -- which matters
//! more for an extension than for core, because an extension that adds a type
//! is no longer optional to understand.
//!
//! -- What a program can and cannot say --
//!
//! `Host`, `Content-Length`, `Connection` and `Transfer-Encoding` belong to
//! the client; a program setting them is refused rather than obeyed, because
//! two of any of them is how one request gets read as two. Header values
//! carrying a newline are refused for the same reason.
//!
//! The body comes back as a string, so a response that is not text is an
//! error rather than mojibake: MRT has no bytes type, and a lossy decode
//! would hand a program something that looks like data and is not.

use std::rc::Rc;
use std::time::Duration;

use mrt_net::client::fetch;
use mrt_net::{parse_url, NetError, Request};

use crate::error::{type_error, value_error, Signal};
use crate::value::{type_name, ObjKey, ObjMap, Value};

/// How long an exchange gets when the caller does not say.
const DEFAULT_TIMEOUT: f64 = 10.0;

/// `netGet(url)` / `netGet(url, options)`.
pub fn get(args: &[Value]) -> Result<Value, Signal> {
    request("netGet", "GET", args, None)
}

/// `netPost(url, body)` / `netPost(url, body, options)`.
pub fn post(args: &[Value]) -> Result<Value, Signal> {
    let Some(body) = args.get(1) else {
        return Err(value_error("netPost() takes a url and a body."));
    };
    let Value::Str(text) = body else {
        return Err(type_error(format!(
            "netPost() needs a body that is text, not {}.",
            type_name(body)
        )));
    };
    let body = text.as_bytes().to_vec();
    request("netPost", "POST", args, Some(body))
}

fn request(
    who: &str,
    method: &str,
    args: &[Value],
    body: Option<Vec<u8>>,
) -> Result<Value, Signal> {
    let Some(Value::Str(raw)) = args.first() else {
        let found = args.first().map(type_name).unwrap_or("nothing".to_string());
        return Err(type_error(format!("{who}() needs a url, not {found}.")));
    };

    let url = parse_url(raw).map_err(|e| failed(who, e))?;
    let mut request = Request::new(method, url, body.unwrap_or_default());

    let options = if method == "POST" {
        args.get(2)
    } else {
        args.get(1)
    };
    let mut timeout = DEFAULT_TIMEOUT;
    if let Some(options) = options {
        timeout = read_options(who, options, &mut request)?;
    }

    let reply = fetch(&request, Duration::from_secs_f64(timeout)).map_err(|e| failed(who, e))?;

    let text = String::from_utf8(reply.body).map_err(|e| {
        value_error(format!(
            "{who}(): the response body is not text ({} bytes).",
            e.as_bytes().len()
        ))
    })?;

    let mut headers = ObjMap::new();
    for (name, value) in reply.headers {
        // Lowercased, because HTTP header names are case-insensitive and a
        // program indexing `headers["content-type"]` should not have to guess
        // which capitalisation this particular server chose.
        headers.insert(
            ObjKey::Str(Rc::from(name.to_ascii_lowercase().as_str())),
            Value::str(value),
        );
    }

    let mut out = ObjMap::new();
    out.insert(
        ObjKey::Str(Rc::from("status")),
        Value::Number(reply.status as f64),
    );
    out.insert(
        ObjKey::Str(Rc::from("ok")),
        Value::Bool((200..300).contains(&reply.status)),
    );
    out.insert(ObjKey::Str(Rc::from("headers")), Value::object(headers));
    out.insert(ObjKey::Str(Rc::from("body")), Value::str(text));
    Ok(Value::object(out))
}

/// Read `{headers: {...}, timeout: seconds}`, returning the timeout.
fn read_options(who: &str, options: &Value, request: &mut Request) -> Result<f64, Signal> {
    let Value::Object(map) = options else {
        return Err(type_error(format!(
            "{who}() needs its options to be an object, not {}.",
            type_name(options)
        )));
    };

    let map = map.borrow();
    let mut timeout = DEFAULT_TIMEOUT;

    if let Some(value) = map.get(&ObjKey::Str(Rc::from("timeout"))) {
        let Value::Number(seconds) = value else {
            return Err(type_error(format!(
                "{who}() needs timeout to be a number, not {}.",
                type_name(value)
            )));
        };
        if !seconds.is_finite() || *seconds <= 0.0 {
            return Err(value_error(format!(
                "{who}() needs timeout to be more than zero, not {seconds}."
            )));
        }
        timeout = *seconds;
    }

    if let Some(value) = map.get(&ObjKey::Str(Rc::from("headers"))) {
        let Value::Object(headers) = value else {
            return Err(type_error(format!(
                "{who}() needs headers to be an object, not {}.",
                type_name(value)
            )));
        };
        for (name, value) in headers.borrow().iter() {
            let Value::Str(text) = value else {
                return Err(type_error(format!(
                    "{who}(): the header {name} needs a text value, not {}.",
                    type_name(value)
                )));
            };
            request.headers.push((name.to_string(), text.to_string()));
        }
    }

    Ok(timeout)
}

/// One kind for every network failure. The language's error kinds are core's,
/// and an extension does not get to add one -- so the message carries what
/// went wrong and `e.kind` stays ValueError, as it does for a sound that will
/// not load or a PNG that will not write.
fn failed(who: &str, error: NetError) -> Signal {
    value_error(format!("{who}(): {error}"))
}

#[cfg(test)]
mod tests {
    use crate::{run, run_vm, SourceFile};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;

    /// Run a program on both engines, insist they agree, and return what it
    /// printed -- with any error appended, so a test can assert on a message
    /// as easily as on output.
    ///
    /// Both engines, because an extension is the one place a caller reaches
    /// Rust through two different paths: one that worked on the tree-walker
    /// and not the VM would be found by nobody, the conformance corpus
    /// deliberately not covering either.
    fn go(source: &str) -> String {
        let file = SourceFile::new("test.mrt", source);
        let text = |outcome: crate::Outcome| {
            let mut lines = outcome.output;
            if let Some(error) = outcome.error {
                lines.push(error);
            }
            lines.join("\n")
        };
        let walked = text(run(&file));
        let compiled = text(run_vm(&file));
        assert_eq!(walked, compiled, "the engines disagree");
        walked
    }

    /// A server on the loopback answering one request. Nothing leaves the
    /// machine, so this is as hermetic as the rest of the suite.
    ///
    /// It is spawned twice per test, because `go` runs the program on both
    /// engines and each one makes its own request.
    fn serve(reply: &'static [u8], times: usize) -> (u16, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();

        let handle = thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..times {
                let (mut socket, _) = listener.accept().expect("accept");
                let mut reader = BufReader::new(socket.try_clone().expect("clone"));
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).expect("read") == 0 || line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                // Whatever body followed the headers, read by length so the
                // server does not block waiting for a close.
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .and_then(|n| n.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if length > 0 {
                    let mut body = vec![0u8; length];
                    std::io::Read::read_exact(&mut reader, &mut body).expect("body");
                    head.push_str(&String::from_utf8_lossy(&body));
                }
                socket.write_all(reply).expect("write");
                seen.push(head);
            }
            seen
        });

        (port, handle)
    }

    #[test]
    fn a_response_is_an_object_the_language_can_look_inside() {
        let (port, server) = serve(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello",
            2,
        );

        assert_eq!(
            go(&format!(
                r#"func main() {{
                     var r = netGet("http://127.0.0.1:{port}/hi");
                     print(r.status, r.ok, r.body);
                     print(r.headers["content-type"]);
                     print(keys(r));
                   }}"#
            )),
            "200 true hello\ntext/plain\n[status, ok, headers, body]"
        );
        server.join().expect("join");
    }

    #[test]
    fn a_header_name_is_lowercased_whatever_the_server_chose() {
        let (port, server) = serve(b"HTTP/1.1 200 OK\r\nX-Mixed-Case: yes\r\n\r\n", 2);
        assert_eq!(
            go(&format!(
                r#"func main() {{ print(netGet("http://127.0.0.1:{port}/").headers["x-mixed-case"]); }}"#
            )),
            "yes"
        );
        server.join().expect("join");
    }

    #[test]
    fn a_post_sends_its_body_and_its_headers() {
        let (port, server) = serve(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\nok", 2);
        assert_eq!(
            go(&format!(
                r#"func main() {{
                     var r = netPost("http://127.0.0.1:{port}/new", "{{\"a\":1}}",
                                     {{headers: {{"Content-Type": "application/json"}}}});
                     print(r.status, r.ok, r.body);
                   }}"#
            )),
            "201 true ok"
        );

        let sent = server.join().expect("join");
        assert!(sent[0].starts_with("POST /new HTTP/1.1\r\n"), "{}", sent[0]);
        assert!(
            sent[0].contains("Content-Type: application/json\r\n"),
            "{}",
            sent[0]
        );
        assert!(sent[0].contains("Content-Length: 7\r\n"), "{}", sent[0]);
        assert!(sent[0].ends_with(r#"{"a":1}"#), "{}", sent[0]);
    }

    #[test]
    fn a_failure_is_catchable_like_any_other() {
        // Bound and dropped: almost certainly nothing is listening there.
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port();

        assert_eq!(
            go(&format!(
                r#"func main() {{
                     try {{ netGet("http://127.0.0.1:{port}/"); }}
                     catch (e) {{ print(e.kind, startsWith(e.message, "netGet():")); }}
                   }}"#
            )),
            "ValueError true"
        );
    }

    #[test]
    fn https_says_what_it_needs_instead_of_falling_back() {
        let out = go(r#"func main() { netGet("https://example.com/"); }"#);
        assert!(out.contains("needs TLS"), "{out}");
        assert!(out.contains("only http://"), "{out}");
    }

    #[test]
    fn the_headers_the_client_owns_cannot_be_overridden() {
        let out =
            go(r#"func main() { netGet("http://127.0.0.1:1/", {headers: {Host: "elsewhere"}}); }"#);
        assert!(out.contains("cannot be overridden"), "{out}");
    }

    #[test]
    fn a_header_value_cannot_carry_a_second_request() {
        let out = go(
            r#"func main() { netGet("http://127.0.0.1:1/", {headers: {"X-A": "a\nGET / HTTP/1.1"}}); }"#,
        );
        assert!(out.contains("control character"), "{out}");
    }

    #[test]
    fn a_body_that_is_not_text_says_so_rather_than_mangling_it() {
        let (port, server) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n\xff\xfe", 2);
        let out = go(&format!(
            r#"func main() {{ netGet("http://127.0.0.1:{port}/"); }}"#
        ));
        assert!(out.contains("not text (2 bytes)"), "{out}");
        server.join().expect("join");
    }

    #[test]
    fn the_arguments_are_checked_before_anything_is_opened() {
        assert!(go(r#"func main() { netGet(7); }"#).contains("needs a url, not number"));
        assert!(go(r#"func main() { netGet(); }"#).contains("netGet() takes 1 or 2 arguments."));
        assert!(go(r#"func main() { netGet("nonsense"); }"#).contains("absolute URL"));
        assert!(go(r#"func main() { netPost("http://a/", 7); }"#)
            .contains("needs a body that is text, not number"));
        assert!(
            go(r#"func main() { netGet("http://a/", {timeout: 0}); }"#).contains("more than zero"),
        );
        assert!(go(r#"func main() { netGet("http://a/", 7); }"#)
            .contains("options to be an object, not number"));
    }
}
