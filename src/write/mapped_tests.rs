use super::*;
use tests::{Parent, Reset, fault, options};
fn caller(n: i64) -> MappedUelId {
    MappedUelId::new(n).unwrap()
}
fn limits(p: &Parent) -> MetadataOptions {
    let mut o = MetadataOptions::new(p.destination().parent().unwrap());
    o.maximum_open_files = 7;
    o.merge_fan_in = 2;
    o.run_bytes = 512;
    o
}
fn writer(p: &Parent) -> Writer {
    let mut w = Writer::create(p.destination(), options()).unwrap();
    w.begin_metadata(&limits(p)).unwrap();
    w
}
fn source(p: &Parent) -> Reader {
    let mut w = writer(p);
    let i = w
        .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
        .unwrap();
    let par = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))
        .unwrap();
    w.register_mapped_uels(
        [(9, b"z".as_slice()), (0, b"a"), (3, b"C"), (9, b"z")]
            .into_iter()
            .map(|(n, b)| Ok((caller(n), b))),
    )
    .unwrap();
    assert_eq!(w.raw_id_for_mapped(caller(0)).unwrap().get(), 2);
    let raw_c = w.raw_id_for_mapped(caller(3)).unwrap();
    assert_eq!(w.mapped_id_for_raw(raw_c).unwrap(), caller(3));
    let mut rows = w.begin_mapped_symbol(i).unwrap();
    for n in [3, 9, 0] {
        rows.write(&[caller(n)], &[Value::ZERO]).unwrap();
    }
    rows.finish().unwrap();
    let mut rows = w.begin_mapped_symbol(par).unwrap();
    for (keys, value) in [
        ([9, 0], Value::NumberBits(1.25f64.to_bits())),
        ([3, 9], Value::Special(SpecialValue::Undefined)),
        ([0, 3], Value::Special(SpecialValue::Epsilon)),
    ] {
        rows.write(&keys.map(caller), &[value]).unwrap();
    }
    rows.finish().unwrap();
    assert!(w.metadata_statistics().peak_open_files <= 7);
    w.finish().unwrap();
    Reader::open(
        p.destination(),
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap()
}
fn map(r: &Reader, p: &Parent) -> MappedUels {
    let mut m = r.mapped_uels(&limits(p)).unwrap();
    m.register(
        [(1, 9), (2, 0), (3, 3)]
            .into_iter()
            .map(|(r, m)| Ok((UelId::new(r).unwrap(), caller(m)))),
    )
    .unwrap();
    m
}
#[test]
fn mapped_write_and_read_order_zero_sparse_ids_values_and_raw_identity() {
    let p = Parent::new();
    let r = source(&p);
    let h = r.symbol(SymbolId::new(2).unwrap()).unwrap();
    let baseline = r.budget_statistics().current_bytes;
    let mut m = map(&r, &p);
    assert_eq!(m.len(), 3);
    assert_eq!(m.mapped(UelId::new(2).unwrap()).unwrap(), Some(caller(0)));
    assert_eq!(m.raw(caller(9)).unwrap(), Some(UelId::new(1).unwrap()));
    assert!(m.raw(caller(i32::MAX as i64)).unwrap().is_none());
    let mut c = r
        .scan_mapped(
            &h,
            &mut m,
            MappedReadOptions {
                batch_bytes: 40,
                ..MappedReadOptions::default()
            },
        )
        .unwrap();
    for (mapped, raw, value) in [
        ([0, 3], [2, 3], Value::Special(SpecialValue::Epsilon)),
        ([3, 9], [3, 1], Value::Special(SpecialValue::Undefined)),
        ([9, 0], [1, 2], Value::NumberBits(1.25f64.to_bits())),
    ] {
        let row = c.next().unwrap().unwrap();
        assert_eq!(row.mapped_keys, &mapped.map(caller));
        assert_eq!(row.raw_keys, &raw.map(|n| UelId::new(n).unwrap()));
        assert_eq!(row.values, &[value]);
    }
    assert!(c.next().unwrap().is_none());
    assert!(c.statistics().spill.complete);
    assert_eq!(c.statistics().decoded, 3);
    c.close().unwrap();
    drop(c);
    assert!(m.statistics().peak_open_files <= 7);
    let peak_handles = m.statistics().peak_open_files;
    m.close().unwrap();
    assert_eq!(m.statistics().peak_open_files, peak_handles);
    drop(m);
    assert_eq!(r.budget_statistics().current_bytes, baseline);
}
#[test]
fn missing_map_error_skip_and_mapped_limit_are_explicit() {
    let p = Parent::new();
    let r = source(&p);
    let h = r.symbol(SymbolId::new(2).unwrap()).unwrap();
    for skip in [false, true] {
        let mut m = r.mapped_uels(&limits(&p)).unwrap();
        m.register(
            [(1, 9), (3, 3)]
                .into_iter()
                .map(|(r, m)| Ok((UelId::new(r).unwrap(), caller(m)))),
        )
        .unwrap();
        let result = r.scan_mapped(
            &h,
            &mut m,
            MappedReadOptions {
                missing: if skip {
                    MissingMapping::SkipRecord
                } else {
                    MissingMapping::Error
                },
                ..MappedReadOptions::default()
            },
        );
        if skip {
            let mut c = result.unwrap();
            assert_eq!(c.statistics().skipped, 2);
            assert_eq!(
                c.next().unwrap().unwrap().raw_keys,
                &[UelId::new(3).unwrap(), UelId::new(1).unwrap()]
            );
            assert!(c.next().unwrap().is_none());
        } else {
            assert!(matches!(result,Err(ref e) if e.kind==ErrorKind::InvalidReference));
            drop(result);
        }
        m.abort().unwrap();
    }
    let mut m = map(&r, &p);
    for limit in [0, 1] {
        let mut c = r
            .scan_mapped(
                &h,
                &mut m,
                MappedReadOptions {
                    limit: Some(limit),
                    ..MappedReadOptions::default()
                },
            )
            .unwrap();
        if limit == 1 {
            assert_eq!(c.statistics().decoded, 3);
            assert_eq!(
                c.next().unwrap().unwrap().mapped_keys,
                &[caller(0), caller(3)]
            );
        } else {
            assert_eq!(c.statistics().decoded, 0);
        }
        assert!(c.next().unwrap().is_none());
        c.close().unwrap();
    }
    m.close().unwrap();
}
#[test]
fn map_collisions_idempotence_and_extreme_sparse_ids() {
    assert!(MappedUelId::new(-1).is_err());
    assert!(MappedUelId::new(i64::from(i32::MAX) + 1).is_err());
    let p = Parent::new();
    let r = source(&p);
    for pairs in [vec![(1, 7), (2, 7)], vec![(1, 7), (1, 8)]] {
        let mut m = r.mapped_uels(&limits(&p)).unwrap();
        assert_eq!(
            m.register(
                pairs
                    .into_iter()
                    .map(|(r, m)| Ok((UelId::new(r).unwrap(), caller(m))))
            )
            .unwrap_err()
            .kind,
            ErrorKind::InvalidReference
        );
        m.abort().unwrap();
    }
    let mut m = r.mapped_uels(&limits(&p)).unwrap();
    m.register(
        [(1, i32::MAX as i64), (1, i32::MAX as i64), (3, 0)]
            .into_iter()
            .map(|(r, m)| Ok((UelId::new(r).unwrap(), caller(m)))),
    )
    .unwrap();
    assert_eq!(m.len(), 2);
    assert_eq!(
        m.raw(caller(i32::MAX as i64)).unwrap(),
        Some(UelId::new(1).unwrap())
    );
    assert!(m.statistics().peak_temporary_bytes < 8192);
    m.close().unwrap();
}
#[test]
fn mapped_writer_duplicates_missing_shape_and_abandonment_are_terminal() {
    for mode in 0..5 {
        let p = Parent::new();
        let mut w = writer(&p);
        let id = w
            .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))
            .unwrap();
        w.register_mapped_uels([Ok((caller(7), b"x"))]).unwrap();
        let mut rows = w.begin_mapped_symbol(id).unwrap();
        if mode == 0 {
            assert!(rows.write(&[caller(8)], &[Value::ZERO]).is_err());
        }
        if mode == 1 {
            assert!(rows.write(&[], &[Value::ZERO]).is_err());
        }
        if mode == 2 {
            assert!(rows.write(&[caller(7)], &[]).is_err());
        }
        if mode == 3 {
            rows.write(&[caller(7)], &[Value::ZERO]).unwrap();
            rows.write(&[caller(7)], &[Value::NumberBits(1.0f64.to_bits())])
                .unwrap();
            assert!(rows.finish().is_err());
        } else {
            drop(rows);
        }
        assert!(w.finish().is_err());
        assert!(!w.statistics().published);
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn mapped_root_quota_handles_and_source_identity_fail_safely() {
    let p = Parent::new();
    let r = source(&p);
    let h = r.symbol(SymbolId::new(1).unwrap()).unwrap();
    let other = Parent::new();
    let r2 = source(&other);
    let foreign = r2.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut m = map(&r, &p);
    assert!(
        matches!(r2.scan_mapped(&h,&mut m,MappedReadOptions::default()),Err(e) if e.kind==ErrorKind::SourceMismatch)
    );
    assert!(
        matches!(r.scan_mapped(&foreign,&mut m,MappedReadOptions{limit:Some(0),..MappedReadOptions::default()}),Err(e) if e.kind==ErrorKind::SourceMismatch)
    );
    m.close().unwrap();
    for mode in 0..3 {
        let mut options = limits(&p);
        if mode == 0 {
            options.maximum_open_files = 6;
        }
        if mode == 1 {
            options.maximum_temporary_bytes = 128;
        }
        let held = if mode == 2 {
            Some(
                r.source
                    .budget
                    .reserve(
                        r.budget_statistics().limit_bytes - r.budget_statistics().current_bytes,
                    )
                    .unwrap(),
            )
        } else {
            None
        };
        let candidate = r.mapped_uels(&options);
        if mode == 2 {
            assert!(candidate.is_err());
        } else {
            let mut m = candidate.unwrap();
            assert!(
                m.register([Ok((UelId::new(1).unwrap(), caller(7)))])
                    .is_err()
            );
            m.abort().unwrap();
        }
        drop(held);
    }
}
#[test]
fn mapped_store_corruption_io_and_cleanup_failures_remain_abortable() {
    let _reset = Reset::new();
    let p = Parent::new();
    let r = source(&p);
    let mut m = map(&r, &p);
    // Corrupt the checked fixed-frame table after construction, including cached pages.
    let dir = fs::read_dir(p.destination().parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .unwrap();
    fs::write(dir.join("mapped-uels"), b"bad").unwrap();
    assert!(m.mapped(UelId::new(1).unwrap()).is_err());
    m.abort().unwrap();
    for read in [false, true] {
        let mut m = r.mapped_uels(&limits(&p)).unwrap();
        let io = crate::spill::test_io_failure(read);
        let cleanup = if read {
            None
        } else {
            Some(crate::spill::test_cleanup_failure())
        };
        fault(
            Event::MetadataRemove,
            usize::from(read),
            true,
            io::ErrorKind::PermissionDenied,
        );
        let error = m
            .register([Ok((UelId::new(1).unwrap(), caller(7)))])
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Io);
        assert_eq!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap()
                .kind(),
            io::ErrorKind::StorageFull
        );
        assert!(error.cleanup_error().is_some());
        assert!(m.abort().is_err());
        drop(io);
        drop(cleanup);
        Reset::new();
        m.abort().unwrap();
    }
}

#[test]
fn mapped_partial_error_preserves_secondary_cleanup_and_never_replays_rows() {
    let _reset = Reset::new();
    let p = Parent::new();
    let r = source(&p);
    let h = r.symbol(SymbolId::new(2).unwrap()).unwrap();
    let mut m = map(&r, &p);
    let mut cursor = r
        .scan_mapped(&h, &mut m, MappedReadOptions::default())
        .unwrap();
    assert_eq!(
        cursor.next().unwrap().unwrap().mapped_keys,
        &[caller(0), caller(3)]
    );
    let cleanup = crate::spill::test_cleanup_failure();
    fault(
        Event::MetadataRead,
        0,
        true,
        io::ErrorKind::PermissionDenied,
    );
    let error = match cursor.next() {
        Err(error) => error,
        Ok(_) => panic!("mapped lookup must fail"),
    };
    assert_eq!(error.kind, ErrorKind::Io);
    assert!(error.cleanup_error().is_some());
    assert_eq!(cursor.statistics().returned, 1);
    assert!(matches!(cursor.next(),Err(e) if e.kind==ErrorKind::Io));
    drop(cursor);
    drop(cleanup);
    Reset::new();
    m.abort().unwrap();
}
