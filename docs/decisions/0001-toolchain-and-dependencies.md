# 0001 — Initial toolchain and dependencies

Rust 1.99.1 is the stable release published in the official distribution manifest dated 2026-10-15, observed during task 001. Pin that exact release, use edition 2024, and initially declare the same MSRV to avoid promising an untested older compiler. Record future MSRV changes separately.

At bootstrap the core had no dependencies; decision 0004 records the subsequent audited M1 codec exception. Python's standard library supports development reporting and later oracle management. Compression is deferred until a codec task can audit and measure `miniz_oxide`; no speculative dependency or feature is added at bootstrap. The production library forbids unsafe code.

The cloud toolchain is installed from official component archives with SHA-256 values verified against that manifest, under an ignored local prefix. No system directories or proxy/TLS settings are changed. The manifest hashes are retained in task 001 evidence; toolchain artifacts are not committed.
