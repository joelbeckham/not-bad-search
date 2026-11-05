//! Bounded, local-only file previews. No database or network service is involved.
use anyhow::{bail, Context, Result};
use std::{
    fs::File,
    io::{Cursor, Read},
    ops::Range,
    path::Path,
};

pub const TEXT_LIMIT: usize = 2 * 1024 * 1024;
const IMAGE_LIMIT: usize = 32 * 1024 * 1024;

pub enum Content {
    Image {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
        original: (u32, u32),
    },
    Text {
        lines: Vec<Line>,
        truncated: bool,
        first_match: usize,
    },
}

pub struct Line {
    pub text: String,
    pub matches: Vec<Range<usize>>,
}

pub fn load(path: &Path, query: &str) -> Result<Content> {
    let file = File::open(path).context("Could not open this file")?;
    if !file.metadata()?.is_file() {
        bail!("Preview is available for files only");
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let is_image = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp");
    let limit = if is_image { IMAGE_LIMIT } else { TEXT_LIMIT };
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    let mut truncated = bytes.len() > limit;
    if is_image {
        if truncated {
            bail!("Image exceeds the 32 MiB preview limit. Open it in its default app.");
        }
        let reader = image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
        let original = reader.into_dimensions()?;
        if u64::from(original.0) * u64::from(original.1) > 40_000_000 {
            bail!("Image exceeds the 40 megapixel preview limit. Open it in its default app.");
        }
        let mut reader = image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let decoded = reader.decode()?.thumbnail(1600, 1600).to_rgba8();
        return Ok(Content::Image {
            width: decoded.width() as usize,
            height: decoded.height() as usize,
            rgba: decoded.into_raw(),
            original,
        });
    }
    bytes.truncate(limit);
    // A bounded read can split the final UTF-8 character; only that incomplete tail is discarded.
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(e) if truncated && e.error_len().is_none() => {
            std::str::from_utf8(&bytes[..e.valid_up_to()])?
        }
        Err(_) => {
            bail!("No text preview for this file type. Press Enter to open it in its default app.")
        }
    };
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        bail!("No text preview for this binary file. Press Enter to open it in its default app.");
    }
    let terms = crate::query::terms(query);
    let lines: Vec<_> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .take(20_001)
        .map(|text| {
            let mut end = text.len().min(8192);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            truncated |= end < text.len();
            let text = text[..end].replace('\t', "    ");
            let matches = matches(&text, &terms);
            Line { text, matches }
        })
        .collect();
    let mut lines = lines;
    truncated |= lines.len() > 20_000;
    lines.truncate(20_000);
    let first_match = lines
        .iter()
        .position(|line| !line.matches.is_empty())
        .unwrap_or(0);
    Ok(Content::Text {
        lines,
        truncated,
        first_match,
    })
}

fn matches(text: &str, terms: &[String]) -> Vec<Range<usize>> {
    let mut folded = String::new();
    let mut offsets = Vec::new();
    for (start, ch) in text.char_indices() {
        for lower in ch.to_lowercase() {
            folded.push(lower);
            offsets.extend(std::iter::repeat_n(
                (start, start + ch.len_utf8()),
                lower.len_utf8(),
            ));
        }
    }
    let mut ranges = Vec::new();
    for term in terms {
        for (start, matched) in folded.match_indices(term) {
            ranges.push(offsets[start].0..offsets[start + matched.len() - 1].1);
        }
    }
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut().filter(|last| last.end >= range.start) {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}
