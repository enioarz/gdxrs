use gdxrs::*;
use std::{
    fmt::Write as _,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::new();
    for b in bytes {
        write!(&mut s, "{b:02x}").unwrap();
    }
    s
}
pub fn metadata(reader: &Reader) -> Result<String> {
    // Test-only bounded corpus output. Production views never collect catalogs.
    let mut s = String::new();
    let info = reader.information()?;
    assert!(info.audit.starts_with(b"GDX Library"));
    writeln!(
        &mut s,
        "GDXRS-ORACLE\t1\nF\t7\t{}\t{}\t{}\t{}\t<creation-system>",
        info.compression_level,
        info.symbol_count,
        info.uel_count,
        hex(info.producer)
    )
    .unwrap();
    let mut uels = reader.uels()?;
    while let Some(v) = uels.next()? {
        writeln!(&mut s, "U\t{}\t{}", v.id.get(), hex(v.label)).unwrap();
    }
    drop(uels);
    let mut texts = reader.set_texts()?;
    while let Some(v) = texts.next()? {
        writeln!(&mut s, "T\t{}\t0\t{}", v.id, hex(v.text)).unwrap();
    }
    drop(texts);
    let mut acronyms = reader.acronyms()?;
    while let Some(v) = acronyms.next()? {
        writeln!(
            &mut s,
            "A\t{}\t{}\t{}\t{}",
            v.ordinal,
            v.id.get(),
            hex(v.name),
            hex(v.text)
        )
        .unwrap();
    }
    drop(acronyms);
    for n in 0..=info.symbol_count {
        let handle = reader.symbol(SymbolId::new(n)?)?;
        let v = handle.metadata()?;
        let count = if v.kind == SymbolType::Alias {
            v.physical_record_count
        } else {
            v.raw_record_count
        };
        writeln!(
            &mut s,
            "S\t{n}\t{}\t{}\t{}\t{count}\t{}\t{}",
            hex(v.name),
            v.dimension,
            v.kind.code(),
            v.user_information,
            hex(v.description)
        )
        .unwrap();
        if n != 0 {
            let mut domains = reader.domains(&handle)?;
            write!(&mut s, "D\t{n}\t{}", domains.mode().code()).unwrap();
            while let Some(d) = domains.next()? {
                write!(
                    &mut s,
                    "\t{}:{}",
                    d.strict_symbol.map(|id| id.get()).unwrap_or(0),
                    hex(d.name)
                )
                .unwrap();
            }
            s.push('\n');
            drop(domains);
            let mut comments = reader.comments(&handle)?;
            while let Some(c) = comments.next()? {
                writeln!(&mut s, "C\t{n}\t{}\t{}", c.ordinal, hex(c.text)).unwrap();
            }
        }
    }
    Ok(s)
}
pub struct TempFile {
    pub path: PathBuf,
}
impl TempFile {
    pub fn new(data: &[u8]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = Path::new(".work").join(format!(
            "reader-test-{}-{}.gdx",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(data).unwrap();
        Self { path }
    }
}
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
