//! Fixed task-028 recipe: three rows, three labels, two acronyms, five fields.
use gdxrs::*;
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("output")?;
    let scratch = args.next().ok_or("scratch")?;
    let (mut zlib, mut bits) = (false, false);
    for mode in args {
        match mode.to_str() {
            Some("zlib") => zlib = true,
            Some("bits") => bits = true,
            _ => return Err("invalid mode".into()),
        }
    }
    let mut w = Writer::create(
        path,
        WriteOptions {
            memory_budget_bytes: 1024 * 1024,
            compression: if zlib {
                WriteCompression::Zlib
            } else {
                WriteCompression::None
            },
            ..WriteOptions::default()
        },
    )?;
    let mut o = MetadataOptions::new(scratch);
    o.maximum_open_files = 6;
    o.merge_fan_in = 2;
    o.run_bytes = 512;
    w.begin_metadata(&o)?;
    w.add_acronym(AcronymId::new(7)?, b"Old", b"before")?;
    w.add_acronym(AcronymId::new(9)?, b"Beta", b"second")?;
    w.update_acronym(AcronymId::new(7)?, b"Alpha", b"renamed")?;
    let specials = if bits {
        [
            f64::from_bits(0x7ff8000000001234),
            -0.0,
            0.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ]
    } else {
        [-101.0, -102.0, -103.0, -104.0, -105.0]
    };
    w.set_write_value_policy(WriteValuePolicy::new(
        SpecialValueMap::new(specials.map(f64::to_bits))?,
        IeeeWritePolicy::NormalizeNative,
    ))?;
    let keys = [
        w.register_uel(b"a")?,
        w.register_uel(b"b")?,
        w.register_uel(b"c")?,
    ];
    let symbol = w.register_symbol(&WriteSymbol::new(b"V", SymbolType::Variable, 1))?;
    let mut rows = w.begin_symbol(symbol, None)?;
    rows.write_f64(&keys[..1], &specials)?;
    rows.write_f64(
        &keys[1..2],
        &if bits {
            [8.0, 9.0, 10.0, 11.0, 12.0]
        } else {
            [
                1.25,
                -0.0,
                f64::from_bits(0x7ff8000000001234),
                f64::INFINITY,
                f64::NEG_INFINITY,
            ]
        },
    )?;
    let a = match AcronymId::new(7)?.value()? {
        Value::Acronym { encoded_bits, .. } => f64::from_bits(encoded_bits),
        _ => return Err("acronym encoding".into()),
    };
    let b = match AcronymId::new(9)?.value()? {
        Value::Acronym { encoded_bits, .. } => f64::from_bits(encoded_bits),
        _ => return Err("acronym encoding".into()),
    };
    rows.write_f64(&keys[2..], &[a, b, 42.0, specials[1], specials[4]])?;
    rows.finish()?;
    w.finish()?;
    println!(
        "{{\"peak_root\":{},\"peak_handles\":{},\"peak_temporary\":{}}}",
        w.budget_statistics().peak_bytes,
        w.metadata_statistics().peak_open_files,
        w.metadata_statistics().peak_temporary_bytes
    );
    Ok(())
}
