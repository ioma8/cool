#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

#[cfg(not(target_arch = "wasm32"))]
pub fn not_wasm_build_placeholder() {}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use serde::{Deserialize, Serialize};
    use std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
        rc::Rc,
    };
    use wasm_bindgen::{Clamped, JsCast, prelude::*};
    use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};
    use xteink_app::{
        AppStorage, DirectoryPage as AppDirectoryPage, DirectoryPageInfo as AppDirectoryPageInfo,
        EpubRenderResult, ListedEntry, Session,
    };
    use xteink_buttons::Button;
    use xteink_fs::{
        DirectoryPage as FsDirectoryPage, DirectoryPageInfo as FsDirectoryPageInfo, FsError,
        ListedEntry as FsListedEntry, SdFilesystem, SdFsFile, listed_entry_from_parts,
        load_directory_page, render_epub_from_entry, render_epub_page_from_entry,
    };
    use xteink_render::{DISPLAY_HEIGHT, DISPLAY_WIDTH, DISPLAY_WIDTH_BYTES, Framebuffer};

    const STORAGE_KEY: &str = "xteink_web_sdcard_v1";

    thread_local! {
        static APP: RefCell<Option<WebApp>> = const { RefCell::new(None) };
    }

    #[derive(Serialize, Deserialize)]
    struct StoredVfs {
        files: BTreeMap<String, String>,
        dirs: Vec<String>,
    }

    #[derive(Clone)]
    struct WebStorage {
        vfs: Rc<RefCell<Vfs>>,
    }

    struct WebEpubSource {
        data: Vec<u8>,
    }

    impl xteink_epub::EpubSource for WebEpubSource {
        fn len(&self) -> usize {
            self.data.len()
        }

        fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, xteink_epub::EpubError> {
            let start = usize::try_from(offset).map_err(|_| xteink_epub::EpubError::Io)?;
            if start >= self.data.len() {
                return Ok(0);
            }
            let end = (start + buffer.len()).min(self.data.len());
            let src = &self.data[start..end];
            buffer[..src.len()].copy_from_slice(src);
            Ok(src.len())
        }
    }

    enum FileMode {
        Read,
        Write,
    }

    struct WebFile<'a> {
        mode: FileMode,
        path: String,
        data: Vec<u8>,
        cursor: usize,
        vfs: Option<&'a RefCell<Vfs>>,
    }

    impl SdFsFile for WebFile<'_> {
        fn len(&self) -> usize {
            self.data.len()
        }

        fn seek_from_start(&mut self, offset: u32) -> Result<(), FsError> {
            self.cursor = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(self.data.len());
            Ok(())
        }

        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, FsError> {
            if self.cursor >= self.data.len() {
                return Ok(0);
            }
            let end = (self.cursor + buffer.len()).min(self.data.len());
            let src = &self.data[self.cursor..end];
            buffer[..src.len()].copy_from_slice(src);
            self.cursor = end;
            Ok(src.len())
        }

        fn write(&mut self, buffer: &[u8]) -> Result<usize, FsError> {
            if !matches!(self.mode, FileMode::Write) {
                return Err(WebStorage::host_error("readonly"));
            }
            if self.cursor > self.data.len() {
                self.data.resize(self.cursor, 0);
            }
            let needed = self.cursor.saturating_add(buffer.len());
            if needed > self.data.len() {
                self.data.resize(needed, 0);
            }
            self.data[self.cursor..self.cursor + buffer.len()].copy_from_slice(buffer);
            self.cursor = self.cursor.saturating_add(buffer.len());
            Ok(buffer.len())
        }

        fn flush(&mut self) -> Result<(), FsError> {
            if let (FileMode::Write, Some(vfs)) = (&self.mode, self.vfs) {
                let mut state = vfs.borrow_mut();
                state.write_file(self.path.as_str(), self.data.clone())?;
                state.persist()?;
            }
            Ok(())
        }
    }

    impl WebStorage {
        const MAX_ENTRY_TEXT: usize = 96;

        fn host_error(message: &str) -> FsError {
            let mut error = heapless::String::<64>::new();
            let _ = error.push_str(message);
            FsError::OpenFailed(error)
        }

        fn epub_error(error: xteink_epub::EpubError) -> FsError {
            match error {
                xteink_epub::EpubError::Io => Self::host_error("epub io"),
                xteink_epub::EpubError::Zip => Self::host_error("epub zip"),
                xteink_epub::EpubError::Utf8 => Self::host_error("epub utf8"),
                xteink_epub::EpubError::InvalidFormat => Self::host_error("epub invalid format"),
                xteink_epub::EpubError::Compression => Self::host_error("epub compression"),
                xteink_epub::EpubError::OutOfSpace => Self::host_error("epub out of space"),
                xteink_epub::EpubError::Unsupported => Self::host_error("epub unsupported"),
                xteink_epub::EpubError::Cancelled => Self::host_error("epub cancelled"),
            }
        }

        fn load() -> Self {
            Self {
                vfs: Rc::new(RefCell::new(Vfs::load())),
            }
        }

        fn reset_session() -> Result<(), JsValue> {
            APP.with(|app| {
                let app = app.borrow();
                let Some(existing) = app.as_ref() else {
                    return Err(JsValue::from_str("app not initialized"));
                };
                let storage = existing.session.storage().clone();
                drop(app);

                let mut session = Session::new(storage, Framebuffer::new(), 8);
                session
                    .bootstrap()
                    .map_err(|_| JsValue::from_str("bootstrap failed"))?;

                APP.with(|slot| {
                    if let Some(app) = slot.borrow_mut().as_mut() {
                        app.session = session;
                        app.render();
                    }
                });
                Ok(())
            })
        }

        fn add_uploaded_file(name: &str, data: Vec<u8>) -> Result<(), FsError> {
            APP.with(|app| {
                let app = app.borrow();
                let Some(existing) = app.as_ref() else {
                    return Err(Self::host_error("app not initialized"));
                };
                let mut vfs = existing.session.storage().vfs.borrow_mut();
                let path = format!("/{name}");
                vfs.write_file(path.as_str(), data)?;
                vfs.persist()?;
                Ok(())
            })
        }

        fn app_entry(entry: &FsListedEntry) -> ListedEntry {
            let mut label = heapless::String::new();
            let mut fs_name = heapless::String::new();
            let _ = label.push_str(entry.label.as_str());
            let _ = fs_name.push_str(entry.fs_name.as_str());
            ListedEntry {
                label,
                fs_name,
                kind: entry.kind,
            }
        }
    }

    impl SdFilesystem for WebStorage {
        type EpubSource<'a>
            = WebEpubSource
        where
            Self: 'a;
        type File<'a>
            = WebFile<'a>
        where
            Self: 'a;

        fn list_directory_page(
            &self,
            path: &str,
            page_start: usize,
            page_size: usize,
            entries: &mut heapless::Vec<FsListedEntry, { xteink_fs::MAX_ENTRIES }>,
        ) -> Result<FsDirectoryPageInfo, FsError> {
            let state = self.vfs.borrow();
            let mut all = state.list_entries(path)?;
            all.sort_by(|a, b| {
                if a.1 != b.1 {
                    b.1.cmp(&a.1)
                } else {
                    a.0.cmp(&b.0)
                }
            });
            let total = all.len();
            let start = page_start.min(total);
            let end = (start + page_size).min(total);
            for (name, is_dir) in all[start..end].iter() {
                let label = if name.len() > Self::MAX_ENTRY_TEXT {
                    &name[..Self::MAX_ENTRY_TEXT]
                } else {
                    name
                };
                let fs_name = if listed_entry_from_parts(label, name, *is_dir).is_ok() {
                    name.clone()
                } else {
                    short_component_name(name, *is_dir)
                };
                let entry = listed_entry_from_parts(label, fs_name.as_str(), *is_dir)?;
                entries.push(entry).map_err(|_| FsError::TooManyEntries)?;
            }
            Ok(FsDirectoryPageInfo {
                page_start: start,
                has_prev: start > 0,
                has_next: end < total,
            })
        }

        fn open_epub_source<'a>(&'a self, path: &str) -> Result<Self::EpubSource<'a>, FsError> {
            let data = self.vfs.borrow().read_file(path)?;
            Ok(WebEpubSource { data })
        }

        fn open_cache_file_read<'a>(&'a self, path: &str) -> Result<Self::File<'a>, FsError> {
            Ok(WebFile {
                mode: FileMode::Read,
                path: normalize(path),
                data: self.vfs.borrow().read_file(path)?,
                cursor: 0,
                vfs: None,
            })
        }

        fn open_cache_file_write<'a>(&'a self, path: &str) -> Result<Self::File<'a>, FsError> {
            Ok(WebFile {
                mode: FileMode::Write,
                path: normalize(path),
                data: Vec::new(),
                cursor: 0,
                vfs: Some(self.vfs.as_ref()),
            })
        }

        fn open_cache_file_append<'a>(&'a self, path: &str) -> Result<Self::File<'a>, FsError> {
            let data = self.vfs.borrow().read_file_or_default(path);
            let cursor = data.len();
            Ok(WebFile {
                mode: FileMode::Write,
                path: normalize(path),
                data,
                cursor,
                vfs: Some(self.vfs.as_ref()),
            })
        }

        fn ensure_directory(&self, path: &str) -> Result<(), FsError> {
            let mut state = self.vfs.borrow_mut();
            state.ensure_directory(path)?;
            state.persist()?;
            Ok(())
        }
    }

    impl AppStorage<Framebuffer> for WebStorage {
        type Error = FsError;

        fn list_directory_page(
            &self,
            path: &str,
            page_start: usize,
            page_size: usize,
        ) -> Result<AppDirectoryPage, Self::Error> {
            let FsDirectoryPage { entries, info } =
                load_directory_page(self, path, page_start, page_size)?;
            let mut mapped = heapless::Vec::new();
            for entry in entries.iter() {
                mapped
                    .push(Self::app_entry(entry))
                    .map_err(|_| Self::host_error("too many entries"))?;
            }
            Ok(AppDirectoryPage {
                entries: mapped,
                info: AppDirectoryPageInfo {
                    page_start: info.page_start,
                    has_prev: info.has_prev,
                    has_next: info.has_next,
                },
            })
        }

        fn render_epub_from_entry(
            &self,
            renderer: &mut Framebuffer,
            current_path: &str,
            entry: &ListedEntry,
        ) -> Result<EpubRenderResult, Self::Error> {
            let fs_entry = listed_entry_from_parts(
                entry.label.as_str(),
                entry.fs_name.as_str(),
                entry.kind == xteink_browser::EntryKind::Directory,
            )?;
            let rendered = render_epub_from_entry(self, renderer, current_path, &fs_entry)
                .map_err(Self::epub_error)?;
            Ok(EpubRenderResult {
                rendered_page: rendered.rendered_page,
                progress_percent: rendered.progress_percent,
            })
        }

        fn render_epub_page_from_entry(
            &self,
            renderer: &mut Framebuffer,
            current_path: &str,
            entry: &ListedEntry,
            target_page: usize,
        ) -> Result<EpubRenderResult, Self::Error> {
            let fs_entry = listed_entry_from_parts(
                entry.label.as_str(),
                entry.fs_name.as_str(),
                entry.kind == xteink_browser::EntryKind::Directory,
            )?;
            let rendered = render_epub_page_from_entry(
                self,
                renderer,
                current_path,
                &fs_entry,
                target_page,
                true,
            )
            .map_err(Self::epub_error)?;
            Ok(EpubRenderResult {
                rendered_page: rendered.rendered_page,
                progress_percent: rendered.progress_percent,
            })
        }
    }

    struct Vfs {
        files: BTreeMap<String, Vec<u8>>,
        dirs: BTreeSet<String>,
    }

    impl Vfs {
        fn load() -> Self {
            let mut state = Self {
                files: BTreeMap::new(),
                dirs: BTreeSet::new(),
            };
            state.dirs.insert("/".to_string());
            state.dirs.insert("/.cool".to_string());
            let Some(storage) = local_storage() else {
                return state;
            };
            let Ok(Some(raw)) = storage.get_item(STORAGE_KEY) else {
                return state;
            };
            let Ok(parsed) = serde_json::from_str::<StoredVfs>(raw.as_str()) else {
                return state;
            };
            for dir in parsed.dirs {
                state.dirs.insert(normalize(dir.as_str()));
            }
            for (path, encoded) in parsed.files {
                if let Some(bytes) = decode_hex(encoded.as_str()) {
                    state.files.insert(normalize(path.as_str()), bytes);
                }
            }
            state
        }

        fn persist(&self) -> Result<(), FsError> {
            let Some(storage) = local_storage() else {
                return Err(WebStorage::host_error("no localStorage"));
            };
            let mut files = BTreeMap::new();
            for (path, bytes) in &self.files {
                files.insert(path.clone(), encode_hex(bytes));
            }
            let dirs = self.dirs.iter().cloned().collect::<Vec<_>>();
            let payload = serde_json::to_string(&StoredVfs { files, dirs })
                .map_err(|_| WebStorage::host_error("serialize storage"))?;
            storage
                .set_item(STORAGE_KEY, payload.as_str())
                .map_err(|_| WebStorage::host_error("persist storage"))
        }

        fn ensure_directory(&mut self, path: &str) -> Result<(), FsError> {
            let mut current = String::from("/");
            for component in normalize(path).split('/').filter(|c| !c.is_empty()) {
                if current.len() > 1 {
                    current.push('/');
                }
                current.push_str(component);
                self.dirs.insert(current.clone());
            }
            Ok(())
        }

        fn write_file(&mut self, path: &str, data: Vec<u8>) -> Result<(), FsError> {
            let normalized = normalize(path);
            let parent = parent_dir(normalized.as_str());
            self.ensure_directory(parent.as_str())?;
            self.files.insert(normalized, data);
            Ok(())
        }

        fn read_file(&self, path: &str) -> Result<Vec<u8>, FsError> {
            let resolved = self.resolve_path(path)?;
            self.files
                .get(resolved.as_str())
                .cloned()
                .ok_or_else(|| WebStorage::host_error("file not found"))
        }

        fn read_file_or_default(&self, path: &str) -> Vec<u8> {
            self.resolve_path(path)
                .ok()
                .and_then(|resolved| self.files.get(resolved.as_str()).cloned())
                .unwrap_or_default()
        }

        fn list_entries(&self, path: &str) -> Result<Vec<(String, bool)>, FsError> {
            let dir = self.resolve_directory(path)?;
            let mut entries = BTreeMap::<String, bool>::new();

            for child in self.dirs.iter().filter(|entry| parent_dir(entry) == dir) {
                if child != &dir {
                    entries.insert(base_name(child), true);
                }
            }
            for file in self.files.keys().filter(|entry| parent_dir(entry) == dir) {
                entries.insert(base_name(file), false);
            }

            Ok(entries.into_iter().collect())
        }

        fn resolve_directory(&self, path: &str) -> Result<String, FsError> {
            let resolved = self.resolve_path(path)?;
            if self.dirs.contains(&resolved) {
                Ok(resolved)
            } else {
                Err(WebStorage::host_error("dir not found"))
            }
        }

        fn resolve_path(&self, path: &str) -> Result<String, FsError> {
            let normalized = normalize(path);
            if self.files.contains_key(&normalized) || self.dirs.contains(&normalized) {
                return Ok(normalized);
            }
            if normalized == "/" {
                return Ok(normalized);
            }

            let mut current = String::from("/");
            for component in normalized.split('/').filter(|c| !c.is_empty()) {
                let candidates = self.list_entries(current.as_str())?;
                let direct = candidates
                    .iter()
                    .find(|(name, _)| name == component)
                    .map(|(name, _)| name.clone())
                    .or_else(|| {
                        candidates
                            .iter()
                            .find(|(name, is_dir)| short_component_name(name, *is_dir) == component)
                            .map(|(name, _)| name.clone())
                    })
                    .ok_or_else(|| WebStorage::host_error("component not found"))?;

                if current.len() > 1 {
                    current.push('/');
                }
                current.push_str(direct.as_str());
            }
            Ok(current)
        }
    }

    fn parent_dir(path: &str) -> String {
        if path == "/" {
            return "/".to_string();
        }
        let trimmed = path.trim_end_matches('/');
        let Some((parent, _)) = trimmed.rsplit_once('/') else {
            return "/".to_string();
        };
        if parent.is_empty() {
            "/".to_string()
        } else {
            parent.to_string()
        }
    }

    fn base_name(path: &str) -> String {
        path.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(path)
            .to_string()
    }

    fn short_component_name(name: &str, is_directory: bool) -> String {
        let hash = stable_name_hash(name) & 0x0FFF_FFFF;
        let extension = if is_directory {
            None
        } else {
            name.rsplit_once('.').and_then(|(_, ext)| {
                let mut short = String::new();
                for ch in ext.chars() {
                    if ch.is_ascii_alphanumeric() {
                        short.push(ch.to_ascii_uppercase());
                        if short.len() == 8 {
                            break;
                        }
                    }
                }
                if short.is_empty() { None } else { Some(short) }
            })
        };
        match extension {
            Some(extension) => format!("F{hash:07X}.{extension}"),
            None if is_directory => format!("D{hash:07X}"),
            None => format!("F{hash:07X}"),
        }
    }

    fn stable_name_hash(name: &str) -> u32 {
        let mut hash = 0x811C9DC5u32;
        for byte in name.as_bytes() {
            hash ^= u32::from(*byte);
            hash = hash.wrapping_mul(0x01000193);
        }
        hash
    }

    fn normalize(path: &str) -> String {
        let mut out = String::from("/");
        for component in path.split('/').filter(|c| !c.is_empty() && *c != ".") {
            if component == ".." {
                out = parent_dir(out.as_str());
                continue;
            }
            if out.len() > 1 {
                out.push('/');
            }
            out.push_str(component);
        }
        out
    }

    fn local_storage() -> Option<web_sys::Storage> {
        let window = web_sys::window()?;
        window.local_storage().ok().flatten()
    }

    fn encode_hex(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in bytes {
            out.push(HEX[usize::from(byte >> 4)] as char);
            out.push(HEX[usize::from(byte & 0x0f)] as char);
        }
        out
    }

    fn decode_hex(input: &str) -> Option<Vec<u8>> {
        if input.len() % 2 != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(input.len() / 2);
        let bytes = input.as_bytes();
        let to_nibble = |b: u8| -> Option<u8> {
            match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(10 + b - b'a'),
                b'A'..=b'F' => Some(10 + b - b'A'),
                _ => None,
            }
        };
        for i in (0..bytes.len()).step_by(2) {
            let hi = to_nibble(bytes[i])?;
            let lo = to_nibble(bytes[i + 1])?;
            out.push((hi << 4) | lo);
        }
        Some(out)
    }

    struct WebApp {
        session: Session<WebStorage, Framebuffer>,
        canvas: HtmlCanvasElement,
        ctx: CanvasRenderingContext2d,
        rgba: Vec<u8>,
    }

    impl WebApp {
        fn render(&mut self) {
            let fb = self.session.renderer();
            let width = usize::from(DISPLAY_WIDTH);
            let height = usize::from(DISPLAY_HEIGHT);

            for y in 0..height {
                for x in 0..width {
                    let py = width - 1 - x;
                    let idx = py * usize::from(DISPLAY_WIDTH_BYTES) + (y / 8);
                    let bit = 7 - (y as u16 % 8);
                    let black = (fb.bytes()[idx] & (1 << bit)) == 0;
                    let color = if black { 0 } else { 255 };
                    let o = (y * width + x) * 4;
                    self.rgba[o] = color;
                    self.rgba[o + 1] = color;
                    self.rgba[o + 2] = color;
                    self.rgba[o + 3] = 255;
                }
            }

            if let Ok(image) = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
                Clamped(self.rgba.as_slice()),
                u32::from(DISPLAY_WIDTH),
                u32::from(DISPLAY_HEIGHT),
            ) {
                let _ = self.ctx.put_image_data(&image, 0.0, 0.0);
            }
        }

        fn handle_button(&mut self, button: Button) {
            let _ = self.session.handle_button(button);
            self.render();
        }
    }

    fn init_app() -> Result<WebApp, JsValue> {
        console_error_panic_hook::set_once();

        let window = web_sys::window().ok_or_else(|| JsValue::from_str("window missing"))?;
        let document = window
            .document()
            .ok_or_else(|| JsValue::from_str("document missing"))?;
        let canvas = document
            .get_element_by_id("screen")
            .ok_or_else(|| JsValue::from_str("#screen missing"))?
            .dyn_into::<HtmlCanvasElement>()?;
        canvas.set_width(u32::from(DISPLAY_WIDTH));
        canvas.set_height(u32::from(DISPLAY_HEIGHT));

        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("2d context missing"))?
            .dyn_into::<CanvasRenderingContext2d>()?;
        ctx.set_image_smoothing_enabled(false);

        let storage = WebStorage::load();
        let mut session = Session::new(storage, Framebuffer::new(), 8);
        session
            .bootstrap()
            .map_err(|_| JsValue::from_str("bootstrap failed"))?;

        let mut app = WebApp {
            session,
            canvas,
            ctx,
            rgba: vec![255; usize::from(DISPLAY_WIDTH) * usize::from(DISPLAY_HEIGHT) * 4],
        };
        app.render();
        Ok(app)
    }

    #[wasm_bindgen(start)]
    pub fn start() -> Result<(), JsValue> {
        let app = init_app()?;
        APP.with(|slot| *slot.borrow_mut() = Some(app));
        Ok(())
    }

    fn with_app(mut f: impl FnMut(&mut WebApp)) {
        APP.with(|slot| {
            if let Some(app) = slot.borrow_mut().as_mut() {
                f(app);
            }
        });
    }

    #[wasm_bindgen]
    pub fn press_up() {
        with_app(|app| app.handle_button(Button::Up));
    }

    #[wasm_bindgen]
    pub fn press_down() {
        with_app(|app| app.handle_button(Button::Down));
    }

    #[wasm_bindgen]
    pub fn press_left() {
        with_app(|app| app.handle_button(Button::Left));
    }

    #[wasm_bindgen]
    pub fn press_right() {
        with_app(|app| app.handle_button(Button::Right));
    }

    #[wasm_bindgen]
    pub fn press_cancel() {
        with_app(|app| app.handle_button(Button::Back));
    }

    #[wasm_bindgen]
    pub fn press_confirm() {
        with_app(|app| app.handle_button(Button::Confirm));
    }

    #[wasm_bindgen]
    pub fn press_power() {
        with_app(|app| app.handle_button(Button::Power));
    }

    #[wasm_bindgen]
    pub fn upload_epub(name: String, bytes: Vec<u8>) -> Result<(), JsValue> {
        WebStorage::add_uploaded_file(name.as_str(), bytes)
            .map_err(|_| JsValue::from_str("failed to store file"))?;
        WebStorage::reset_session()?;
        Ok(())
    }
}
