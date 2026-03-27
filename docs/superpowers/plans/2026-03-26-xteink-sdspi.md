# xteink-sdspi Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the C++ SD bridge with a pure-Rust SD transport crate that matches the working SdFat bring-up behavior and exposes read-only block access for the firmware filesystem layer.

**Architecture:** Add a small no_std transport crate that owns the SD command sequence, clock switching, and block reads. Keep filesystem concerns in firmware by adapting the transport into `embedded-sdmmc` for directory browsing and EPUB reads. The firmware keeps the display path intact and only swaps the SD backend.

**Tech Stack:** Rust 2024, `embedded-hal`, `embedded-sdmmc`, `esp-hal`

---

### Task 1: Create the transport crate and failing tests

**Files:**
- Create: `crates/xteink-sdspi/Cargo.toml`
- Create: `crates/xteink-sdspi/src/lib.rs`
- Create: `crates/xteink-sdspi/tests/driver.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn acquire_sequence_matches_sd_spi_boot_flow() {
    // Assert the exact power / clock / CMD0 / CMD8 / ACMD41 / CMD58 sequence.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-sdspi`
Expected: compile/test failure because the transport implementation does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Implement the crate skeleton and the test-only mock transport / pin types so the failing assertions become meaningful.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-sdspi`
Expected: PASS

### Task 2: Implement read-only block device behavior

**Files:**
- Modify: `crates/xteink-sdspi/src/lib.rs`
- Modify: `crates/xteink-sdspi/tests/driver.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn single_block_read_uses_cmd17_and_returns_512_bytes() {
    // Assert sector addressing, token handling, and exact byte count.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-sdspi`
Expected: FAIL because block reads are still missing or incomplete.

- [ ] **Step 3: Write minimal implementation**

Add `embedded-sdmmc::BlockDevice` support with `read` and `num_blocks`, plus the SD CSD parsing needed for capacity.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p xteink-sdspi`
Expected: PASS

### Task 3: Replace the firmware C++ bridge with the Rust transport

**Files:**
- Modify: `firmware/Cargo.toml`
- Modify: `firmware/build.rs`
- Modify: `firmware/src/sd_ffi.rs`
- Modify: `firmware/src/main.rs`
- Delete or stop using: `firmware/native/sd_bridge.cpp`
- Delete or stop using: `firmware/native/SPI.cpp`

- [ ] **Step 1: Write the failing test**

Add a firmware-side test that validates the SD adapter can list directory entries and open an EPUB from the mounted card through the Rust transport.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xteink-reader`
Expected: FAIL until the new adapter is wired up.

- [ ] **Step 3: Write minimal implementation**

Wire the Rust transport into an `embedded-sdmmc::VolumeManager` adapter and remove the C++ build bridge.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo build -p xteink-reader --release`
Expected: PASS

