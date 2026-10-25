# 016 throughput follow-up — checked scalar buffer reads

Status: **complete in the same task/PR**, following the user's request to address
the measured read gap now. The measured wide, uncompressed read gap is closed
without an architectural rewrite. This is not a general parity claim for every
format/workload. [Original qualification](016.md) and its slower timings remain
historical evidence; [new raw results](016-throughput-results.json) retain the
fresh full qualification at `95f8003ea6f7aea353a8e1576690a4390d224be1`.

## Change and preserved invariants

`Stream::array<N>` now copies a scalar directly from the existing decoded buffer
when all N bytes are available. Checked slice access proves the range; checked
logical-position arithmetic and exact byte copying preserve the cursor state.
Tiny scalar wrappers are inline candidates, allowing constant-width parsing to
avoid repeated general-purpose byte-read loops/calls. The general `read_exact`
path still handles boundary crossings and refills, including full codec/checksum
validation. No fields, values, keys, frame validation or limits are skipped.

The production change is 23 added lines and one replacement. It adds no buffer,
dependency, unsafe code, concurrency, index or API change. It does not alter file
compatibility, budgets or accepted output. Existing codec/bad-input tests remain
mandatory. A focused new unit test compares scalar reads against the general path
and independently authored bytes at **14 offsets × four widths × two modes = 112**
raw/stored-block boundary configurations (56 raw configurations without compression).
It checks following bytes, logical/physical/decoded checkpoint positions, empty
reads at EOF, errors at EOF and budget release, using short positioned reads.

## Measurements

Same deterministic input SHA256, seed, 20 million 20-dimensional rows/five value
fields, full per-field validation and ordered digest `d9aa8da2bf4ef5a8`. The
fresh native comparison reads the same new artifact. All six generated input hashes
match the original qualification's files. All workers use the same 256 MiB root /
512 MiB hard/soft Linux address-space limits and allocation negative control.

| Full validating read | Median wall | Range | Standard deviation | Median user CPU |
| --- | ---: | ---: | ---: | ---: |
| Original Rust, `5ec3569` | 6.827 s | 6.825–7.130 s | 0.176 s | 6.593 s |
| Optimized Rust, `95f8003` | **3.946 s** | 3.894–4.047 s | 0.078 s | **3.719 s** |
| Fresh pinned native comparison | **4.604 s** | 4.399–4.902 s | 0.253 s | **4.371 s** |

Three trials per row. Rust wall time falls **42.2%** against its previous result;
its fresh median is **14.3% lower than native** on this workload. The scalar-buffer
fast path and targeted inlining are the only production-code changes between
qualified revisions. This supports removing avoidable per-scalar read overhead
here; it does not isolate the separate contribution of each inline annotation.

Decoded work and output validation are identical: **20,000,000 decoded/returned
rows**, **2,520,033,217 source bytes**, **2,520,033,077 decoded-buffer bytes**, and
**76,908 fills** for each full Rust scan. Leading/non-leading slices still decode
all rows; none of this improvement is attributed to data pruning. New source and
input hashes were reverified after the complete run.

Same AMD EPYC 9V74 / five-visible-CPU Linux VM, Rust/Cargo 1.99.1 and GCC 14.2.0,
pinned GDX/zlib as in the original evidence. Trial zero uses fsync plus advisory
POSIX_FADV_DONTNEED, subsequent trials reuse OS cache, and every process has empty
library state/no index. This does not prove cold physical-device cache or a portable
speed guarantee. The native median varies between runs; the new native comparison
is retained rather than reusing only the older 4.452 s value.

## Full qualification and regression verification

`python3 tools/dev.py m2-scale-check --profile scale` passed at `95f8003`, including:

- All **42** validating/benchmark trials, **six** generators and the actual-cap
  allocation negative control. Both input-growth comparisons pass unchanged bounds.
- Separate 2.52 GB wide-record and 2.30 GB metadata inputs, 36 million labels,
  9,562,501 externally resolved requests, large disk membership, observed domains,
  exact limits, pages, multi-pass merges and explicit cancellation/cleanup.
- Maximum recorded root **8,849,752 bytes** and maximum process RSS
  **13,369,344 bytes**, with the same measurement scope/fork-startup caveat.
  Post-run audit verifies every recorded root/disk/handle bound and complete cleanup.
- Default/all-feature suites each **46 unit + 31 integration + six doctests**;
  no-default **41 unit + 31 integration + six doctests**. Two doctests compile
  no-run examples; four check borrowing compile failures. No failed/ignored tests.
- Full **141,358-record / ten-fixture** independent corpus, all prior M2 paths,
  both stored/zlib boundary probes and **39 Python tooling tests**. Fmt, Clippy
  warnings-denied, locked dependency audits and declared MSRV pass.

Full report: `.work/reports/m2-scale-check-f0ad53a3318c4a379f0a0e4efc035302.json`.
Artifacts: `.work/scale/32c0372fcd1a4fab9cbd061cf075e653/`.
Fresh regression reports:
`reader-check-a3f010a979c545eeaa8b7c9f0ac3d0a3`,
`m0-check-c96c03a233784ecaaf9074492f3144a7`,
`label-pages-check-5634b8032ac949459f7ff30d7416aaec`.

Author review checked the contiguous scalar-range proof, unchanged refill and
integrity behavior, position advancement, terminal error/partial-output behavior,
block-edge and EOF tests, feature/endian coverage, identical decoded work, memory
bounds and fresh native timing. No acceptance criterion or expected result was
weakened. No further production change is needed to close this measured gap.
Broader workloads, especially compressed decoding, still need their own performance
measurements; platform and other scope limitations from task 016 remain in force.
