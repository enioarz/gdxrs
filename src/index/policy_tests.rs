use super::policy::*;
use crate::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        fs::create_dir_all(".work").unwrap();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = PathBuf::from(".work").join(format!(
            "policy-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn measurements() -> ScanMeasurements {
    ScanMeasurements {
        rows: 1_000_000,
        selected_rows: 1_000,
        session_valid: true,
        serial_full_scan_seconds: 10.0,
        indexed_selected_seconds: 1.0,
        parallel_full_scan_seconds: 4.0,
        strict_reopen_seconds: 2.0,
        build_seconds: 20.0,
    }
}

fn policy() -> ScanPolicy {
    ScanPolicy::default()
}

#[test]
fn policy_prefers_measured_cheapest_allowed_choice() {
    let plan = policy().plan(&measurements(), None).unwrap();
    assert_eq!(plan.choice, ScanChoice::Indexed);
    assert_eq!(plan.estimated_seconds, 1.0);
    assert_eq!(plan.reopen_seconds_charged, 0.0);
    assert_eq!(plan.build_break_even_queries, Some(3));
    assert!(
        plan.alternatives
            .iter()
            .all(|a| a.allowed || !a.reason.is_empty())
    );
}

#[test]
fn policy_charges_strict_reopen_and_never_builds_without_a_session() {
    let mut m = measurements();
    m.session_valid = false;
    let p = ScanPolicy {
        allow_strict_reopen: false,
        ..policy()
    };
    let plan = p.plan(&m, None).unwrap();
    assert_eq!(plan.choice, ScanChoice::Serial);
    assert_eq!(plan.reopen_seconds_charged, 0.0);
    assert_eq!(plan.build_break_even_queries, None);
    assert!(!plan.alternatives[1].allowed && !plan.alternatives[2].allowed);
    let p = ScanPolicy {
        allow_strict_reopen: true,
        ..policy()
    };
    let plan = p.plan(&m, None).unwrap();
    assert_eq!(plan.choice, ScanChoice::Indexed);
    assert_eq!(plan.estimated_seconds, 3.0);
    assert_eq!(plan.reopen_seconds_charged, 2.0);
}

#[test]
fn policy_is_conservative_for_head_zero_and_small_inputs() {
    for limit in [Some(0), Some(13), Some(1024)] {
        let plan = policy().plan(&measurements(), limit).unwrap();
        assert_eq!(plan.choice, ScanChoice::Serial);
    }
    let mut m = measurements();
    m.rows = 100;
    m.selected_rows = 100;
    let plan = policy().plan(&m, None).unwrap();
    assert_eq!(plan.choice, ScanChoice::Serial);
    assert_eq!(plan.alternatives[0].reason, "small input");
}

#[test]
fn policy_rejects_indexed_for_poor_pruning_and_parallel_under_pressure() {
    let mut m = measurements();
    m.selected_rows = 900_000;
    let plan = policy().plan(&m, None).unwrap();
    assert!(!plan.alternatives[1].allowed);
    assert_eq!(plan.alternatives[1].reason, "poor pruning");
    assert_eq!(plan.choice, ScanChoice::Parallel { workers: 8 });
    let mut m = measurements();
    m.parallel_full_scan_seconds = 12.0;
    let plan = policy().plan(&m, None).unwrap();
    assert!(!plan.alternatives[2].allowed);
    assert_eq!(plan.choice, ScanChoice::Indexed);
    let p = ScanPolicy {
        required_parallel_advantage: 3.0,
        ..policy()
    };
    let m = measurements();
    let plan = p.plan(&m, None).unwrap();
    assert!(!plan.alternatives[2].allowed);
}

#[test]
fn policy_honors_explicit_overrides_and_validates_limits() {
    for choice in [
        ScanChoice::Serial,
        ScanChoice::Indexed,
        ScanChoice::Parallel { workers: 3 },
    ] {
        let p = ScanPolicy {
            choice_override: Some(choice),
            ..policy()
        };
        let plan = p.plan(&measurements(), None).unwrap();
        assert_eq!(plan.choice, choice);
    }
    let p = ScanPolicy {
        choice_override: Some(ScanChoice::Parallel { workers: 8 }),
        max_workers: 4,
        ..policy()
    };
    assert!(p.plan(&measurements(), None).is_err());
    for bad in [
        ScanPolicy {
            max_workers: 0,
            ..policy()
        },
        ScanPolicy {
            max_workers: 9,
            ..policy()
        },
        ScanPolicy {
            min_prune_fraction: 0.0,
            ..policy()
        },
        ScanPolicy {
            min_prune_fraction: 1.5,
            ..policy()
        },
        ScanPolicy {
            required_parallel_advantage: 0.5,
            ..policy()
        },
    ] {
        assert!(bad.plan(&measurements(), None).is_err());
    }
    let mut m = measurements();
    m.selected_rows = m.rows + 1;
    assert!(policy().plan(&m, None).is_err());
    m.selected_rows = 1;
    m.serial_full_scan_seconds = f64::NAN;
    assert!(policy().plan(&m, None).is_err());
}

fn planned_reader() -> (Temp, Reader) {
    let temp = Temp::new();
    let reader = Reader::open(
        if cfg!(feature = "compression") {
            "tests/fixtures/v7/blocks.gdx"
        } else {
            "tests/fixtures/v7/key-widths.gdx"
        },
        OpenOptions {
            memory_budget_bytes: 64 * 1024 * 1024,
            metadata_cache_bytes: 0,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    (temp, reader)
}

fn digest(cursor: &mut PlannedScan) -> (u64, u64) {
    let mut count = 0u64;
    let mut digest = 0xcbf29ce484222325u64;
    while let Some(batch) = cursor.next_batch().unwrap() {
        for at in 0..batch.len() {
            let row = batch.row(at).unwrap();
            for d in 0..20 {
                if let Some(key) = row.key(d) {
                    digest = (digest.rotate_left(7) ^ u64::from(key.get()))
                        .wrapping_mul(0x9e3779b185ebca87);
                }
            }
            for field in ValueField::ALL.iter() {
                if let Some(value) = row.value(*field) {
                    let bits = value.number_bits().unwrap_or(0);
                    digest = (digest.rotate_left(7) ^ bits).wrapping_mul(0x9e3779b185ebca87);
                }
            }
            count += 1;
        }
    }
    (count, digest)
}

fn ready_open(reader: &Reader, symbol: &SymbolHandle, temp: &Temp, name: &str) -> IndexOpen {
    let path = temp.0.join(name);
    let mut builder = reader
        .index_builder(
            symbol,
            path.clone(),
            IndexOptions {
                record_stride: 1,
                decoded_byte_stride: 17,
                ..IndexOptions::default()
            },
        )
        .unwrap();
    let built = builder.finish().unwrap();
    assert!(built.complete && built.published);
    reader
        .open_index(
            symbol,
            &path,
            IndexOptions {
                record_stride: 1,
                decoded_byte_stride: 17,
                ..IndexOptions::default()
            },
            IndexPolicy::UseIfValid,
        )
        .unwrap()
}

#[test]
fn planned_scans_preserve_exact_results_across_every_choice() {
    let (temp, reader) = planned_reader();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let _ = &symbol;
    let clauses = [DimensionSelection {
        dimension: 0,
        keys: KeySelection::One(UelId::new(4000).unwrap()),
    }];
    let selection = reader.selection(&symbol, &clauses).unwrap();
    let empty = reader.selection(&symbol, &[]).unwrap();
    let mut results = Vec::new();
    let mut built = 0;
    for choice in [
        ScanChoice::Serial,
        ScanChoice::Indexed,
        ScanChoice::Parallel { workers: 2 },
    ] {
        for (name, selection, limit) in [
            ("all", &empty, None),
            ("selected", &selection, None),
            ("head", &empty, Some(3)),
        ] {
            built += 1;
            let open = ready_open(&reader, &symbol, &temp, &format!("index-{built}"));
            let policy = ScanPolicy {
                choice_override: Some(choice),
                ..ScanPolicy::default()
            };
            let (plan, mut scan) = reader
                .scan_planned(
                    &symbol,
                    selection,
                    ScanOptions {
                        limit,
                        ..reader.scan_options()
                    },
                    Some(open),
                    &policy,
                    &measurements(),
                )
                .unwrap();
            assert_eq!(plan.choice, choice);
            results.push((choice, name, digest(&mut scan)));
        }
    }
    for name in ["all", "selected", "head"] {
        let row = results
            .iter()
            .filter(|(_, n, _)| *n == name)
            .collect::<Vec<_>>();
        assert_eq!(row.len(), 3);
        assert!(
            row.windows(2).all(|w| w[0].2 == w[1].2),
            "choice changed exact results for {name}"
        );
    }
    let open = ready_open(&reader, &symbol, &temp, "index-final");
    let policy = ScanPolicy::default();
    assert!(
        reader
            .scan_planned(
                &symbol,
                &empty,
                reader.scan_options(),
                None,
                &ScanPolicy {
                    choice_override: Some(ScanChoice::Indexed),
                    ..policy
                },
                &measurements(),
            )
            .is_err()
    );
    assert!(
        reader
            .scan_planned(
                &symbol,
                &empty,
                reader.scan_options(),
                Some(open),
                &ScanPolicy {
                    choice_override: Some(ScanChoice::Parallel { workers: 2 }),
                    ..policy
                },
                &measurements(),
            )
            .is_ok()
    );
}
