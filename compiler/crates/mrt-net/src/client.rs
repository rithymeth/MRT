//! The half that opens a connection. `std::net`, and nothing else.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::{parse_response, NetError, Request, Response};

/// A ceiling on what one response may occupy in memory. A client with no
/// limit is a client a server can use to exhaust the machine running it.
pub const MAX_RESPONSE: usize = 8 * 1024 * 1024;

/// Send one request and read the whole answer.
///
/// `timeout` covers the *exchange*, not each read: a server dribbling one
/// byte at a time cannot hold the caller forever by keeping each individual
/// read inside its own limit.
pub fn fetch(request: &Request, timeout: Duration) -> Result<Response, NetError> {
    if request.url.scheme != "http" {
        return Err(NetError::Unsupported(format!(
            "{}:// needs TLS, which this build does not have; only http:// works",
            request.url.scheme
        )));
    }

    let deadline = Instant::now() + timeout;
    let wire = request.to_bytes()?;
    let mut stream = connect(&request.url.host, request.url.port, deadline)?;

    stream
        .set_write_timeout(Some(remaining(deadline)?))
        .map_err(|e| NetError::Io(e.to_string()))?;
    stream.write_all(&wire).map_err(io_error)?;
    stream.flush().map_err(io_error)?;

    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        stream
            .set_read_timeout(Some(remaining(deadline)?))
            .map_err(|e| NetError::Io(e.to_string()))?;

        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if raw.len() + n > MAX_RESPONSE {
                    return Err(NetError::TooLarge(MAX_RESPONSE));
                }
                raw.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(io_error(e)),
        }
    }

    parse_response(&raw)
}

/// Resolve and connect, trying every address the name gives back -- a host
/// with an unreachable AAAA and a working A is ordinary.
fn connect(host: &str, port: u16, deadline: Instant) -> Result<TcpStream, NetError> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|e| NetError::Io(format!("cannot find {host}: {e}")))?;

    let mut last = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, remaining(deadline)?) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = Some(e),
        }
    }

    Err(match last {
        Some(e) => io_error(e),
        None => NetError::Io(format!("{host} resolved to no addresses")),
    })
}

/// How long is left. Running out is the timeout, not a zero-length wait --
/// `set_read_timeout(0)` means *no* timeout, which is the opposite.
fn remaining(deadline: Instant) -> Result<Duration, NetError> {
    let now = Instant::now();
    if now >= deadline {
        return Err(NetError::Timeout);
    }
    Ok(deadline - now)
}

/// A socket timeout surfaces as WouldBlock or TimedOut depending on the
/// platform, and both mean the deadline, not a broken connection.
fn io_error(e: std::io::Error) -> NetError {
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => NetError::Timeout,
        _ => NetError::Io(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_url;
    use std::io::BufRead;
    use std::net::TcpListener;
    use std::thread;

    /// A server on the loopback that answers once with `reply`, and hands back
    /// the request line it was sent. No network, so this runs anywhere.
    fn serve(reply: &'static [u8]) -> (u16, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();

        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut reader = std::io::BufReader::new(socket.try_clone().expect("clone"));
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).expect("read") == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            socket.write_all(reply).expect("write");
            head
        });

        (port, handle)
    }

    fn get(port: u16, path: &str) -> Result<Response, NetError> {
        let url = parse_url(&format!("http://127.0.0.1:{port}{path}")).expect("url");
        fetch(
            &Request::new("GET", url, Vec::new()),
            Duration::from_secs(5),
        )
    }

    #[test]
    fn a_request_goes_out_and_an_answer_comes_back() {
        let (port, server) =
            serve(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello");

        let response = get(port, "/thing?x=1").expect("fetch");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"hello");
        assert_eq!(response.header("content-type"), Some("text/plain"));

        let sent = server.join().expect("join");
        assert!(sent.starts_with("GET /thing?x=1 HTTP/1.1\r\n"), "{sent}");
        assert!(
            sent.contains(&format!("Host: 127.0.0.1:{port}\r\n")),
            "{sent}"
        );
        assert!(sent.contains("Connection: close\r\n"), "{sent}");
    }

    #[test]
    fn a_chunked_body_arrives_whole() {
        let (port, server) = serve(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n2\r\n, \r\n5\r\nworld\r\n0\r\n\r\n",
        );

        let response = get(port, "/").expect("fetch");
        assert_eq!(response.body, b"hello, world");
        server.join().expect("join");
    }

    #[test]
    fn a_body_with_no_length_ends_at_the_close() {
        let (port, server) = serve(b"HTTP/1.1 404 Not Found\r\n\r\ngone");
        let response = get(port, "/").expect("fetch");
        assert_eq!((response.status, response.body), (404, b"gone".to_vec()));
        server.join().expect("join");
    }

    #[test]
    fn nothing_listening_is_an_io_error() {
        // Bound and dropped, so the port is almost certainly free and nothing
        // is answering on it.
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port();

        match get(port, "/") {
            Err(NetError::Io(_)) => {}
            other => panic!("expected an Io error, got {other:?}"),
        }
    }

    #[test]
    fn a_server_that_never_answers_hits_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let held = thread::spawn(move || {
            let socket = listener.accept().expect("accept");
            // Hold the connection open, saying nothing, until the client gives
            // up and the test drops this.
            thread::sleep(Duration::from_millis(600));
            drop(socket);
        });

        let url = parse_url(&format!("http://127.0.0.1:{port}/")).expect("url");
        let result = fetch(
            &Request::new("GET", url, Vec::new()),
            Duration::from_millis(120),
        );
        assert_eq!(result, Err(NetError::Timeout));
        held.join().expect("join");
    }

    #[test]
    fn https_says_what_it_needs_instead_of_falling_back() {
        let url = parse_url("https://example.com/").expect("url");
        let result = fetch(
            &Request::new("GET", url, Vec::new()),
            Duration::from_secs(1),
        );
        match result {
            Err(NetError::Unsupported(message)) => assert!(message.contains("TLS"), "{message}"),
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }
}
