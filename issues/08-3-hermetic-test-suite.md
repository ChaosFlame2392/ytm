# 08-3: Test Suite Hermeticity & Guarded Integration Tests

**What to build:** Mark live external network tests in `tests/streaming_repro_test.rs` with `#[ignore]` so standard `cargo test` executes 100% offline, deterministically, and in under 3 seconds. Ensure offline mock integration tests cover stream resolution, IPC error handling, and prefetch failure resilience. Verify `cargo test -- --ignored` functions on-demand for manual or staging integration verification.

**Blocked by:** 08-1, 08-2

**Status:** completed

- [x] Add `#[ignore = "requires live internet connection and YouTube access"]` to network-hitting tests in `tests/streaming_repro_test.rs`.
- [x] Add hermetic mock tests in `tests/` verifying stream error handling and cookie resolution offline.
- [x] Ensure full `cargo test` runs completely offline in < 3s without hitting external network services.
- [x] Verify `cargo test -- --ignored` passes when network is available.
