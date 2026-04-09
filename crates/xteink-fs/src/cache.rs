use core::fmt::Write;

use heapless::String;

pub const CACHE_VERSION: u8 = 6;
pub const META_FILE_NAME: &str = "meta.txt";
pub const CONTENT_FILE_NAME: &str = "content.txt";
pub const PROGRESS_FILE_NAME: &str = "progress.bin";
pub const CACHE_ROOT_DIR: &str = "/.cool";

const PATH_CAPACITY: usize = 220;
const NAME_CAPACITY: usize = 64;
const MAX_COMPONENT_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheMeta {
    pub version: u8,
    pub source_size: u32,
    pub content_length: u32,
    pub cached_pages: u32,
    pub cached_progress_percent: u8,
    pub next_spine_index: u16,
    pub resume_page: u32,
    pub resume_cursor_y: u16,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachePaths {
    pub directory: String<PATH_CAPACITY>,
    pub meta: String<PATH_CAPACITY>,
    pub content: String<PATH_CAPACITY>,
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
    let progress = join_cache_file(directory.as_str(), PROGRESS_FILE_NAME);

    CachePaths {
        directory,
        meta,
        content,
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

pub fn serialize_meta(meta: &CacheMeta, source_size: u32) -> String<256> {
    let mut output = String::<256>::new();
    let _ = write!(
        &mut output,
        "version={}\nsource_size={}\ncontent_length={}\ncached_pages={}\ncached_progress_percent={}\nnext_spine_index={}\nresume_page={}\nresume_cursor_y={}\ncomplete={}\nsource_len={}\n",
        meta.version,
        meta.source_size,
        meta.content_length,
        meta.cached_pages,
        meta.cached_progress_percent,
        meta.next_spine_index,
        meta.resume_page,
        meta.resume_cursor_y,
        if meta.complete { 1 } else { 0 },
        source_size
    );
    output
}

pub fn parse_meta(raw: &str) -> Option<CacheMeta> {
    let mut parsed = CacheMeta {
        version: 0,
        source_size: 0,
        content_length: 0,
        cached_pages: 0,
        cached_progress_percent: 0,
        next_spine_index: 0,
        resume_page: 0,
        resume_cursor_y: 0,
        complete: false,
    };
    let mut seen = [false; 9];

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
            parsed.content_length = value.parse::<u32>().ok()?;
            seen[2] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("cached_pages=") {
            parsed.cached_pages = value.parse::<u32>().ok()?;
            seen[3] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("cached_progress_percent=") {
            parsed.cached_progress_percent = value.parse::<u8>().ok()?;
            seen[4] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("next_spine_index=") {
            parsed.next_spine_index = value.parse::<u16>().ok()?;
            seen[5] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("resume_page=") {
            parsed.resume_page = value.parse::<u32>().ok()?;
            seen[6] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("resume_cursor_y=") {
            parsed.resume_cursor_y = value.parse::<u16>().ok()?;
            seen[7] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("complete=") {
            parsed.complete = match value {
                "0" => false,
                "1" => true,
                _ => return None,
            };
            seen[8] = true;
            continue;
        }
    }

    if !seen.iter().all(|seen| *seen) {
        return None;
    }
    if parsed.version != CACHE_VERSION {
        return None;
    }

    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_cache_name_fits_component_limit() {
        let name = short_cache_name(0x11B3_D183);
        assert_eq!(name.len(), MAX_COMPONENT_LEN);
        assert_eq!(name.as_str(), "B1B3D183");
    }

    #[test]
    fn sanitize_cache_name_keeps_only_alnum_and_underscore() {
        let name = sanitize_cache_name("/MYBOOKS/WHEN_I WRITE/BOOK.EPU");
        assert_eq!(name.len(), 8);
        assert!(
            name.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        );
    }

    #[test]
    fn paths_are_rooted_under_cool_dir() {
        let paths = cache_paths_for_epub("/MYBOOKS", "PET.JA~1.EPU");
        let directory = paths.directory.as_str();
        let meta = paths.meta.as_str();
        let content = paths.content.as_str();
        let progress = paths.progress.as_str();

        assert!(directory.starts_with("/.cool/"));
        assert!(paths.directory.as_str().starts_with("/.cool/"));
        assert_eq!(directory.split('/').nth(2).unwrap().len(), 8);
        assert!(meta.starts_with(directory));
        assert!(meta.ends_with("/meta.txt"));
        assert!(content.starts_with(directory));
        assert!(content.ends_with("/content.txt"));
        assert!(progress.starts_with(directory));
        assert!(progress.ends_with("/progress.bin"));
    }

    #[test]
    fn cache_paths_stay_under_logical_dot_cool_root() {
        let paths = cache_paths_for_epub("/MYBOOKS", "WHEN_I~1.EPU");

        assert!(paths.directory.as_str().starts_with("/.cool/"));
        assert!(paths.meta.as_str().starts_with("/.cool/"));
        assert!(paths.content.as_str().starts_with("/.cool/"));
        assert!(paths.progress.as_str().starts_with("/.cool/"));
    }

    #[test]
    fn serialize_and_parse_meta_roundtrip() {
        let meta = CacheMeta {
            version: CACHE_VERSION,
            source_size: 12345,
            content_length: 4096,
            cached_pages: 3,
            cached_progress_percent: 42,
            next_spine_index: 7,
            resume_page: 2,
            resume_cursor_y: 412,
            complete: false,
        };
        let raw = serialize_meta(&meta, 12345);
        let parsed = parse_meta(raw.as_str()).expect("meta should parse");
        assert_eq!(
            parsed,
            CacheMeta {
                version: CACHE_VERSION,
                source_size: 12345,
                content_length: 4096,
                cached_pages: 3,
                cached_progress_percent: 42,
                next_spine_index: 7,
                resume_page: 2,
                resume_cursor_y: 412,
                complete: false,
            }
        );
    }

    #[test]
    fn parse_meta_rejects_invalid_version() {
        let raw = "version=2\nsource_size=1\ncontent_length=2\ncached_pages=1\ncached_progress_percent=1\nnext_spine_index=0\nresume_page=0\nresume_cursor_y=0\ncomplete=1\n";
        assert!(parse_meta(raw).is_none());
    }
}
