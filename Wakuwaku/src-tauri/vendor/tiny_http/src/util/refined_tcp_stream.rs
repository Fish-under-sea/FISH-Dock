use std::io::Result as IoResult;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr};
use std::sync::Arc;

use crate::connection::Connection;
#[cfg(any(feature = "ssl-openssl", feature = "ssl-rustls"))]
use crate::ssl::SslStream;

pub(crate) enum Stream {
    // Wakuwaku: the one socket, shared by the connection's two halves.
    Http(Arc<Connection>),
    #[cfg(any(feature = "ssl-openssl", feature = "ssl-rustls"))]
    Https(SslStream),
}

impl Stream {
    // Wakuwaku: the other half shares the socket. 0.12.0's Clone took a
    // second handle to it (TcpStream::try_clone, WSADuplicateSocket on
    // Windows) and unwrapped it: a process that may not open itself for
    // DUP_HANDLE (one started from an elevated one with another parent, as
    // her own copy out of a job) and some network filters have that answer
    // WSAEINVAL on every connection, and the panic took the whole program
    // down. Still a Result, for a split that fails (fail_next_split).
    fn try_clone(&self) -> IoResult<Self> {
        match self {
            Stream::Http(tcp_stream) => Ok(Stream::Http(Arc::clone(tcp_stream))),
            #[cfg(any(feature = "ssl-openssl", feature = "ssl-rustls"))]
            Stream::Https(ssl_stream) => Ok(Stream::Https(ssl_stream.clone())),
        }
    }
}

impl From<Connection> for Stream {
    fn from(tcp_stream: Connection) -> Self {
        Stream::Http(Arc::new(tcp_stream))
    }
}

impl Stream {
    fn secure(&self) -> bool {
        match self {
            Stream::Http(_) => false,
            #[cfg(any(feature = "ssl-openssl", feature = "ssl-rustls"))]
            Stream::Https(_) => true,
        }
    }

    fn peer_addr(&mut self) -> IoResult<Option<SocketAddr>> {
        match self {
            Stream::Http(tcp_stream) => tcp_stream.peer_addr(),
            #[cfg(any(feature = "ssl-openssl", feature = "ssl-rustls"))]
            Stream::Https(ssl_stream) => ssl_stream.peer_addr(),
        }
    }

    fn shutdown(&mut self, how: Shutdown) -> IoResult<()> {
        match self {
            Stream::Http(tcp_stream) => tcp_stream.shutdown(how),
            #[cfg(any(feature = "ssl-openssl", feature = "ssl-rustls"))]
            Stream::Https(ssl_stream) => ssl_stream.shutdown(how),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        match self {
            Stream::Http(tcp_stream) => (&mut &**tcp_stream).read(buf),
            #[cfg(any(feature = "ssl-openssl", feature = "ssl-rustls"))]
            Stream::Https(ssl_stream) => ssl_stream.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        match self {
            Stream::Http(tcp_stream) => (&mut &**tcp_stream).write(buf),
            #[cfg(any(feature = "ssl-openssl", feature = "ssl-rustls"))]
            Stream::Https(ssl_stream) => ssl_stream.write(buf),
        }
    }

    fn flush(&mut self) -> IoResult<()> {
        match self {
            Stream::Http(tcp_stream) => (&mut &**tcp_stream).flush(),
            #[cfg(any(feature = "ssl-openssl", feature = "ssl-rustls"))]
            Stream::Https(ssl_stream) => ssl_stream.flush(),
        }
    }
}

pub struct RefinedTcpStream {
    stream: Stream,
    close_read: bool,
    close_write: bool,
}

impl RefinedTcpStream {
    // Wakuwaku: an Err when the stream cannot be split in two (only a test's
    // now, fail_next_split); the stream is then closed, and the caller goes
    // on without that one connection.
    pub(crate) fn new<S>(stream: S) -> IoResult<(RefinedTcpStream, RefinedTcpStream)>
    where
        S: Into<Stream>,
    {
        let stream: Stream = stream.into();

        let (read, write) = (stream.try_clone()?, stream);

        let read = RefinedTcpStream {
            stream: read,
            close_read: true,
            close_write: false,
        };

        let write = RefinedTcpStream {
            stream: write,
            close_read: false,
            close_write: true,
        };

        Ok((read, write))
    }

    /// Returns true if this struct wraps around a secure connection.
    #[inline]
    pub(crate) fn secure(&self) -> bool {
        self.stream.secure()
    }

    pub(crate) fn peer_addr(&mut self) -> IoResult<Option<SocketAddr>> {
        self.stream.peer_addr()
    }
}

impl Drop for RefinedTcpStream {
    fn drop(&mut self) {
        if self.close_read {
            self.stream.shutdown(Shutdown::Read).ok();
        }

        if self.close_write {
            self.stream.shutdown(Shutdown::Write).ok();
        }
    }
}

impl Read for RefinedTcpStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        self.stream.read(buf)
    }
}

impl Write for RefinedTcpStream {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> IoResult<()> {
        self.stream.flush()
    }
}
