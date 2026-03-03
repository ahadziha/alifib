use std::fmt;

pub type Source = String;

pub fn unknown_source() -> Source {
    "<unknown>".to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    pub source: Source,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
    pub bol_offset: usize,
}

impl Point {
    pub fn new(source: Source, offset: usize, line: usize, column: usize, bol_offset: usize) -> Self {
        assert!(line >= 1, "line must be positive");
        assert!(column >= 1, "column must be positive");
        Self { source, offset, line, column, bol_offset }
    }

    pub fn unknown() -> Self {
        Self {
            source: unknown_source(),
            offset: 0,
            line: 1,
            column: 1,
            bol_offset: 0,
        }
    }

}

impl PartialOrd for Point {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Point {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.source.cmp(&other.source)
            .then_with(|| self.offset.cmp(&other.offset))
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{} (offset {})", self.source, self.line, self.column, self.offset)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: Point,
    pub stop: Point,
}

impl Span {
    pub fn new(start: Point, stop: Point) -> Self {
        assert_eq!(start.source, stop.source, "span points must be from same source");
        let (start, stop) = if start <= stop { (start, stop) } else { (stop, start) };
        Self { start, stop }
    }

    pub fn point(p: Point) -> Self {
        Self { start: p.clone(), stop: p }
    }

    pub fn unknown() -> Self {
        Self::point(Point::unknown())
    }

    pub fn length(&self) -> usize {
        self.stop.offset.saturating_sub(self.start.offset)
    }

    pub fn is_point(&self) -> bool {
        self.length() == 0
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_point() {
            write!(f, "{}", self.start)
        } else {
            write!(f, "{}-{}", self.start, self.stop)
        }
    }
}

/// Build a point from a chumsky byte offset in a source string
pub fn point_from_offset(source: &str, source_name: &str, offset: usize) -> Point {
    let mut line = 1usize;
    let mut col = 1usize;
    let mut bol_offset = 0usize;
    for (i, ch) in source.char_indices() {
        if i == offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
            bol_offset = i + 1;
        } else {
            col += 1;
        }
    }
    Point::new(source_name.to_owned(), offset, line, col, bol_offset)
}

pub fn span_from_range(source: &str, source_name: &str, range: std::ops::Range<usize>) -> Span {
    let start = point_from_offset(source, source_name, range.start);
    let stop = point_from_offset(source, source_name, range.end);
    Span::new(start, stop)
}
