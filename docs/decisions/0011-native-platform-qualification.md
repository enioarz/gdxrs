# 0011 — Native macOS and Windows qualification

Task 017 is a user-directed M2 portability follow-up before starting M3. Native
CI covers Ubuntu x64, macOS 15 ARM64, macOS 15 Intel and Windows Server 2025 x64
MSVC. Verify rustc's actual host matches the requested runner target. The full
reader-check command runs all default/minimal/all-feature suites, doctests,
Clippy, fmt, dependency audits and complete frozen expected output comparisons.
Default-feature tests also run in the optimized release profile. It has no
native GDX, CMake or Unix resource-module runtime requirement.

Use lazy imports for separate oracle/benchmark/scale commands. Python text/tool
output is explicitly UTF-8; Git attributes preserve LF sources/expectations and
binary GDX bytes. Test helpers create their own ignored scratch parent so a plain
Cargo test does not depend on prior Python/setup execution. Platform tests use
caller-owned Unicode paths and trusted same-filesystem scratch parents. Hard-link
support remains required; explicit cleanup assertions close source/run owners
before removal, including on Windows. No filesystem/security guarantees are relaxed.

The native matrix qualifies reader functionality, resource-accounting invariants
and lifecycle. Multi-GB OS-capped scale/performance evidence remains Linux-only;
this task does not invent Windows Job Object or macOS memory containment. Frozen
independently specified records/labels and all malformed/injected failure checks
remain enabled. Record actual native runs before claiming compatibility.
