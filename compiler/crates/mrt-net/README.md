# MRT-Net

An HTTP/1.1 client, hand-written, with no dependencies.

It is an **engine extension**, not part of the language: only the Rust engines
have it, a program using it is not portable, and it is deliberately outside the
conformance corpus. See "Engine extensions" in `docs/LANGUAGE_SPEC.md`.

## The split

`lib.rs` is the protocol and nothing else — parsing a URL, building a request,
reading a status line, decoding a chunked body. Bytes in, bytes out, no socket
anywhere near it, so most of this crate's tests are arithmetic and run on a
machine with no network at all.

`client.rs` is the part that connects, and it needs nothing but `std::net`.
Its tests bind a listener on 127.0.0.1 and talk to themselves, which is
hermetic in the same way the rest of the suite is: nothing leaves the machine,
and CI needs no more than a loopback interface.

This is the split `mrt-audio` and `mrt-speaker` already make, with one
difference worth stating: there the hardware half needs `cpal`, and here the
socket half needs nothing. TCP is in the standard library.

## Why there is no https

Plain HTTP is free. TLS is not: it means `rustls` and the tree underneath it,
or platform FFI, and this workspace's manifest says in its first line that it
has no dependencies. Adding one is a decision about what this project is,
which belongs to whoever is making it and not to a crate that wanted to fetch
a URL.

So `https://` is refused by name, and the message says TLS is what is missing.
The alternative — quietly fetching over plain HTTP instead — would hand a
program a response it believes was protected. That is worse than not fetching.

When the decision is made, the shape is already here: `client.rs` is the only
file that touches a socket, and a TLS stream substitutes for a `TcpStream`
behind the same two calls.

## Connection: close

Every request sends it, and that is what makes the reader simple. Persistent
connections mean tracking where one response ends and the next begins, inside
a client that has no connection pool to reuse them with. One request per
connection, and end of stream means end of body.

## What is not here

No server, no sockets exposed to the language, no async, no redirect
following, no cookie jar. Each is a real feature and none is needed to fetch a
URL; a redirect is visible to the caller as a 30x and a `location` header,
which is a fact about the response rather than something to hide.
