# Domain and data-error API status (task 027)

Reference: GAMS-dev/gdx `9e2a8c8932412010cd7d2f94727d1740143bf3da`,
`src/gxfile.cpp`, `src/gdxapi.yaml`. The development-only `tools/domains` driver
links this source and preserves the frozen generator verbatim.

| Native operation/source | Rust equivalent and policy |
| --- | --- |
| `gdxSymbolSetDomain` (2509), `DoWrite` (1234) | Strict domains retain direct alias IDs after chain validation. Membership uses completed disk-backed one-dimensional sets. Universe imposes no restriction. Direct set self-domain skips membership checks, as native does. |
| `gdxSymbolSetDomainX` (2600) | Relaxed domain names are metadata only; no membership restriction. |
| `gdxSymbolGetDomain`, `gdxSymbolGetDomainX` | Existing streamed reader domain metadata preserves direct referenced IDs/names. |
| `gdxDataErrorCount` (2197) | Explicit phase/read error statistics, session-local. Counts distinguish detected rejections from successfully delivered rows. |
| `gdxDataErrorRecordX` (2215) | Borrowed `DataErrorRecord`, one-based original raw IDs, bad-axis bit mask, typed values, input ordinals and provenance. No negative raw-ID sentinel. |
| `gdxDataErrorRecord` (2202) | Same diagnostic's raw keys already contain absolute original IDs. |
| `gdxSetErrorRecordCutoff` (2235) | Explicit maximum records AND logical payload bytes. Exceeding either fails before the next callback; no silent truncation. |
| `AddToErrorListDomErrs` (1514) | Native coalesces rows unless an unseen `(axis,bad key)` appears. Rust's opt-in stream reports every rejected occurrence. |
| `AddToErrorList` (1547), `DoWrite` | Domain checks precede duplicates. Rust stable sorting makes first valid input win; every later duplicate is reported. Existing writer calls continue to reject terminally. |
| Mapped-read missing IDs | `scan_mapped_reporting` reports all missing axes and partial mapping provenance before applying explicit Error/SkipRecord policy. |
| Forward/deferred membership | Bounded disk staging and dependency passes; no native call-state emulation. Unresolvable membership cycles fail explicitly. |
| Native last-error numbers, tracing, random access to a retained error list | Typed Rust errors and callback adapter; no native C call-state/error-number ABI. A future C compatibility layer would be separate. |

Error lists reset at native symbol start and do not survive publication. Our
separate native helper captures them immediately after DataWriteDone and probes
reset, signed versus absolute keys, cutoff and coalescing. Native error-list
ordinals differ from original input ordinals; the oracle recipe has independent
expected streams for each. Rust includes the repeated domain failure that native
coalesces. Both implementations must match the same complete accepted metadata
and record expectations for strict aliases, direct self, relaxed and universe
cases. Forward-input Rust writes use the same registration IDs while staging P
before its domain set I. See [decision 0021](decisions/0021-domain-error-streams.md).
