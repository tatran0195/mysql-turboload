# Large Item Progress Tracking, ETA Estimation & Gzip Stream Acceleration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Provide real-time streaming progress, throughput, and predictive ETA for large items across both import and export (compressed `.sql.gz` and plain `.sql`), and accelerate Gzip decompression throughput by 3x–5x using SIMD `zlib-ng` and buffered I/O.

**Architecture:**
- Universal `CountingReader` wrapping buffered streams to update lock-free `AtomicU64` byte counters.
- Feeder threads in `runner.rs` and `exporter.rs` stream through 256 KB buffers and signal `is_committing` upon EOF.
- Sampling tick in `progress.rs` computes EWMA throughput, detects stalls, and renders multi-phase worker lines and critical-path overall ETAs.
- SIMD-accelerated `flate2` (`zlib-ng`) with 256 KB `BufReader` and pipe buffers.

**Tech Stack:** Rust (edition 2021), `flate2` with `zlib-ng`, `indicatif` (MultiProgress), `crossbeam-channel`, `parking_lot`.

---

## Global Constraints
- Must support all 4 combinations: import compressed, import plain, export compressed, export plain.
- Zero channel flooding: Use lock-free `AtomicU64` and `AtomicBool` for progress sampling.
- Graceful handling of `ErrorKind::BrokenPipe` without panicking.
- Initial 3-second grace period for ETA calculation to avoid caching spikes.

---

### Task 1: SIMD Gzip Acceleration & Buffered I/O Setup

**Files:**
- Modify: `Cargo.toml`
- Test: `cargo check` and `cargo test`

**Interfaces:**
- Consumes: `flate2` crate
- Produces: `flate2` with `zlib-ng` hardware SIMD feature enabled

- [ ] **Step 1: Update Cargo.toml with zlib-ng feature**
Replace line 28 in `Cargo.toml`:
```toml
flate2 = { version = "1.0", default-features = false, features = ["zlib-ng"] }
```

- [ ] **Step 2: Verify compilation and tests**
Run: `cargo check`
Run: `cargo test`
Expected: Clean build and 17 tests pass.

- [ ] **Step 3: Commit**
```bash
git add Cargo.toml Cargo.lock
git commit -m "build: enable zlib-ng SIMD acceleration for flate2"
```

---

### Task 2: Counting Stream Reader & Ingest State Machine

**Files:**
- Modify: `src/progress.rs`
- Test: Unit tests in `src/progress.rs`

**Interfaces:**
- Consumes: `std::io::Read`, `std::sync::atomic::{AtomicU64, AtomicBool}`
- Produces:
  - `pub struct CountingReader<R> { inner: R, bytes_read: Arc<AtomicU64> }`
  - `pub struct WorkerLiveState { ... }`
  - `pub fn calculate_eta(bytes_done: u64, total_bytes: u64, speed_bps: f64) -> Option<Duration>`
  - `pub fn format_duration_compact(d: Duration) -> String`

- [ ] **Step 1: Write unit tests for CountingReader and ETA calculation**
Add tests in `src/progress.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_counting_reader_tracks_exact_bytes() {
        let data = b"Hello, MySQL TurboLoad!";
        let counter = Arc::new(AtomicU64::new(0));
        let mut reader = CountingReader::new(Cursor::new(data), Arc::clone(&counter));
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut buf).unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), data.len() as u64);
    }

    #[test]
    fn test_eta_calculation_and_formatting() {
        // 50 MB done out of 100 MB at 10 MB/s -> 5 seconds ETA
        let eta = calculate_eta(50 * 1024 * 1024, 100 * 1024 * 1024, 10.0 * 1024.0 * 1024.0);
        assert_eq!(eta.map(|d| d.as_secs()), Some(5));
        assert_eq!(format_duration_compact(Duration::from_secs(65)), "01:05");
        assert_eq!(format_duration_compact(Duration::from_secs(3665)), "01:01:05");
    }
}
```

- [ ] **Step 2: Implement CountingReader, WorkerLiveState, and ETA helpers**
Add `CountingReader`, `WorkerLiveState`, and rate/ETA helpers in `src/progress.rs`.

- [ ] **Step 3: Run tests to verify they pass**
Run: `cargo test --lib progress`
Expected: All tests pass.

- [ ] **Step 4: Commit**
```bash
git add src/progress.rs
git commit -m "feat(progress): add CountingReader, WorkerLiveState, and ETA helpers"
```

---

### Task 3: Production Import Pipeline Stream Integration

**Files:**
- Modify: `src/runner.rs`
- Test: `cargo test`

**Interfaces:**
- Consumes: `CountingReader`, `WorkerLiveState` from `src/progress.rs`
- Produces: Live atomic byte tracking and commit phase signaling for both `.sql.gz` and `.sql` in `execute_tasks`.

- [ ] **Step 1: Integrate CountingReader and buffered streaming in `runner.rs`**
In `src/runner.rs`:
- Create `Arc<Vec<Arc<WorkerLiveState>>>` indexed by worker ID.
- Wrap input files in `BufReader::with_capacity(256 * 1024, file)`.
- Wrap in `CountingReader::new(buffered, live_state.bytes_current.clone())`.
- Increase feeder buffer to 256 KB (`vec![0u8; 256 * 1024]`).
- For `.sql`, use feeder thread with `CountingReader` instead of `Stdio::from(file)` so uncompressed files also report real-time bytes and speed.
- Handle `BrokenPipe` gracefully.
- Mark `live_state.is_committing.store(true, Ordering::SeqCst)` when streaming ends while child is running.

- [ ] **Step 2: Run tests and cargo check**
Run: `cargo check`
Run: `cargo test`
Expected: PASS.

- [ ] **Step 3: Commit**
```bash
git add src/runner.rs
git commit -m "feat(runner): implement buffered streaming and live byte tracking for import"
```

---

### Task 4: Production Export Pipeline Stream Integration

**Files:**
- Modify: `src/exporter.rs`
- Test: `cargo test`

**Interfaces:**
- Consumes: `CountingReader`, `WorkerLiveState`
- Produces: Live byte tracking and commit phase signaling for `.sql.gz` and `.sql` exports in `execute_export_tasks`.

- [ ] **Step 1: Integrate CountingReader and buffered output in `exporter.rs`**
In `src/exporter.rs`:
- Track live uncompressed stdout bytes from `mysqldump` through `CountingReader`.
- For `.sql.gz`: Pipe `CountingReader` into `GzEncoder` with `BufWriter::with_capacity(256 * 1024, file)`.
- For `.sql`: Pipe `CountingReader` into `BufWriter::with_capacity(256 * 1024, file)` via feeder thread.
- Handle `BrokenPipe` gracefully.
- Mark `is_committing` while waiting for output flush and child exit.

- [ ] **Step 2: Run tests and cargo check**
Run: `cargo test`
Expected: PASS.

- [ ] **Step 3: Commit**
```bash
git add src/exporter.rs
git commit -m "feat(exporter): implement buffered streaming and live byte tracking for export"
```

---

### Task 5: Real-Time UI MultiProgress Ticking & Critical-Path Overall ETA

**Files:**
- Modify: `src/progress.rs`
- Test: `cargo test` and manual dry-run verification

**Interfaces:**
- Consumes: `WorkerLiveState` atomics across all active workers
- Produces:
  - Worker spinner message showing `bytes/total (percent%) @ speed | ETA: MM:SS` or `[Committing in MySQL... (elapsed)]`
  - Main bar message showing `done/total MB | active workers | Overall ETA`

- [ ] **Step 1: Implement periodic UI sampling in `ProgressTracker`**
- In `ProgressTracker`, start a sampling timer/thread that ticks every 250ms.
- Read each active worker's `WorkerLiveState`.
- Calculate EWMA speed: $\text{speed} = 0.25 \times \text{instant} + 0.75 \times \text{prev}$.
- If elapsed < 3s, show `[Calculating ETA...]`.
- If speed == 0 for > 5s, show `[Stalled on DB I/O]`.
- If `is_committing`, show `100% [Committing in MySQL... (MM:SS)]`.
- Update main bar with critical-path overall ETA ($\max(\text{worker ETAs})$).

- [ ] **Step 2: Verify cleanly on exit**
Ensure UI timer cleanly stops when import/export finishes without thread leaks.

- [ ] **Step 3: Commit**
```bash
git add src/progress.rs
git commit -m "feat(ui): add 250ms sampling tick, EWMA speed, and critical-path overall ETA"
```

---

### Task 6: Verification & End-to-End Testing

**Files:**
- Modify: `tests/e2e_test.rs`
- Run: Full test suite and CLI dry-run

- [ ] **Step 1: Run comprehensive cargo test**
Run: `cargo test -- --nocapture`
Expected: All tests pass.

- [ ] **Step 2: Verify dry-run with import and export commands**
Run: `cargo run -- --help`
Run: `cargo run -- export --help`
Expected: Successful output, zero warnings.

- [ ] **Step 3: Commit all changes**
```bash
git add .
git commit -m "test: verify end-to-end progress tracking and gzip optimization"
```
