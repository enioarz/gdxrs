use super::*;
use crate::source::{Counts, ReadAt};
use std::{
    io,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

struct Bytes {
    data: Arc<Mutex<Vec<u8>>>,
    changed: Arc<AtomicBool>,
    fail: Arc<AtomicU64>,
    interrupt: AtomicBool,
}
impl ReadAt for Bytes {
    fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<usize> {
        if self.interrupt.swap(false, Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if at >= self.fail.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let data = self.data.lock().unwrap();
        let rest = data.get(at as usize..).unwrap_or(&[]);
        let n = out.len().min(rest.len()).min(7);
        out[..n].copy_from_slice(&rest[..n]);
        Ok(n)
    }
    fn unchanged(&self) -> io::Result<bool> {
        Ok(!self.changed.load(Ordering::Relaxed))
    }
}
struct Input {
    reader: Reader,
    symbol: SymbolRecord,
    data: Arc<Mutex<Vec<u8>>>,
    changed: Arc<AtomicBool>,
    fail: Arc<AtomicU64>,
}
fn bits(n: u64, aligned: bool) -> u64 {
    0x3ff0000000000000 | if aligned && n <= 3 { 0 } else { n }
}
fn input(mode: u8, order: Order, chunk: usize, aligned: bool) -> Input {
    let template = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        crate::OpenOptions::default(),
    )
    .unwrap();
    let mut header = crate::format::Header::read(&template.source).unwrap();
    let mut symbol = template
        .lookup_record(crate::SymbolId::new(1).unwrap())
        .unwrap();
    // Independent primitive recipe, not a new on-disk fixture: 5,000 parameter
    // rows, explicit first key then compact deltas, each ordinary numeric bit.
    let mut decoded = vec![6];
    decoded.extend_from_slice(b"_DATA_");
    decoded.push(1);
    fn i32bytes(n: i32, order: Order) -> [u8; 4] {
        match order {
            Order::Little => n.to_le_bytes(),
            Order::Big => n.to_be_bytes(),
        }
    }
    decoded.extend_from_slice(&i32bytes(-1, order));
    decoded.extend_from_slice(&i32bytes(1, order));
    decoded.extend_from_slice(&i32bytes(5000, order));
    for n in 1..=5000 {
        if n == 1 {
            decoded.extend_from_slice(&[1, 0, 0]);
        } else {
            decoded.push(2);
        }
        if aligned && n <= 3 {
            decoded.push(6);
        } else {
            decoded.push(10);
            decoded.extend_from_slice(&match order {
                Order::Little => bits(n, aligned).to_le_bytes(),
                Order::Big => bits(n, aligned).to_be_bytes(),
            });
        }
    }
    decoded.push(255);
    let mut data = Vec::new();
    if mode == 0 {
        data = decoded;
    } else {
        for part in decoded.chunks(chunk) {
            let payload = if mode == 1 {
                part.to_vec()
            } else {
                #[cfg(feature = "compression")]
                {
                    miniz_oxide::deflate::compress_to_vec_zlib(part, 1)
                }
                #[cfg(not(feature = "compression"))]
                {
                    panic!("compressed test requires feature")
                }
            };
            data.push(if mode == 1 { 0 } else { 1 });
            data.extend_from_slice(&(payload.len() as u16).to_be_bytes());
            data.extend_from_slice(&payload);
        }
    }
    let length = data.len() as u64;
    header.order = order;
    header.offsets[0] = length;
    header.data_start = 0;
    symbol.position = 0;
    symbol.dimension = 1;
    symbol.kind = SymbolType::Parameter;
    symbol.user = 0;
    symbol.count = 5000;
    symbol.compressed = mode != 0;
    let data = Arc::new(Mutex::new(data));
    let changed = Arc::new(AtomicBool::new(false));
    let fail = Arc::new(AtomicU64::new(u64::MAX));
    let source = Source::testing(
        Box::new(Bytes {
            data: Arc::clone(&data),
            changed: Arc::clone(&changed),
            fail: Arc::clone(&fail),
            interrupt: AtomicBool::new(true),
        }),
        length,
    );
    assert!(source.header.set(header).is_ok());
    assert!(
        source
            .counts
            .set(Counts {
                symbols: 1,
                uels: 5000
            })
            .is_ok()
    );
    Input {
        reader: Reader { source },
        symbol,
        data,
        changed,
        fail,
    }
}
#[test]
fn full_state_restores_raw_stored_zlib_crossings_and_same_block_points() {
    let mut checked = 0;
    let mut exact_end = false;
    let mut crossing = false;
    for mode in 0..=2 {
        if mode != 0 && !cfg!(feature = "compression") {
            continue;
        }
        for order in [Order::Little, Order::Big] {
            for chunk in if mode == 0 {
                vec![32768]
            } else {
                vec![13, 32768]
            } {
                for aligned in [false, true] {
                    let input = input(mode, order, chunk, aligned);
                    let budget = Arc::clone(&input.reader.source.budget);
                    let base = budget.statistics().current_bytes;
                    let mut decoder = Decoder::new(&input.reader, &input.symbol).unwrap();
                    for ordinal in 0..=5000 {
                        if ordinal < 3
                            || ordinal % 97 == 0
                            || (3273..=3280).contains(&ordinal)
                            || ordinal == 4000
                            || ordinal >= 4999
                        {
                            let point = decoder.checkpoint().unwrap();
                            assert_eq!(point.row_ordinal(), ordinal);
                            if mode != 0 && chunk == 32768 {
                                if aligned && ordinal == 3277 {
                                    assert_eq!(point.stream.unwrap().decoded, 32768);
                                    exact_end = true;
                                }
                                if !aligned && ordinal == 3274 {
                                    assert_eq!(point.stream.unwrap().decoded, 32762);
                                    crossing = true;
                                }
                            }
                            let before_io = input.reader.io_statistics().bytes_read;
                            let mut suffix =
                                Decoder::resume(&input.reader, &input.symbol, &point).unwrap();
                            for next in ordinal + 1..=(ordinal + 3).min(5000) {
                                let row = suffix.next().unwrap().unwrap();
                                assert_eq!(row.key(0).unwrap().get() as u64, next);
                                assert_eq!(row.values[0], Value::NumberBits(bits(next, aligned)));
                            }
                            if ordinal == 4000 {
                                let bytes = input.reader.io_statistics().bytes_read - before_io;
                                if mode != 0 && chunk == 13 {
                                    assert!(
                                        bytes < 2048,
                                        "read the compressed record prefix: {bytes}"
                                    );
                                }
                                if mode == 0 {
                                    assert!(
                                        bytes
                                            <= 32768 + input.reader.source.length
                                                - point.stream.unwrap().physical,
                                        "read the raw record prefix: {bytes}"
                                    );
                                }
                            }
                            let next_point = suffix.checkpoint().unwrap();
                            assert_eq!(next_point.emitted, (ordinal + 3).min(5000));
                            if ordinal + 3 <= 5000 {
                                assert_eq!(
                                    next_point.stream.unwrap().logical,
                                    22 + 10 * (ordinal + 3)
                                        - if aligned { 8 * (ordinal + 3).min(3) } else { 0 }
                                );
                            }
                            checked += 1;
                        }
                        if ordinal < 5000 {
                            let row = decoder.next().unwrap().unwrap();
                            assert_eq!(row.key(0).unwrap().get() as u64, ordinal + 1);
                            assert_eq!(
                                row.values[0],
                                Value::NumberBits(bits(ordinal + 1, aligned))
                            );
                        }
                    }
                    // Confirm and retain EOF independently of the pending terminator.
                    assert!(decoder.next().unwrap().is_none());
                    let end = decoder.checkpoint().unwrap();
                    assert!(
                        Decoder::resume(&input.reader, &input.symbol, &end)
                            .unwrap()
                            .next()
                            .unwrap()
                            .is_none()
                    );
                    drop(decoder);
                    assert_eq!(budget.statistics().current_bytes, base);
                }
            }
        }
    }
    if cfg!(feature = "compression") {
        assert!(exact_end && crossing);
    }
    eprintln!("M5 primitive restarts: {checked}");
}
#[test]
fn malformed_private_positions_fail_and_release_all_restore_charges() {
    let input = input(0, Order::Little, 32768, false);
    let mut decoder = Decoder::new(&input.reader, &input.symbol).unwrap();
    decoder.next().unwrap();
    let point = decoder.checkpoint().unwrap();
    let mut variants = Vec::new();
    let mut p = point;
    p.emitted = 5001;
    variants.push(p);
    let mut p = point;
    p.done = true;
    variants.push(p);
    let mut p = point;
    p.started = false;
    variants.push(p);
    let mut p = point;
    p.previous[0] = i64::MAX;
    variants.push(p);
    let mut p = point;
    p.previous[1] = 1;
    variants.push(p);
    let mut p = point;
    p.minimum[0] = 0;
    variants.push(p);
    let mut p = point;
    p.widths[0] = 4;
    variants.push(p);
    let mut p = point;
    p.stream.as_mut().unwrap().decoded = 1;
    variants.push(p);
    let mut p = point;
    p.stream.as_mut().unwrap().physical = p.identity.end;
    variants.push(p);
    let mut p = point;
    p.stream = None;
    variants.push(p);
    let base = input.reader.budget_statistics().current_bytes;
    for point in variants {
        let error = Decoder::resume(&input.reader, &input.symbol, &point)
            .err()
            .unwrap();
        assert_eq!(error.kind, ErrorKind::InvalidOffset);
        assert_eq!(error.symbol_id, Some(1));
        assert_eq!(input.reader.budget_statistics().current_bytes, base);
    }
    let mut p = point;
    p.identity.requested = 2;
    assert_eq!(
        Decoder::resume(&input.reader, &input.symbol, &p)
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    input.changed.store(true, Ordering::Relaxed);
    assert_eq!(
        decoder.checkpoint().err().unwrap().kind,
        ErrorKind::SourceChanged
    );
    input.changed.store(false, Ordering::Relaxed);
    assert_eq!(decoder.next().err().unwrap().kind, ErrorKind::SourceChanged);
}
#[cfg(feature = "compression")]
#[test]
fn restart_rechecks_whole_zlib_block_and_propagates_short_io_failures() {
    for mode in [1, 2] {
        let input = input(mode, Order::Little, 32768, false);
        let mut decoder = Decoder::new(&input.reader, &input.symbol).unwrap();
        for _ in 0..4000 {
            decoder.next().unwrap();
        }
        let point = decoder.checkpoint().unwrap();
        let target = point.stream.unwrap().physical;
        assert!(target > 0);
        let base = input.reader.budget_statistics().current_bytes;
        input.fail.store(target, Ordering::Relaxed);
        let error = Decoder::resume(&input.reader, &input.symbol, &point)
            .err()
            .unwrap();
        assert_eq!(error.kind, ErrorKind::Io);
        assert_eq!(error.symbol_id, Some(1));
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(input.reader.budget_statistics().current_bytes, base);
        input.fail.store(u64::MAX, Ordering::Relaxed);
        if mode == 2 {
            // Checksum corruption after the point, simulating an undetected
            // physical fault, must be caught before returning the next row.
            let mut bytes = input.data.lock().unwrap();
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
            drop(bytes);
            assert_eq!(
                Decoder::resume(&input.reader, &input.symbol, &point)
                    .err()
                    .unwrap()
                    .kind,
                ErrorKind::InvalidCompression
            );
            assert_eq!(input.reader.budget_statistics().current_bytes, base);
        }
    }
}

#[test]
fn pending_terminator_is_validated_and_late_errors_remain_sticky() {
    let input = input(0, Order::Little, 32768, false);
    let mut decoder = Decoder::new(&input.reader, &input.symbol).unwrap();
    for _ in 0..5000 {
        decoder.next().unwrap();
    }
    let point = decoder.checkpoint().unwrap();
    assert!(!point.done);
    input
        .fail
        .store(point.stream.unwrap().physical, Ordering::Relaxed);
    let mut failed = Decoder::resume(&input.reader, &input.symbol, &point).unwrap();
    assert_eq!(failed.next().err().unwrap().kind, ErrorKind::Io);
    assert_eq!(failed.next().err().unwrap().kind, ErrorKind::Io);
    input.fail.store(u64::MAX, Ordering::Relaxed);

    let mut data = input.data.lock().unwrap();
    let last = data.len() - 1;
    data[last] = 1;
    drop(data);
    let mut suffix = Decoder::resume(&input.reader, &input.symbol, &point).unwrap();
    assert_eq!(suffix.next().err().unwrap().kind, ErrorKind::InvalidCount);
    assert_eq!(suffix.next().err().unwrap().kind, ErrorKind::InvalidCount);
    assert_eq!(
        suffix.checkpoint().err().unwrap().kind,
        ErrorKind::InvalidCount
    );
}
