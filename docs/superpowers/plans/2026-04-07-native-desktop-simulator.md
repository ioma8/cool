# Native Desktop Simulator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a native desktop simulator that uses the exact same monochrome framebuffer, Bookerly font rendering, and EPUB pagination path as the real device while reading content from a host directory and accepting keyboard input as device buttons.

**Architecture:** First extract the reusable software renderer out of `xteink-display` into a shared crate, then extract firmware integration flow into a small shared app crate, then build a desktop binary that wires host storage, keyboard input, and a window presenter around those shared layers. Keep the SSD1677 transport in `xteink-display` so hardware and simulator share rendering but not device IO.

**Tech Stack:** Rust 2024, `embedded-hal`, `heapless`, `xteink-epub`, desktop windowing crate (`minifb` or `sdl2`), existing workspace crates

---

### Task 1: Add the shared renderer crate skeleton

**Files:**
- Create: `crates/xteink-render/Cargo.toml`
- Create: `crates/xteink-render/src/lib.rs`
- Modify: `Cargo.toml`
- Test: `crates/xteink-render/tests/framebuffer.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn framebuffer_starts_white_and_sets_black_pixels_in_device_layout() {
    // Assert buffer size, initial color, and pixel bit layout.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-render --test framebuffer`
Expected: FAIL because the crate does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Add the crate to the workspace and implement the framebuffer type plus display constants and `set_pixel`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-render --test framebuffer`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check --workspace --target aarch64-apple-darwin`
Expected: PASS

### Task 2: Move font/text rendering into the shared renderer

**Files:**
- Create: `crates/xteink-render/src/bookerly.rs`
- Create: `crates/xteink-render/src/text.rs`
- Modify: `crates/xteink-render/src/lib.rs`
- Modify: `crates/xteink-display/src/lib.rs`
- Test: `crates/xteink-render/tests/text_render.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn draw_text_uses_bookerly_glyphs_and_changes_expected_pixels() {
    // Render a short string and assert stable framebuffer bytes.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-render --test text_render`
Expected: FAIL because glyph drawing is not extracted yet.

- [ ] **Step 3: Write minimal implementation**

Move Bookerly font access, glyph drawing, wrapped text helpers, and related text buffer utilities into `xteink-render`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-render --test text_render`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check --workspace --target aarch64-apple-darwin`
Expected: PASS

### Task 3: Move EPUB page rendering and cached pagination into the shared renderer

**Files:**
- Create: `crates/xteink-render/src/pagination.rs`
- Modify: `crates/xteink-render/src/lib.rs`
- Modify: `crates/xteink-display/src/lib.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn cached_text_pagination_matches_existing_page_break_behavior() {
    // Feed cached text chunks and assert rendered page index/output.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-render --test epub_render`
Expected: FAIL because pagination/rendering is still embedded in `xteink-display`.

- [ ] **Step 3: Write minimal implementation**

Move cached-text pagination and EPUB render-to-framebuffer logic into the shared renderer while preserving behavior.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-render --test epub_render`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check --workspace --target aarch64-apple-darwin`
Expected: PASS

### Task 4: Reduce `xteink-display` to panel transport over a shared framebuffer

**Files:**
- Modify: `crates/xteink-display/Cargo.toml`
- Modify: `crates/xteink-display/src/lib.rs`
- Test: `crates/xteink-display/tests/transport.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn display_transport_exposes_shared_framebuffer_bytes_without_reencoding() {
    // Assert the transport writes the shared framebuffer unchanged.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-display --test transport --target aarch64-apple-darwin`
Expected: FAIL because the transport boundary is not isolated yet.

- [ ] **Step 3: Write minimal implementation**

Make `xteink-display` depend on `xteink-render`, own a renderer/framebuffer instance, and only handle SSD1677 transport and refresh operations.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-display --test transport --target aarch64-apple-darwin`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check --workspace --target aarch64-apple-darwin`
Expected: PASS

### Task 5: Add a shared app/session crate for browse and reader integration flow

**Files:**
- Create: `crates/xteink-app/Cargo.toml`
- Create: `crates/xteink-app/src/lib.rs`
- Create: `crates/xteink-app/src/browser.rs`
- Create: `crates/xteink-app/src/session.rs`
- Modify: `Cargo.toml`
- Test: `crates/xteink-app/tests/session.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn opening_an_epub_from_loaded_directory_transitions_to_reader_page_zero() {
    // Drive session commands with fake storage and fake renderer.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-app --test session`
Expected: FAIL because the crate does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Extract firmware integration helpers into a shared session layer with small traits for storage access and framebuffer rendering.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-app --test session`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check --workspace --target aarch64-apple-darwin`
Expected: PASS

### Task 6: Rewire firmware to use `xteink-app`

**Files:**
- Modify: `firmware/Cargo.toml`
- Modify: `firmware/src/main.rs`
- Test: existing firmware/controller host checks where possible

- [ ] **Step 1: Write the failing test**

Add or extend a host-side session test that proves firmware-specific glue is no longer needed for browser rendering and controller command dispatch.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-app --test session`
Expected: FAIL until firmware logic has been fully extracted.

- [ ] **Step 3: Write minimal implementation**

Replace inline browser/error/loading rendering and controller command dispatch in firmware with calls into `xteink-app`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-app --test session`
Expected: PASS

- [ ] **Step 5: Run validity checks**

Run: `cargo check --workspace --target aarch64-apple-darwin`
Expected: PASS

Run: `bash scripts/check-firmware.sh`
Expected: PASS

### Task 7: Add host storage adapter for simulator SD-card semantics

**Files:**
- Create: `simulator/Cargo.toml`
- Create: `simulator/src/storage.rs`
- Modify: `Cargo.toml`
- Test: `simulator/tests/storage.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn host_storage_maps_root_to_simulator_sdcard_and_lists_entries() {
    // Use a temp directory fixture and assert `/` semantics plus entry kinds.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p simulator --test storage`
Expected: FAIL because the simulator crate and storage adapter do not exist yet.

- [ ] **Step 3: Write minimal implementation**

Implement directory listing, path joining, and EPUB file open behavior over a host directory rooted at `simulator/sdcard/`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p simulator --test storage`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check --workspace --target aarch64-apple-darwin`
Expected: PASS

### Task 8: Add keyboard mapping and framebuffer presenter

**Files:**
- Create: `simulator/src/input.rs`
- Create: `simulator/src/window.rs`
- Test: `simulator/tests/input.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn arrow_enter_backspace_escape_keys_map_to_device_buttons() {
    // Assert exact key-to-button mapping.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p simulator --test input`
Expected: FAIL because keyboard mapping and presenter do not exist yet.

- [ ] **Step 3: Write minimal implementation**

Implement crisp black/white framebuffer presentation at an integer scale and exact keyboard mapping for device buttons.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p simulator --test input`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check --workspace --target aarch64-apple-darwin`
Expected: PASS

### Task 9: Add simulator runtime and end-to-end boot path

**Files:**
- Create: `simulator/src/runtime.rs`
- Create: `simulator/src/main.rs`
- Create: `simulator/sdcard/.gitkeep`
- Test: `simulator/tests/runtime.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn runtime_boots_root_directory_and_renders_first_browser_screen() {
    // Start runtime with fake window/input and assert non-empty browser framebuffer.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p simulator --test runtime`
Expected: FAIL because the runtime loop does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Wire storage, shared app session, renderer, and window/input loop into a runnable desktop simulator.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p simulator --test runtime`
Expected: PASS

- [ ] **Step 5: Run validity check**

Run: `cargo check --workspace --target aarch64-apple-darwin`
Expected: PASS

### Task 10: Add one-command startup script and docs

**Files:**
- Create: `scripts/run-simulator.sh`
- Modify: `README.md`
- Modify: `docs/PROJECT_OVERVIEW.md`

- [ ] **Step 1: Write the failing test**

Add a smoke verification step that fails when the simulator binary cannot be launched from the script.

- [ ] **Step 2: Run the verification to confirm failure**

Run: `bash scripts/run-simulator.sh`
Expected: FAIL until the script and binary wiring exist.

- [ ] **Step 3: Write minimal implementation**

Add the startup script, document keyboard mappings, document the `simulator/sdcard/` workflow, and ensure the script creates the folder if missing.

- [ ] **Step 4: Run the verification to confirm success**

Run: `bash scripts/run-simulator.sh`
Expected: Simulator window launches successfully.

- [ ] **Step 5: Run final quality gates**

Run: `cargo fmt --all`
Expected: PASS

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::all -D clippy::pedantic -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic`
Expected: PASS or actionable findings to fix

Run: `cargo test --workspace --target aarch64-apple-darwin`
Expected: PASS

Run: `bash scripts/check-firmware.sh`
Expected: PASS
