# Technical Design: Large Item Progress Tracking, ETA Estimation & Gzip Stream Acceleration

**Date**: 2026-09-18  
**Topic**: High-Performance Real-Time Progress, Throughput & Time Estimation, and Gzip SIMD I/O Acceleration for MySQL TurboLoad  
**Status**: Approved by User  

---

## 1. Problem Statement & Motivation

During parallel bulk import and export operations involving large tables (e.g. 500 MB – 50+ GB), the existing CLI interface in `mysql-turboload` presents significant operational blindspots:
1. **Worker Progress Invisibility**: Workers processing large items display a static spinner message `[W1] db / table.sql.gz (688.8 MB)` for 30–90+ minutes without reporting intermediate bytes read, ingest speed, elapsed task time, or estimated completion time (ETA).
2. **Main Progress Bar Freezes at 99%**: The main progress bar only increments task counts and bytes when an entire task finishes. In skewed workloads where 105 small tasks finish quickly and 1 large task runs for an hour, the overall bar stays frozen at 99% without an accurate overall ETA.
3. **Commit Phase Blindspot**: When all bytes have been piped into MySQL, the worker appears "stalled" at 100% while MySQL flushes logs, checks constraints, or builds secondary indexes.
4. **Gzip I/O & Decompression Bottleneck**:
   - `flate2::read::GzDecoder` was initialized directly on an unbuffered `File`, generating millions of small `ReadFile` syscalls on Windows.
   - Default `flate2` backend (`miniz_oxide`) is single-threaded and lacks SIMD acceleration.
   - The stdin pipe write buffer was fixed at 64 KB, leading to excessive pipe context switches.

---

## 2. Architecture & Design

### 2.1 Scope of Operations
The solution applies symmetrically across **all 4 execution paths**:
* **Import Compressed (`.sql.gz`)**: `File` -> `BufReader` (256 KB) -> `CountingReader` -> `zlib-ng` SIMD Decompression -> `mysql` stdin (256 KB pipe buffer).
* **Import Plain (`.sql`)**: `File` -> `BufReader` (256 KB) -> `CountingReader` -> `mysql` stdin (256 KB pipe buffer).
* **Export Compressed (`.sql.gz`)**: `mysqldump` stdout -> `CountingReader` -> `zlib-ng` SIMD GzEncoder (128 KB) -> `BufWriter` (256 KB) -> Output file.
* **Export Plain (`.sql`)**: `mysqldump` stdout -> `CountingReader` -> `BufWriter` (256 KB) -> Output file.

---

### 2.2 Live Byte-Tracking & State Architecture

#### Zero-Overhead Lock-Free Atomics
Instead of sending thousands of IPC channel events per second across threads, each active worker maintains a shared state struct:
```rust
pub struct WorkerLiveState {
    pub worker_id: usize,
    pub database: String,
    pub file_name: String,
    pub total_bytes: u64,
    pub bytes_current: AtomicU64,
    pub start_time: Instant,
    pub is_committing: AtomicBool,
}
```

* **Feeder Thread**: Calls `bytes_current.fetch_add(n, Ordering::Relaxed)` inside the streaming loop.
* **BrokenPipe & Error Handling**: If `mysql` or `mysqldump` exits prematurely, the feeder thread traps `ErrorKind::BrokenPipe` without panicking, cleanly closes the stream, and yields control back to child process exit status capture.

---

### 2.3 Throughput & ETA Calculation

#### Exponential Moving Average (EWMA) Throughput
To avoid wild spikes (initial OS buffer caching) or lagging cumulative averages:
* **Sampling Cadence**: 250 ms UI tick.
* **Rolling Speed**:
  Speed_new = alpha * InstantSpeed + (1 - alpha) * Speed_prev, with alpha = 0.25
* **Warm-up Grace Period**: For the first 3 seconds of any task, display `[Calculating ETA...]` instead of noisy extrapolated values.
* **Stall Detection**: If `InstantSpeed == 0` for > 5 seconds while `bytes_current < total_bytes`, report `[Stalled on DB I/O]` rather than `ETA: infinite`.

#### Multi-Phase Lifecycle Display
* **Streaming Phase (`bytes_current < total_bytes`)**:
  `[W1] 9837987 / operation.sql.gz: 420.5/688.8 MB (61.0%) @ 1.8 MB/s | ETA: 02:29`
* **Finalizing / Commit Phase (`bytes_current >= total_bytes` & child process running)**:
  `[W1] 9837987 / operation.sql.gz: 688.8 MB (100%) [Finalizing in MySQL... (01:14)]`
* **Completed**:
  `[W1] operation.sql.gz (688.8 MB in 52:10) - next task...`

#### Critical-Path Bounded Overall ETA
Overall ETA displayed in the main progress bar takes the maximum of the active workers' remaining times and the remaining unassigned queue throughput:
ETA_overall = max(max(ETA_workers), Unassigned_Bytes / sum(Worker_Speeds))

---

### 2.4 Gzip Stream Acceleration

#### 1. SIMD Acceleration with `zlib-ng`
Update `Cargo.toml`:
```toml
flate2 = { version = "1.0", default-features = false, features = ["zlib-ng"] }
```
Replaces `miniz_oxide` with C-based SIMD assembly (AVX2/SSE4.2/NEON), boosting raw decompression throughput from ~70 MB/s to 350-500+ MB/s per core.

#### 2. Buffered I/O on Disk & Pipe
* **Input Buffer**: `BufReader::with_capacity(256 * 1024, file)` eliminates Windows `ReadFile` kernel transition overhead.
* **Pipe Transfer Buffer**: Increase transfer chunks from 64 KB to 256 KB to match MySQL's `--net_buffer_length=1M` and maximize kernel pipe throughput.

---

## 3. Testing & Verification Strategy

1. **Compilation & Dependency Check**: `cargo check` and `cargo test` to verify `flate2` with `zlib-ng` compiles cleanly on Windows.
2. **Unit Tests**:
   - `CountingReader` test verifying exact byte accuracy.
   - ETA calculation tests (normal progress, stall handling, commit phase transition, zero-division safety).
3. **Integration Verification**:
   - Dry-run verification on sample `.sql` and `.sql.gz` files.
   - Validate that progress ticks smoothly and cleanly formats speeds, ETAs, and commit states.
