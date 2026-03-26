# EPUB parser crate design (no_std, low-memory, firmware parser)

## 1. Goal and constraints
- Implement a firmware-ready EPUB parser in a new workspace crate `xteink-epub`.
- The parser runs in `no_std` firmware and supports future SD-card input via a storage-agnostic source abstraction.
- Initial behavior is text rendering support with lightweight markup preservation, not image rasterization.
- Priority: low memory usage and zero-copy where feasible; no unnecessary heap allocation in parser core.
- EPUB validation and regression will use files in `test/epubs`.

## 2. Scope for this phase
- Parse EPUB container (ZIP) with deflate-compressed and stored entries.
- Resolve `META-INF/container.xml` to find `content.opf`.
- Parse OPF manifest and spine in order.
- Parse XHTML chapter content for a markup-preserving event stream (`<p>`, headings, `<br>`, `<img>`, text).
- Expose minimal APIs for incremental consumption suitable for firmware rendering logic.
- Do not implement SD card transport in this crate; SD access remains a separate future crate.

## 3. Crate layout and interfaces
- New crate path: `crates/xteink-epub/`
- Public API:
  - `pub trait EpubSource { fn len(&self) -> usize; fn read_at(&self, offset: u64, dst: &mut [u8]) -> Result<usize, EpubError>; }`
  - `pub struct Epub<S: EpubSource> { source: S }`
  - `pub fn Epub::open(source: S) -> Result<Self, EpubError>`
  - `pub struct ReaderBuffers<'a> {`
  - `    zip_cd: &'a mut [u8],`
  - `    inflate: &'a mut [u8],`
  - `    xml: &'a mut [u8],`
  - `    catalog: &'a mut [u8],`
  - `    path_buf: &'a mut [u8],`
  - `}`
  - `pub fn next_event<'a>(&'a mut self, workspace: &'a mut ReaderBuffers<'_>) -> Result<Option<EpubEvent<'a>>, EpubError>`
- Output model:
  - `EpubEvent<'a>` variants:
    - `Text(&'a str)`
    - `ParagraphStart`, `ParagraphEnd`
    - `HeadingStart(u8)`, `HeadingEnd`
    - `LineBreak`
    - `Image { src: &'a str, alt: Option<&'a str> }`
    - `UnsupportedTag`
- Event lifetime rule: returned `EpubEvent<'a>` borrows from `workspace` memory and is invalidated by the next `next_event` call.
- Non-reentrancy rule: only one active event/cursor exists; caller must consume or clone event content before calling `next_event` again.
- `ReaderBuffers` must be alive for the full iterator lifetime and lives no longer than `Epub` if you need borrowed events across calls. For immediate processing flows, one call per event is expected.
- `ReaderBuffers<'a>` is a caller-supplied set of scratch buffers:
  - `zip_cd: &'a mut [u8]` for central-directory/entry scan
  - `inflate: &'a mut [u8]` for DEFLATE output per entry
  - `xml: &'a mut [u8]` for XML text staging where needed
  - `catalog: &'a mut [u8]` for compact manifest/spine index table
  - `path_buf: &'a mut [u8]` for URI/path normalize and UTF-8 decode
- Minimum sizing expectations:
  - `zip_cd`: at least one full EOCD + one central directory buffer (minimum 4 KiB; recommended 64 KiB).
  - `inflate`: at least maximum compressed XHTML file chunk seen in workload (recommend 256 KiB; must be explicit per build config).
  - `xml`: at least the longest serialized XML text chunk per parsed document (recommend 16 KiB for v1).
  - `catalog`: at least `(spine_entries + manifest_items) * 16` bytes; if insufficient, return `OutOfSpace`.
  - `path_buf`: at least 512 bytes.
- Out-of-space behavior:
  - APIs must return `EpubError::OutOfSpace` as soon as a fixed-capacity buffer cannot hold a record.
  - No partial event emission is allowed when a required capacity check fails.

### Source contracts
- `EpubSource::read_at` may return short reads if fewer bytes are available.
- `offset >= len` or `dst.is_empty()` returns `Ok(0)` immediately.
- For `offset < len`, parser expects the trait to return `Ok(n)` where `0 < n <= dst.len()` and `n <= len - offset`.
- If the underlying reader can satisfy fewer bytes than requested for reasons other than EOF, it may return a short read once; parser must retry as needed.
- `read_at` on an invalid underlying I/O condition returns `Err(EpubError::Io(...))`.

## 4. Parsing architecture
- **ZIP layer**
  - Read EOCD from end of file.
  - Parse central directory records and list entry metadata (name, compression method, offsets, sizes).
  - Locate needed entries by name lookup.
- **OPF layer**
  - Inflate `container.xml` and parse `rootfile` entry.
  - Inflate and parse `content.opf`, extract package base path, manifest item ids/hrefs/media-types, and spine order.
  - Store a compact catalog of manifest/spine entries in `ReaderBuffers::catalog`; if it does not fit return `OutOfSpace`.
- **XHTML layer**
  - For each spine item in order, inflate XHTML stream and parse tags.
  - Emit events while flattening nested structure into linear markup events.

## 5. Memory model
- Default parser operation uses no heap allocation.
- Internal parsing uses caller-provided scratch buffers for deflate output and parser workspaces.
- XML parsing and EPUB traversal operate on borrowed slices when valid (`&str` tied to scratch buffer lifetime).
- `EpubError::OutOfSpace` is returned if any supplied buffer is too small for required data.

## 6. Dependencies and no_std plan
- Use `quick-xml` for XML parsing.
- Use `zlib-rs` for Deflate decompression of ZIP entries.
- Keep dependencies without `std`:
  - `quick-xml = { version = "0.31", default-features = false, features = ["encoding"] }`
  - `zlib-rs = { version = "0.4", default-features = false, features = ["decompress"] }`
  - If either crate requires `alloc` on this target, the crate will switch to `alloc`-enabled mode explicitly in feature flags, but not `std`.
- Use `core` only in library by default; tests may enable `alloc`/`std`.

## 7. Error model
- `EpubError` enum: `Zip`, `Xml`, `Utf8`, `Compression`, `Io`, `OutOfSpace`, `InvalidFormat`, `Unsupported`.
- Errors always carry recoverable context where possible.

## 8. EPUB path and text normalization
- `container.xml` and `content.opf` follow EPUB relative-path rules.
- `Image.src` should be emitted as **package-relative path** resolved from:
  - package root from `container.xml` (`rootfile@full-path`) and OPF base path, with `../` normalization and percent-decoding where safe.
- Deterministic resolution order:
  1. Strip fragment/query from XHTML `src`/`href` before path resolution.
  2. Percent-decode once into ASCII/UTF-8 bytes.
  3. Resolve against package root and OPF base path using pure lexical `.` and `..`.
  4. Normalize path separators to `/`.
  5. Validate UTF-8 output; on decode failure emit `EpubError::Utf8`.
- Text normalization:
  - Entities must be decoded (`&lt;`, `&gt;`, `&amp;`, `&quot;`, `&apos;`, numeric entities).
  - Collapse consecutive whitespace from parsed XHTML text runs into a single space outside `<pre>`.
  - Unknown namespaces are ignored; unknown tags emit `UnsupportedTag` and continue with children.
  - `<br>` emits `LineBreak`.

- Explicit non-goals:
  - No ZIP64, no DRM/encryption, no signature verification.
  - No CSS cascade / computed-style rendering.
  - No support for `canvas`, script execution, or font embedding.

## 10. Test-driven workflow
- Add tests for each behavior before implementation:
  1. Container discovery and entry lookup against `test/epubs` samples.
  2. OPF parsing yields expected manifest/spine order.
  3. XHTML events preserve paragraph/heading/line-break/image markers.
  4. Non-text media-only EPUB path handles gracefully.
  5. Compressed/uncompressed entries both parse.
  6. Namespace, missing `content.opf`, missing `container.xml`, and bad item/path combinations produce explicit errors.
- Use `#[test]` fixtures by including sample epubs through crate tests.
- For this phase, tables are out-of-scope; renderers should rely on `UnsupportedTag` for table structures.
- Minimum test matrix must include `test/epubs` files with:
  - valid + malformed EPUB containers,
  - mixed path forms (`./`, `../`, percent-encoded paths),
  - and a deliberately truncated `ReaderBuffers` to verify `OutOfSpace`.

## 11. Acceptance criteria
- `cargo check` succeeds for workspace with crate included.
- New crate API can list and iterate spine items as stream events with low allocations.
- Parser can extract readable ordered content from at least one EPUB in `test/epubs` and emit the expected basic markup-preserving events.
