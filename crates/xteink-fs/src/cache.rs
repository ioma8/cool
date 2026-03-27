use core::fmt::Write;

use heapless::String;

pub const CACHE_VERSION: u8 = 1;
pub const META_FILE_NAME: &str = "meta.txt";
pub const CONTENT_FILE_NAME: &str = "content.txt";
pub const PROGRESS_FILE_NAME: &str = "progress.bin";
pub const CACHE_ROOT_DIR: &str = "/.cool";

const PATH_CAPACITY: usize = 220;
const NAME_CAPACITY: usize = 64;
const MAX_COMPONENT_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheMeta {
    pub version: u8,
    pub source_size: u32,
    pub content_length: u32,
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
    let mut previous_sep = false;
    let mut hasher = FnvHasher::new();

    for &byte in input.as_bytes() {
        hasher.write(byte.to_ascii_uppercase());
        let mut ch = byte.to_ascii_uppercase();
        let is_sep = matches!(ch, b'/' | b'\\');

        if is_sep {
            ch = b'_';
        }

        if !ch.is_ascii_alphanumeric()
            && ch != b'.'
            && ch != b'_'
            && ch != b'-'
        {
            ch = b'_';
        }

        if previous_sep && ch == b'_' {
            continue;
        }
        previous_sep = ch == b'_';
        let _ = out.push(char::from(ch));
        if out.is_full() {
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
        let hash = hasher.finish();
        let limit = MAX_COMPONENT_LEN.saturating_sub(1 + 8);
        if limit > 0 {
            let prefix = &out.as_str()[..limit];
            let mut next = String::<NAME_CAPACITY>::new();
            let _ = next.push_str(prefix);
            let _ = next.push('_');
            let _ = write!(&mut next, "{hash:08X}");
            out = next;
        } else {
            break;
        }
    }

    out
}

pub fn cache_paths_for_epub(source_path: &str, entry_name: &str) -> CachePaths {
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
    let _ = directory.push_str(CACHE_ROOT_DIR);
    let _ = directory.push('/');
    let _ = directory.push_str(name.as_str());

    let mut meta = String::<PATH_CAPACITY>::new();
    let _ = meta.push_str(directory.as_str());
    let _ = meta.push('/');
    let _ = meta.push_str(META_FILE_NAME);

    let mut content = String::<PATH_CAPACITY>::new();
    let _ = content.push_str(directory.as_str());
    let _ = content.push('/');
    let _ = content.push_str(CONTENT_FILE_NAME);

    let mut progress = String::<PATH_CAPACITY>::new();
    let _ = progress.push_str(directory.as_str());
    let _ = progress.push('/');
    let _ = progress.push_str(PROGRESS_FILE_NAME);

    CachePaths {
        directory,
        meta,
        content,
        progress,
    }
}

pub fn serialize_meta(meta: &CacheMeta, source_size: u32) -> String<256> {
    let mut output = String::<256>::new();
    let _ = write!(
        &mut output,
        "version={}\nsource_size={}\ncontent_length={}\nsource_len={}\n",
        meta.version,
        meta.source_size,
        meta.content_length,
        source_size
    );
    output
}

pub fn parse_meta(raw: &str) -> Option<CacheMeta> {
    let mut parsed = CacheMeta {
        version: 0,
        source_size: 0,
        content_length: 0,
    };
    let mut seen = [false; 3];

    for line in raw.split('\n') {
        if let Some(value) = line.strip_prefix("version=") {
            parsed.version = value.parse().ok()?;
            seen[0] = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("source_size=") {
            let _ = value.parse::<u32>().map(|value| {
                parsed.source_size = value;
                seen[1] = true;
            })?;
            continue;
        }
        if let Some(value) = line.strip_prefix("content_length=") {
            let _ = value.parse::<u32>().map(|value| {
                parsed.content_length = value;
                seen[2] = true;
            })?;
            continue;
        }
    }

    if !seen[0] || !seen[1] || !seen[2] {
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
    fn sanitize_cache_name_collapses_repeated_unsupported_chars() {
        let name = sanitize_cache_name("/MYBOOKS/WHEN_I WRITE/BOOK.EPU");
        assert_eq!(name.as_str(), "MYBOOKS_WHEN_I_WRITE_BOOK.EPU");
    }

    #[test]
    fn paths_are_rooted_under_cool_dir() {
        let paths = cache_paths_for_epub("/MYBOOKS", "PET.JA~1.EPU");
        assert_eq!(paths.directory.as_str(), "/.cool/MYBOOKS_PET.JA~1.EPU");
        assert_eq!(paths.meta.as_str(), "/.cool/MYBOOKS_PET.JA~1.EPU/meta.txt");
        assert_eq!(paths.content.as_str(), "/.cool/MYBOOKS_PET.JA~1.EPU/content.txt");
    }

    #[test]
    fn serialize_and_parse_meta_roundtrip() {
        let meta = CacheMeta {
            version: CACHE_VERSION,
            source_size: 12345,
            content_length: 4096,
        };
        let raw = serialize_meta(&meta, 12345);
        let parsed = parse_meta(raw.as_str()).expect("meta should parse");
        assert_eq!(parsed, CacheMeta {
            version: CACHE_VERSION,
            source_size: 12345,
            content_length: 4096,
        });
    }

    #[test]
    fn parse_meta_rejects_invalid_version() {
        let raw = "version=2\nsource_size=1\ncontent_length=2\n";
        assert!(parse_meta(raw).is_none());
    }
}
