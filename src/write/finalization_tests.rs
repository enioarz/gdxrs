//! Exhaust the finish path's individual failure sites on a nonempty transaction.
use super::tests::{Parent, Reset, fault, options};
use super::*;
fn prepared(p: &Parent, compression: WriteCompression) -> Writer {
    let mut opts = options();
    opts.compression = compression;
    let mut w = Writer::create(p.destination(), opts).unwrap();
    w.begin_metadata(&MetadataOptions::new(p.destination().parent().unwrap()))
        .unwrap();
    let key = w.register_uel(b"key").unwrap();
    w.add_set_text(b"member").unwrap();
    w.add_acronym(AcronymId::new(7).unwrap(), b"Acro", b"text")
        .unwrap();
    let set = w
        .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
        .unwrap();
    w.add_comment(set, b"comment").unwrap();
    let mut spec = WriteSymbol::new(b"J", SymbolType::Alias, 1);
    spec.alias_target = Some(SymbolReference::Registered(set));
    let alias = w.register_symbol(&spec).unwrap();
    let p = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))
        .unwrap();
    w.set_domains(
        p,
        &[WriteDomain::Strict(SymbolReference::Registered(alias))],
    )
    .unwrap();
    let r = w
        .register_symbol(&WriteSymbol::new(b"R", SymbolType::Parameter, 1))
        .unwrap();
    w.set_domains(r, &[WriteDomain::Relaxed(b"future")])
        .unwrap();
    let mut out = w.begin_symbol(set, None).unwrap();
    out.write(&[key], &[Value::ZERO]).unwrap();
    out.finish().unwrap();
    let mut out = w.begin_symbol(p, None).unwrap();
    out.write(&[key], &[Value::Special(SpecialValue::Epsilon)])
        .unwrap();
    out.finish().unwrap();
    w
}
#[test]
fn every_nonempty_finish_io_site_prevents_partial_publication_and_retains_errors() {
    let _reset = Reset::new();
    for compression in [WriteCompression::None, WriteCompression::Zlib] {
        if compression == WriteCompression::Zlib && !cfg!(feature = "compression") {
            continue;
        }
        let mut events = vec![
            Event::Write,
            Event::Seek,
            Event::Sync,
            Event::Publish,
            Event::MetadataRead,
            Event::MetadataWrite,
            Event::MetadataSeek,
            Event::MetadataSync,
            Event::MetadataRemove,
        ];
        #[cfg(feature = "compression")]
        if compression == WriteCompression::Zlib {
            events.push(Event::Codec);
        }
        // With minimal features the list is still extended by a real output stage.
        events.push(Event::Remove);
        for event in events {
            let mut reached_success = false;
            for site in 0..20000 {
                let p = Parent::new();
                let mut w = prepared(&p, compression);
                fault(event, site, false, io::ErrorKind::StorageFull);
                match w.finish() {
                    Ok(_) => {
                        reached_success = true;
                        Reset::new();
                        drop(w);
                        fs::remove_file(p.destination()).unwrap();
                        break;
                    }
                    Err(error) => {
                        assert_eq!(w.statistics().status, WriteStatus::Failed);
                        let published = w.statistics().published;
                        assert_eq!(
                            published,
                            matches!(event, Event::Remove | Event::MetadataRemove)
                        );
                        assert_eq!(p.destination().exists(), published);
                        assert_eq!(w.finish().unwrap_err().kind, error.kind);
                        Reset::new();
                        w.abort().unwrap();
                        drop(w);
                        if published {
                            let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
                            assert_eq!(r.information().unwrap().symbol_count, 4);
                            drop(r);
                            fs::remove_file(p.destination()).unwrap();
                        }
                        p.empty();
                    }
                }
            }
            assert!(reached_success, "every site must be visited: {event:?}");
        }
    }
}
#[test]
fn nonempty_finalization_needs_no_new_root_capacity_and_preserves_late_collisions() {
    let _reset = Reset::new();
    for compression in [WriteCompression::None, WriteCompression::Zlib] {
        if compression == WriteCompression::Zlib && !cfg!(feature = "compression") {
            continue;
        }
        let p = Parent::new();
        let mut w = prepared(&p, compression);
        let stats = w.budget_statistics();
        let reserve = w
            .budget
            .reserve(stats.limit_bytes - stats.current_bytes)
            .unwrap();
        w.finish().unwrap();
        drop(reserve);
        drop(w);
        fs::remove_file(p.destination()).unwrap();
        p.empty();
        let mut w = prepared(&p, compression);
        fs::write(p.destination(), b"late owner").unwrap();
        assert!(w.finish().is_err());
        assert!(!w.statistics().published);
        w.abort().unwrap();
        assert_eq!(fs::read(p.destination()).unwrap(), b"late owner");
        fs::remove_file(p.destination()).unwrap();
        p.empty();
    }
}
