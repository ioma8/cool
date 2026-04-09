use core::fmt::Write;

use heapless::String;

pub const CACHE_VERSION: u8 = 7;
pub const META_FILE_NAME: &str = "meta.txt";
pub const CONTENT_FILE_NAME: &str = "content.txt";
pub const CHAPTERS_FILE_NAME: &str = "chapters.idx";
pub const PAGES_FILE_NAME: &str = "pages.idx";
pub const PROGRESS_FILE_NAME: &str = "progress.bin";
pub const CACHE_ROOT_DIR: &str = "/.cool";

const PATH_CAPACITY: usize = 220;
const NAME_CAPACITY: usize = 64;
const MAX_COMPONENT_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheMeta {
    pub version: u8,
    pub source_size: u32,
    pub content_length: u64,
    pub build_complete: bool,
    pub next_chapter_index: u16,
    pub layout_sig_version: u16,
    pub layout_sig_width: u16,
    pub layout_sig_height: u16,
    pub layout_sig_content_height: u16,
    pub layout_sig_font: u32,
    pub layout_sig_paginator: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressState {
    pub current_byte_offset: u64,
    pub current_page_hint: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachePaths {
    pub directory: String<PATH_CAPACITY>,
    pub meta: String<PATH_CAPACITY>,
    pub content: String<PATH_CAPACITY>,
    pub chapters: String<PATH_CAPACITY>,
    pub pages: String<PATH_CAPACITY>,
    pub progress: String<PATH_CAPACITY>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FnvHasher(u32);

impl FnvHasher {
    fn new() -> Self {
        Self(0x811C9DC5)
    }

    fn write(&mut self, byte: u8) {
        self.0 = self.0.wrapping_mul(0x0100_0193) ^ u32::from(byte);
    }

    fn finish(self) -> u32 {
        self.0
    }
}

pub fn sanitize_cache_name(input: &str) -> String<NAME_CAPACITY> {
    let mut out = String::new();
    let mut hasher = FnvHasher::new();

    for &byte in input.as_bytes() {
        hasher.write(byte.to_ascii_uppercase());
        let ch = byte.to_ascii_uppercase();
        let mapped = if ch.is_ascii_alphanumeric() { ch } else { b'_' };
        let _ = out.push(char::from(mapped));
        if out.len() == out.capacity() {
            break;
        }
    }

    while out.ends_with('_') {
        out.pop();
    }

    if out.is_empty() {
        let _ = out.push_str("COOL");
    }

    while out.len() > MAX_COMPONENT_LEN {
        out = short_cache_name(hasher.finish());
    }

    out
}

fn short_cache_name(hash: u32) -> String<NAME_CAPACITY> {
    let mut out = String::<NAME_CAPACITY>::new();
    let _ = out.push('B');
    let _ = write!(&mut out, "{:07X}", hash & 0x0FFF_FFFF);
    out
}

pub fn cache_paths_for_epub(source_path: &str, entry_name: &str) -> CachePaths {
    cache_paths_for_epub_with_root(source_path, entry_name, CACHE_ROOT_DIR)
}

fn cache_paths_for_epub_with_root(
    source_path: &str,
    entry_name: &str,
    root_dir: &str,
) -> CachePaths {
    let full_path = {
        let mut merged = String::<NAME_CAPACITY>::new();
        let _ = merged.push_str(source_path);
        if !source_path.ends_with('/') && !entry_name.starts_with('/') {
            let _ = merged.push('/');
        }
        let _ = merged.push_str(entry_name);
        merged
    };

    let name = sanitize_cache_name(full_path.as_str());
    let mut directory = String::<PATH_CAPACITY>::new();
    let _ = directory.push_str(root_dir);
    let _ = directory.push('/');
    let _ = directory.push_str(name.as_str());

    let meta = join_cache_file(directory.as_str(), META_FILE_NAME);
    let content = join_cache_file(directory.as_str(), CONTENT_FILE_NAME);
    let chapters = join_cache_file(directory.as_str(), CHAPTERS_FILE_NAME);
    let pages = join_cache_file(directory.as_str(), PAGES_FILE_NAME);
    let progress = join_cache_file(directory.as_str(), PROGRESS_FILE_NAME);

    CachePaths {
        directory,
        meta,
        content,
        chapters,
        pages,
        progress,
    }
}

fn join_cache_file(directory: &str, file_name: &str) -> String<PATH_CAPACITY> {
    let mut path = String::<PATH_CAPACITY>::new();
    let _ = path.push_str(directory);
    let _ = path.push('/');
    let _ = path.push_str(file_name);
    path
}

pub fn serialize_meta(meta: &CacheMeta) -> String<320> {
    let mut output = String::<320>::new();
    let _ = write!(
        &mut output,
        "version={}\nsource_size={}\ncontent_length={}\nbuild_complete={}\nnext_chapter_index={}\nlayout_sig_version={}\nlayout_sig_width={}\nlayout_sig_height={}\nlayout_sig_content_height={}\nlayout_sig_font={}\nlayout_sig_paginator={}\n",
        meta.version,
        meta.source_size,
        meta.content_length,
        if meta.build_complete { 1 } else { 0 },
        meta.next_chapter_index,
        meta.layout_sig_version,
        meta.layout_sig_width,
        meta.layout_sig_height,
        meta.layout_sig_content_height,
        meta.layout_sig_font,
        meta.layout_sig_paginator,
    );
    output
}

pub fn parse_meta(raw: &str) -> Option<CacheMeta> {
    let mut parsed = CacheMeta {
        version: 0,
        source_size: 0,
        content_length: 0,
        build_complete: false,
        next_chapter_index: 0,
        layout_sig_version: 0,
        layout_sig_width: 0,
        layout_sig_height: 0,
        layout_sig_content_height: 0,
        layout_sig_font: 0,
        layout_sig_paginator: 0,
    };
    let mut seen = [false; 11];

    for line in raw.split('\n') {
        if let Some(value) = line.strip_prefix("version=") {
            parsed.version = value.parse().ok()?;
            seen[0] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("source_size=") {
            parsed.source_size = value.parse::<u32>().ok()?;
            seen[1] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("content_length=") {
            parsed.content_length = value.parse::<u64>().ok()?;
            seen[2] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("build_complete=") {
            parsed.build_complete = match value {
                "0" => false,
                "1" => true,
                _ => return None,
            };
            seen[3] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("next_chapter_index=") {
            parsed.next_chapter_index = value.parse::<u16>().ok()?;
            seen[4] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("layout_sig_version=") {
            parsed.layout_sig_version = value.parse::<u16>().ok()?;
            seen[5] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("layout_sig_width=") {
            parsed.layout_sig_width = value.parse::<u16>().ok()?;
            seen[6] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("layout_sig_height=") {
            parsed.layout_sig_height = value.parse::<u16>().ok()?;
            seen[7] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("layout_sig_content_height=") {
            parsed.layout_sig_content_height = value.parse::<u16>().ok()?;
            seen[8] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("layout_sig_font=") {
            parsed.layout_sig_font = value.parse::<u32>().ok()?;
            seen[9] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("layout_sig_paginator=") {
            parsed.layout_sig_paginator = value.parse::<u32>().ok()?;
            seen[10] = true;
        }
    }

    if !seen.iter().all(|v| *v) || parsed.version != CACHE_VERSION {
        return None;
    }
    Some(parsed)
}

pub fn encode_progress(progress: ProgressState) -> [u8; 12] {
    let mut raw = [0u8; 12];
    raw[..8].copy_from_slice(&progress.current_byte_offset.to_le_bytes());
    raw[8..12].copy_from_slice(&progress.current_page_hint.to_le_bytes());
    raw
}

pub fn decode_progress(raw: &[u8]) -> Option<ProgressState> {
    if raw.len() < 12 {
        return None;
    }
    Some(ProgressState {
        current_byte_offset: u64::from_le_bytes(raw[..8].try_into().ok()?),
        current_page_hint: u32::from_le_bytes(raw[8..12].try_into().ok()?),
    })
}

pub fn encode_offset(offset: u64) -> [u8; 8] {
    offset.to_le_bytes()
}

pub fn decode_offset(raw: &[u8]) -> Option<u64> {
    if raw.len() < 8 {
        return None;
    }
    Some(u64::from_le_bytes(raw[..8].try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_meta() -> CacheMeta {
        CacheMeta {
            version: CACHE_VERSION,
            source_size: 100,
            content_length: 1234,
            build_complete: false,
            next_chapter_index: 3,
            layout_sig_version: 1,
            layout_sig_width: 600,
            layout_sig_height: 800,
            layout_sig_content_height: 748,
            layout_sig_font: 0xABCDEF,
            layout_sig_paginator: 0x1234,
        }
    }

    #[test]
    fn serialize_parse_meta_roundtrip() {
        let meta = sample_meta();
        let raw = serialize_meta(&meta);
        assert_eq!(parse_meta(raw.as_str()), Some(meta));
    }

    #[test]
    fn progress_roundtrip() {
        let progress = ProgressState {
            current_byte_offset: 4242,
            current_page_hint: 11,
        };
        assert_eq!(decode_progress(&encode_progress(progress)), Some(progress));
    }

    #[test]
    fn paths_include_sidecars() {
        let paths = cache_paths_for_epub("/MYBOOKS", "BOOK.EPUB");
        assert!(paths.chapters.ends_with(CHAPTERS_FILE_NAME));
        assert!(paths.pages.ends_with(PAGES_FILE_NAME));
    }
}
