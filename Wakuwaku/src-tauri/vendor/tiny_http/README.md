# tiny_http 0.12.0, patched for Wakuwaku

This is [tiny_http](https://github.com/tiny-http/tiny-http) 0.12.0 as published on
crates.io (git 212b1c45852f, its `src/`, `Cargo.toml` and licence files; the
tests, benches and examples left out), put in place of the published crate by
`[patch.crates-io]` in `src-tauri/Cargo.toml`. It is MIT OR Apache-2.0, as
upstream is (`LICENSE-MIT`, `LICENSE-APACHE`).

Her release build has `panic = "abort"`, so a panic on any of tiny_http's
threads ends her whole. 0.12.0 panics on a connection's failures that are the
client's or the network's, not hers: a crash report from a test VM (a copy
started elevated) said `called Result::unwrap() on an Err value: Os { code:
10022, kind: InvalidInput }` at `src/util/refined_tcp_stream.rs:18`, the
socket's second handle refused by WSADuplicateSocket, as some network filters
(VPN or antivirus layers) make it, and as it does on every connection in a
process that may not open itself for DUP_HANDLE: hers when an elevated one
started her and she started herself again out of its job with Explorer as
the parent (the new process's security is the elevated starter's, its token
Explorer's). 0.12.0 is the latest release, and upstream's master (2023-05)
has the same unwraps (tiny-http/tiny-http#195, open since 2021; #285, open).

What is changed, each marked `Wakuwaku:` in the code:

- `src/util/refined_tcp_stream.rs`, `src/connection.rs`: a connection's two
  halves share its one socket (`Arc<Connection>`, read and written through
  `&Connection` as std's `&TcpStream` is) instead of each holding a duplicate
  (`try_clone`, which `Stream`'s `Clone` unwrapped), so no second handle is
  asked for. The split is still a `try_clone` returning a Result, and
  `RefinedTcpStream::new` returns one, for a test's failed split.
- `src/lib.rs`, the accept loop: a connection that cannot be split is closed
  and the next one waited for; a connection reset or aborted before it was
  accepted (WSAECONNRESET, ECONNABORTED) is let go the same way, where 0.12.0
  ended the accept thread, and the server with it. Other accept errors end it
  as before.
- `src/lib.rs`: `tell_failures(f)` has `f` called with a line for each
  connection dropped so, and when the accept thread ends on an error (which
  0.12.0 only gave to `log::error!`). Her `server.rs` writes each kind once
  into her log. It is not `log::warn!`, for `log` needs a logger set, and a
  logger keeps every crate's log calls in the program: some 130 KB more in
  her exe when tried.
- `src/client.rs`: a client whose address could not be had when it connected
  (gone already, WSAENOTCONN) ends that connection instead of panicking at
  `self.remote_addr.as_ref().unwrap()`, as #285 mends it.
- `Cargo.toml`, `src/lib.rs`: a feature `test-failed-split`, which her tests
  alone turn on (her `[dev-dependencies]`), gives `Server::fail_next_split`,
  so a test can make the next connection's split fail; her builds never have
  it.

- `src/lib.rs`: `#![allow(unused_imports, dead_code)]`, for 0.12.0's own
  warnings, which a path dependency shows on every build of hers.

Nothing else differs from 0.12.0. Should a tiny_http release mend these, the
patch can go and `tiny_http` be that version.
