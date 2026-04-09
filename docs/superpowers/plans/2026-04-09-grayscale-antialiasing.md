# Grayscale Antialiasing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement 4-shade grayscale antialiasing across the shared renderer, web simulator, desktop simulator, and hardware display path, while keeping the architecture easy to extend to 16 shades later.

**Architecture:** Replace the current 1-bit shared framebuffer with a compact grayscale framebuffer, preserve quantized grayscale glyph coverage in generated font assets, and let each runtime consume the same shared render target through a thin adapter layer. Web and desktop will map shades directly to pixels; hardware will approximate 4 shades using multiple fast-update passes derived from the shared buffer.

**Tech Stack:** Rust, FreeType/fontdue/rustybuzz build pipeline, shared `xteink-render`, wasm/web canvas output, `minifb` desktop output, SSD1677 hardware display adapter

---

## Planned File Structure

### Shared renderer and font pipeline

- Modify: `crates/xteink-render/build.rs`
  - Quantize grayscale glyph coverage and emit packed multi-bit glyph data.
- Modify: `crates/xteink-render/src/bookerly.rs`
  - Update glyph bitmap access to support multi-bit coverage.
- Modify: `crates/xteink-render/src/lib.rs`
  - Replace the 1-bit framebuffer storage and drawing APIs with grayscale equivalents.
- Modify: `crates/xteink-render/tests/text_render.rs`
  - Validate glyph coverage quantization and grayscale text rendering.
- Modify: `crates/xteink-render/tests/framebuffer.rs`
  - Validate pixel packing/unpacking and shade semantics.
- Modify: `crates/xteink-render/tests/epub_render.rs`
  - Confirm EPUB rendering remains stable after grayscale framebuffer migration.

### Desktop simulator

- Modify: `simulator/src/window.rs`
  - Convert grayscale framebuffer shades into desktop window pixels.
- Modify: `simulator/src/runtime.rs`
  - Keep runtime glue aligned with the updated framebuffer type.

### Web simulator

- Modify: `web-simulator/src/lib.rs`
  - Convert grayscale framebuffer shades into RGBA canvas output.
- Modify: `web/app.js`
  - Keep the wasm driver path aligned if any exported behavior changes.

### Hardware display

- Modify: `crates/xteink-display/src/lib.rs`
  - Add grayscale pass extraction and multi-pass fast-update scheduling from the shared grayscale buffer.
- Modify: `firmware/src/main.rs`
  - Wire the updated display path if call signatures or refresh modes change.

### Validation and docs

- Modify: `README.md`
  - Document grayscale rendering behavior and simulator expectations if user-visible behavior changes materially.

---

### Task 1: Lock The Grayscale Framebuffer Format

**Files:**
- Modify: `crates/xteink-render/src/lib.rs`
- Test: `crates/xteink-render/tests/framebuffer.rs`

- [ ] **Step 1: Write the failing framebuffer packing tests**

Add tests that define:
- 4-shade pixel encoding/decoding
- clearing a framebuffer to a specific shade
- writing and reading individual shades
- compatibility helpers that map binary black/white requests onto grayscale endpoints

- [ ] **Step 2: Run the framebuffer tests to verify they fail**

Run: `cargo test -p xteink-render framebuffer -- --nocapture`
Expected: FAIL because the current framebuffer is still 1-bit.

- [ ] **Step 3: Introduce grayscale framebuffer constants and helpers**

Implement in `crates/xteink-render/src/lib.rs`:
- `GRAY_LEVELS`
- bits-per-pixel helpers
- packed storage indexing
- `set_shade`
- `shade_at`
- binary compatibility helpers layered on top

- [ ] **Step 4: Update clear/fill operations to use shades**

Convert:
- `clear`
- `fill_rect`
- any internal assumptions that `0xFF` means white for every storage byte

- [ ] **Step 5: Run the framebuffer tests and full render crate tests**

Run: `cargo test -p xteink-render`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-render/src/lib.rs crates/xteink-render/tests/framebuffer.rs
git commit -m "Add grayscale framebuffer core"
```

### Task 2: Preserve Grayscale Glyph Coverage In Generated Font Assets

**Files:**
- Modify: `crates/xteink-render/build.rs`
- Modify: `crates/xteink-render/src/bookerly.rs`
- Test: `crates/xteink-render/tests/text_render.rs`

- [ ] **Step 1: Write failing glyph coverage tests**

Extend `text_render.rs` to assert:
- generated glyph coverage preserves multiple shade levels
- unpacked glyph data matches quantized FreeType coverage, not binary threshold output

- [ ] **Step 2: Run the targeted text-render test to verify it fails**

Run: `cargo test -p xteink-render generated_bookerly -- --nocapture`
Expected: FAIL because generated glyph data is still binary.

- [ ] **Step 3: Implement 4-shade quantization in the build script**

Update `crates/xteink-render/build.rs` to:
- quantize grayscale coverage into 4 levels
- pack multi-bit glyph rows
- emit metadata that keeps glyph metrics unchanged

- [ ] **Step 4: Update generated glyph access helpers**

Adjust `crates/xteink-render/src/bookerly.rs` to expose per-pixel shade lookup for glyph coverage.

- [ ] **Step 5: Run render tests**

Run: `cargo test -p xteink-render`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-render/build.rs crates/xteink-render/src/bookerly.rs crates/xteink-render/tests/text_render.rs
git commit -m "Preserve grayscale glyph coverage"
```

### Task 3: Migrate Shared Text Rendering To Grayscale

**Files:**
- Modify: `crates/xteink-render/src/lib.rs`
- Test: `crates/xteink-render/tests/text_render.rs`
- Test: `crates/xteink-render/tests/epub_render.rs`

- [ ] **Step 1: Write a failing grayscale text test**

Add a test proving that rendering a glyph writes more than two distinct shade values into the framebuffer.

- [ ] **Step 2: Run the new grayscale text test to verify it fails**

Run: `cargo test -p xteink-render grayscale -- --nocapture`
Expected: FAIL because text rendering currently writes binary black/white only.

- [ ] **Step 3: Update glyph drawing to write shades**

Modify `draw_glyph` and related helpers to read grayscale glyph coverage and write shade values into the grayscale framebuffer.

- [ ] **Step 4: Keep layout and pagination logic behaviorally stable**

Do not alter text shaping, metrics, line breaking, or EPUB pagination semantics beyond the rasterization target.

- [ ] **Step 5: Run the full render test suite**

Run: `cargo test -p xteink-render`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/xteink-render/src/lib.rs crates/xteink-render/tests/text_render.rs crates/xteink-render/tests/epub_render.rs
git commit -m "Render text into grayscale framebuffer"
```

### Task 4: Update The Desktop Simulator Output Path

**Files:**
- Modify: `simulator/src/window.rs`
- Modify: `simulator/src/runtime.rs`

- [ ] **Step 1: Write a small output-mapping test if practical**

If `simulator/src/window.rs` supports lightweight unit coverage, add a test for 4-shade-to-RGB mapping. If not practical, document that this task relies on compile verification plus manual smoke testing.

- [ ] **Step 2: Implement 4-shade desktop pixel mapping**

Update `simulator/src/window.rs` to:
- read shades from the shared framebuffer
- map the four levels to a fixed grayscale ramp

- [ ] **Step 3: Verify desktop simulator compilation**

Run: `cargo check -p simulator`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add simulator/src/window.rs simulator/src/runtime.rs
git commit -m "Render grayscale output in desktop simulator"
```

### Task 5: Update The Web Simulator Output Path

**Files:**
- Modify: `web-simulator/src/lib.rs`
- Modify: `web/app.js` (only if wasm API changes)

- [ ] **Step 1: Update the canvas conversion path**

Convert framebuffer shades into RGBA grayscale values in `web-simulator/src/lib.rs`.

- [ ] **Step 2: Keep wasm interactions stable unless forced**

Only touch `web/app.js` if exported wasm behavior or user-visible diagnostics require it.

- [ ] **Step 3: Verify wasm compilation**

Run: `cargo check -p web-simulator --target wasm32-unknown-unknown`
Expected: PASS

- [ ] **Step 4: Smoke-build the web bundle**

Run: `bash scripts/build-web-simulator.sh`
Expected: PASS and updated bundle in `dist/`

- [ ] **Step 5: Commit**

```bash
git add web-simulator/src/lib.rs web/app.js
git commit -m "Render grayscale output in web simulator"
```

### Task 6: Implement Hardware 4-Shade Approximation

**Files:**
- Modify: `crates/xteink-display/src/lib.rs`
- Modify: `firmware/src/main.rs`

- [ ] **Step 1: Write focused tests or debug helpers for shade-plane extraction**

Add unit coverage where practical for:
- deriving binary pass masks from grayscale levels
- preserving expected black/white endpoints

If full hardware unit tests are not practical, at minimum add deterministic helper tests for pass generation.

- [ ] **Step 2: Run the new hardware helper tests to verify they fail**

Run: `cargo test -p xteink-display`
Expected: FAIL for new grayscale helper assertions before implementation.

- [ ] **Step 3: Implement grayscale pass extraction**

In `crates/xteink-display/src/lib.rs`, add:
- a representation for grayscale approximation passes
- conversion from shared grayscale framebuffer into fast-update pass masks

- [ ] **Step 4: Implement multi-pass fast-update scheduling**

Use the existing SSD1677 update flow to apply the generated passes without switching to a full-refresh-only path.

- [ ] **Step 5: Wire firmware call sites**

Update firmware integration only as needed to select the new grayscale-capable display path.

- [ ] **Step 6: Run hardware-oriented checks**

Run: `cargo check -p xteink-display && cargo check -p firmware`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/xteink-display/src/lib.rs firmware/src/main.rs
git commit -m "Approximate grayscale on hardware display"
```

### Task 7: End-To-End Validation

**Files:**
- Modify: `README.md` (if needed)

- [ ] **Step 1: Run shared renderer tests**

Run: `cargo test -p xteink-render`
Expected: PASS

- [ ] **Step 2: Run desktop compile check**

Run: `cargo check -p simulator`
Expected: PASS

- [ ] **Step 3: Run web compile check**

Run: `cargo check -p web-simulator --target wasm32-unknown-unknown`
Expected: PASS

- [ ] **Step 4: Run firmware/display compile checks**

Run: `cargo check -p xteink-display && cargo check -p firmware`
Expected: PASS

- [ ] **Step 5: Build the web bundle**

Run: `bash scripts/build-web-simulator.sh`
Expected: PASS

- [ ] **Step 6: Document any user-visible behavior changes**

If grayscale output changes simulator or hardware expectations, update `README.md` concisely.

- [ ] **Step 7: Commit**

```bash
git add README.md
git commit -m "Document grayscale antialiasing support"
```

### Task 8: Manual Smoke Verification Checklist

**Files:**
- No source changes required unless issues are found.

- [ ] **Step 1: Desktop simulator visual check**

Run: `bash scripts/run-simulator.sh`
Expected: text appears cleaner with visible intermediate shades instead of binary heavy edges.

- [ ] **Step 2: Web simulator visual check**

Run:

```bash
bash scripts/build-web-simulator.sh
cd dist && python3 -m http.server 8000
```

Expected: sample book pages show visible 4-shade antialiasing in the browser.

- [ ] **Step 3: Hardware smoke check**

Run the firmware on device and verify:
- text is visibly smoother
- shades are distinguishable
- update path uses fast-update approximation rather than full-refresh-only rendering

- [ ] **Step 4: Final commit if smoke-check-driven fixes were needed**

```bash
git add <exact files>
git commit -m "Polish grayscale antialiasing rollout"
```

## Notes For The Implementer

- Keep the renderer and runtime adapters separated. Do not let hardware approximation details leak into shared text rendering.
- Make all shade count assumptions local. The plan should leave a clear seam for 16-shade expansion later.
- Preserve existing tests wherever possible; the rasterization target is changing, not the pagination/text-shaping semantics.
- Avoid carrying `dist/` or other generated artifacts into source commits unless explicitly requested.
