//! Parsing conflict markers and preserving the merged file's bytes and line endings.

/// Conflict markers longer than git's usual seven, so a line of `=======` in a file is not
/// mistaken for one; longer still when a version has a line that starts like the marker.
const MARKER: usize = 13;

/// A chunk with its lines as they are in the file, as bytes with line endings included: the
/// file need not be UTF-8, and lines nobody picked must be written back unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RawChunk {
    Same(Vec<Vec<u8>>),
    Conflict {
        ours: Vec<Vec<u8>>,
        base: Vec<Vec<u8>>,
        theirs: Vec<Vec<u8>>,
    },
}

/// Whether most lines of the file end in CRLF, so typed lines do too.
/// Whether the merged file ends without a line break. Inside a conflict merge-file adds the
/// break before its marker, so the versions themselves tell.
pub(super) fn ends_open(chunks: &[RawChunk], ours: &[u8], theirs: &[u8]) -> bool {
    match chunks.last() {
        Some(RawChunk::Same(lines)) => lines.last().is_some_and(|l| !l.ends_with(b"\n")),
        Some(RawChunk::Conflict { .. }) => [ours, theirs]
            .iter()
            .any(|blob| !blob.is_empty() && !blob.ends_with(b"\n")),
        None => false,
    }
}

pub(super) fn crlf(chunks: &[RawChunk]) -> bool {
    let (mut crlf, mut lf) = (0usize, 0usize);
    let mut count = |lines: &[Vec<u8>]| {
        for line in lines {
            if line.ends_with(b"\r\n") {
                crlf += 1;
            } else if line.ends_with(b"\n") {
                lf += 1;
            }
        }
    };
    for chunk in chunks {
        match chunk {
            RawChunk::Same(lines) => count(lines),
            RawChunk::Conflict { ours, theirs, .. } => {
                count(ours);
                count(theirs);
            }
        }
    }
    crlf > lf
}

/// A marker longer than any run of `<`, `|`, `=` or `>` a line of the versions starts with, so
/// no line of the file reads as a marker.
pub(super) fn marker_size(versions: &[&[u8]]) -> usize {
    let longest = versions
        .iter()
        .flat_map(|blob| blob.split(|&c| c == b'\n'))
        .filter_map(|line| {
            let first = *line.first()?;
            matches!(first, b'<' | b'|' | b'=' | b'>').then(|| line.iter().take_while(|&&c| c == first).count())
        })
        .max()
        .unwrap_or(0);
    MARKER.max(longest + 1)
}

/// Split `git merge-file --diff3` output, marked with `size`-long markers, into agreed lines
/// and conflicts; `None` when a conflict doesn't end, rather than lose the lines after it.
pub(super) fn parse_merged(text: &[u8], size: usize) -> Option<Vec<RawChunk>> {
    enum In {
        Same,
        Ours,
        Base,
        Theirs,
    }
    let marker = |c: u8| vec![c; size];
    let (start, mid_base, mid, end) = (marker(b'<'), marker(b'|'), marker(b'='), marker(b'>'));
    let mut chunks = Vec::new();
    let mut same = Vec::new();
    let (mut ours, mut base, mut theirs) = (Vec::new(), Vec::new(), Vec::new());
    let mut state = In::Same;
    for line in text.split_inclusive(|&c| c == b'\n') {
        let bare = trim_eol(line);
        let is = |m: &[u8]| bare == m || bare.strip_prefix(m).is_some_and(|rest| rest.starts_with(b" "));
        match state {
            In::Same if is(&start) => {
                if !same.is_empty() {
                    chunks.push(RawChunk::Same(std::mem::take(&mut same)));
                }
                state = In::Ours;
            }
            In::Same => same.push(line.to_vec()),
            In::Ours if is(&mid_base) => state = In::Base,
            In::Ours | In::Base if bare == mid.as_slice() => state = In::Theirs,
            In::Ours => ours.push(line.to_vec()),
            In::Base => base.push(line.to_vec()),
            In::Theirs if is(&end) => {
                chunks.push(RawChunk::Conflict {
                    ours: std::mem::take(&mut ours),
                    base: std::mem::take(&mut base),
                    theirs: std::mem::take(&mut theirs),
                });
                state = In::Same;
            }
            In::Theirs => theirs.push(line.to_vec()),
        }
    }
    if !matches!(state, In::Same) {
        return None;
    }
    if !same.is_empty() {
        chunks.push(RawChunk::Same(same));
    }
    Some(chunks)
}

/// Lines for showing, without line endings. Bytes that aren't UTF-8 show as `�`; the file
/// itself is written from the bytes.
pub(super) fn bare(lines: Vec<Vec<u8>>) -> Vec<String> {
    lines
        .iter()
        .map(|l| String::from_utf8_lossy(trim_eol(l)).into_owned())
        .collect()
}

fn trim_eol(mut line: &[u8]) -> &[u8] {
    while let [rest @ .., b'\n' | b'\r'] = line {
        line = rest;
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_text_splits_into_agreed_lines_and_conflicts() {
        let m = |c: char| c.to_string().repeat(MARKER);
        let text = format!(
            "a\n{} ours\nours 1\nours 2\n{} base\nold\n{}\ntheirs\n{} theirs\nz\n=======\n",
            m('<'),
            m('|'),
            m('='),
            m('>')
        );
        let chunks = parse_merged(text.as_bytes(), MARKER).unwrap();
        let l = |s: &str| s.as_bytes().to_vec();
        assert_eq!(
            chunks,
            [
                RawChunk::Same(vec![l("a\n")]),
                RawChunk::Conflict {
                    ours: vec![l("ours 1\n"), l("ours 2\n")],
                    base: vec![l("old\n")],
                    theirs: vec![l("theirs\n")],
                },
                // A line of seven `=` in the file is just a line.
                RawChunk::Same(vec![l("z\n"), l("=======\n")]),
            ]
        );
        // A conflict that never ends is not cut off silently.
        assert_eq!(
            parse_merged(format!("a\n{} ours\nb\n", m('<')).as_bytes(), MARKER),
            None
        );
    }

    #[test]
    fn markers_are_longer_than_any_line_that_looks_like_one() {
        assert_eq!(marker_size(&[b"a\n", b"=======\n"]), MARKER);
        let long = format!("x\n{} example\n", "<".repeat(MARKER));
        assert_eq!(marker_size(&[b"a\n", long.as_bytes(), b""]), MARKER + 1);
        assert_eq!(marker_size(&[">".repeat(20).as_bytes()]), 21);
    }
}
