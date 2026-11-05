//! Bounded HTTP/1.1 framing for the server and the one-shot client. Every
//! message closes its connection, so a reader may buffer past the head.

use std::fmt;
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// Framing bounds, and the server's socket deadline.
#[derive(Clone, Copy)]
pub struct Limits {
    /// The deadline for a whole request head, then for each later socket read
    /// or write. It never bounds waiting for or running the kernel.
    pub timeout: Duration,
    /// The largest head, in bytes.
    pub head: usize,
    /// The largest body, in bytes.
    pub body: usize,
}

impl Limits {
    pub const DEFAULT: Self = Self {
        timeout: Duration::from_secs(10),
        head: 64 * 1024,
        body: 64 * 1024 * 1024,
    };
}

/// The statuses the server answers with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ok,
    NoContent,
    BadRequest,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    RequestTimeout,
    ContentTooLarge,
    HeaderFieldsTooLarge,
    InternalServerError,
}

impl Status {
    const fn line(self) -> (u16, &'static str) {
        match self {
            Self::Ok => (200, "OK"),
            Self::NoContent => (204, "No Content"),
            Self::BadRequest => (400, "Bad Request"),
            Self::Forbidden => (403, "Forbidden"),
            Self::NotFound => (404, "Not Found"),
            Self::MethodNotAllowed => (405, "Method Not Allowed"),
            Self::RequestTimeout => (408, "Request Timeout"),
            Self::ContentTooLarge => (413, "Content Too Large"),
            Self::HeaderFieldsTooLarge => (431, "Request Header Fields Too Large"),
            Self::InternalServerError => (500, "Internal Server Error"),
        }
    }
}

/// Why a message could not be read.
#[derive(Debug)]
pub enum ReadError {
    /// No byte arrived before the peer closed, reset, or the deadline passed.
    Idle,
    /// The peer reset the connection mid-message.
    Reset,
    /// The deadline passed mid-message.
    TimedOut,
    HeadTooLarge,
    BodyTooLarge,
    /// The bytes do not frame a supported message.
    Malformed(&'static str),
    Io(io::Error),
}

const INVALID: ReadError = ReadError::Malformed("invalid HTTP message");
const INCOMPLETE: &str = "incomplete HTTP message";

impl ReadError {
    /// Classify a failed read, given whether any byte of the message arrived.
    fn from_io(error: io::Error, started: bool) -> Self {
        match error.kind() {
            kind if disconnected(kind) => Self::Reset,
            ErrorKind::WouldBlock | ErrorKind::TimedOut if started => Self::TimedOut,
            ErrorKind::WouldBlock | ErrorKind::TimedOut => Self::Idle,
            ErrorKind::UnexpectedEof => Self::Malformed(INCOMPLETE),
            _ => Self::Io(error),
        }
    }
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => f.write_str("the connection ended before any message"),
            Self::Reset => f.write_str("the connection was reset"),
            Self::TimedOut => f.write_str("the message did not arrive in time"),
            Self::HeadTooLarge => f.write_str("message head is too large"),
            Self::BodyTooLarge => f.write_str("message body is too large"),
            Self::Malformed(message) => f.write_str(message),
            Self::Io(error) => error.fmt(f),
        }
    }
}

/// Whether an I/O failure only means that the peer has gone.
pub fn disconnected(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
    )
}

/// Header fields in arrival order, looked up case-insensitively.
struct Headers(Vec<(String, String)>);

impl Headers {
    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// A request as the server reads it: a bounded body, a decoded path, a Host.
pub struct Request {
    pub method: String,
    /// The target's path, percent-decoded once, after its query was removed.
    pub path: String,
    pub host: String,
    headers: Headers,
    pub body: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }
}

/// A response as the client reads it.
pub struct Response {
    pub status: u16,
    /// The whole status line, for reporting.
    pub line: String,
    pub body: String,
}

/// A socket whose reads share one deadline while `until` is set.
struct Deadline<'a> {
    stream: &'a TcpStream,
    until: Option<Instant>,
}

impl Read for Deadline<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(until) = self.until {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(ErrorKind::TimedOut.into());
            }
            self.stream.set_read_timeout(Some(left))?;
        }
        (&mut self.stream).read(buf)
    }
}

/// Read a request. Its head must arrive whole within `limits.timeout`; each
/// body read then has that long. Every bound and the framing are checked
/// before the body is allocated.
pub fn read_request(stream: &TcpStream, limits: Limits) -> Result<Request, ReadError> {
    let mut reader = BufReader::new(Deadline {
        stream,
        until: Some(Instant::now() + limits.timeout),
    });
    let head = read_head(&mut reader, limits.head)?;
    let (start, headers) = parse_head(&head)?;
    let (method, target, version) = three(start).ok_or(INVALID)?;
    if !is_token(method)
        || !target.starts_with('/')
        || target.bytes().any(|byte| byte.is_ascii_control())
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
    {
        return Err(INVALID);
    }
    let path = decode_path(target)?;
    let host = headers
        .get("host")
        .filter(|host| !host.is_empty())
        .ok_or(INVALID)?
        .to_owned();
    let length = match content_length(&headers, limits.body)? {
        Some(length) => length,
        None if method == "POST" => return Err(INVALID),
        None => 0,
    };
    reader.get_mut().until = None;
    stream
        .set_read_timeout(Some(limits.timeout))
        .map_err(ReadError::Io)?;
    let body = read_body(&mut reader, length)?;
    Ok(Request {
        method: method.to_owned(),
        path,
        host,
        headers,
        body,
    })
}

/// Read a response under the default bounds, with no deadline: a command may
/// compute for as long as it needs. Without Content-Length the body runs to
/// the connection's close.
pub fn read_response(stream: &TcpStream) -> Result<Response, ReadError> {
    let limits = Limits::DEFAULT;
    let mut reader = BufReader::new(stream);
    let head = read_head(&mut reader, limits.head)?;
    let (start, headers) = parse_head(&head)?;
    let (version, code, reason) = three(start).ok_or(INVALID)?;
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || code.len() != 3
        || !code.bytes().all(|byte| byte.is_ascii_digit())
        || reason.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(INVALID);
    }
    let status = code.parse().map_err(|_| INVALID)?;
    let body = match content_length(&headers, limits.body)? {
        Some(length) => read_body(&mut reader, length)?,
        None => read_to_close(&mut reader, limits.body)?,
    };
    Ok(Response {
        status,
        line: start.to_owned(),
        body,
    })
}

/// Read up to the blank line ending a head, returning the head without it.
fn read_head(reader: &mut impl BufRead, limit: usize) -> Result<String, ReadError> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let room = limit - head.len();
        if room == 0 {
            return Err(ReadError::HeadTooLarge);
        }
        let read = reader
            .by_ref()
            .take(room as u64)
            .read_until(b'\n', &mut head)
            .map_err(|error| ReadError::from_io(error, !head.is_empty()))?;
        if read == 0 {
            return Err(if head.is_empty() {
                ReadError::Idle
            } else {
                ReadError::Malformed(INCOMPLETE)
            });
        }
    }
    head.truncate(head.len() - 4);
    String::from_utf8(head).map_err(|_| INVALID)
}

/// Split a head into its start line and header fields. Duplicated framing
/// fields are ambiguous, and transfer codings are unsupported.
fn parse_head(head: &str) -> Result<(&str, Headers), ReadError> {
    const SINGLE: [&str; 5] = [
        "host",
        "content-length",
        "transfer-encoding",
        "content-type",
        "origin",
    ];
    let (start, fields) = head.split_once("\r\n").unwrap_or((head, ""));
    let mut headers = Headers(Vec::new());
    for field in fields.split_terminator("\r\n") {
        let (name, value) = field.split_once(':').ok_or(INVALID)?;
        if !is_token(name)
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() && byte != b'\t')
            || (SINGLE
                .iter()
                .any(|single| single.eq_ignore_ascii_case(name))
                && headers.get(name).is_some())
        {
            return Err(INVALID);
        }
        headers
            .0
            .push((name.to_owned(), value.trim_matches([' ', '\t']).to_owned()));
    }
    if headers.get("transfer-encoding").is_some() {
        return Err(INVALID);
    }
    Ok((start, headers))
}

/// A start line's three space-separated parts; the last may contain spaces.
fn three(line: &str) -> Option<(&str, &str, &str)> {
    let mut parts = line.splitn(3, ' ');
    Some((parts.next()?, parts.next()?, parts.next()?))
}

fn content_length(headers: &Headers, limit: usize) -> Result<Option<usize>, ReadError> {
    let Some(value) = headers.get("content-length") else {
        return Ok(None);
    };
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(INVALID);
    }
    match value.parse() {
        Ok(length) if length <= limit => Ok(Some(length)),
        _ => Err(ReadError::BodyTooLarge),
    }
}

fn read_body(reader: &mut impl Read, length: usize) -> Result<String, ReadError> {
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|error| ReadError::from_io(error, true))?;
    String::from_utf8(body).map_err(|_| INVALID)
}

fn read_to_close(reader: &mut impl Read, limit: usize) -> Result<String, ReadError> {
    let mut body = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| ReadError::from_io(error, true))?;
    if body.len() > limit {
        return Err(ReadError::BodyTooLarge);
    }
    String::from_utf8(body).map_err(|_| INVALID)
}

fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// Remove the query, then percent-decode the path exactly once.
fn decode_path(target: &str) -> Result<String, ReadError> {
    const INVALID_PATH: ReadError = ReadError::Malformed("invalid request path");
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    let mut decoded = Vec::with_capacity(path.len());
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let mut digit = || bytes.next().and_then(|byte| (byte as char).to_digit(16));
            let high = digit().ok_or(INVALID_PATH)?;
            let low = digit().ok_or(INVALID_PATH)?;
            decoded.push((high * 16 + low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).map_err(|_| INVALID_PATH)
}

/// Write a request that closes its connection, with a byte-counted body.
pub fn write_request(
    writer: impl Write,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> io::Result<()> {
    write_message(
        writer,
        &format!("{method} {target} HTTP/1.1"),
        headers,
        Some(body),
    )
}

/// Write a response that closes its connection. A 204 has no body framing,
/// so its body must be empty.
pub fn write_response(
    writer: impl Write,
    status: Status,
    headers: &[(&str, &str)],
    body: &str,
) -> io::Result<()> {
    let (code, reason) = status.line();
    let body = if status == Status::NoContent {
        debug_assert!(body.is_empty(), "a 204 response has no body");
        None
    } else {
        Some(body)
    };
    write_message(writer, &format!("HTTP/1.1 {code} {reason}"), headers, body)
}

fn write_message(
    mut writer: impl Write,
    start: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> io::Result<()> {
    let mut head = format!("{start}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("Connection: close\r\n\r\n");
    writer.write_all(head.as_bytes())?;
    writer.write_all(body.unwrap_or_default().as_bytes())?;
    writer.flush()
}
