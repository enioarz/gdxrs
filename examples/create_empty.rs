//! Create an empty v7 transaction. Registration/symbol writing follow in M3.
use gdxrs::{WriteCompression, WriteOptions, Writer};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let destination = args.next().ok_or("usage: create_empty NEW_FILE.gdx")?;
    let compression = match args.next().as_deref() {
        None => WriteCompression::None,
        Some(arg) if arg == "zlib" => WriteCompression::Zlib,
        _ => return Err("usage: create_empty NEW_FILE.gdx [zlib]".into()),
    };
    if args.next().is_some() {
        return Err("usage: create_empty NEW_FILE.gdx [zlib]".into());
    }
    let mut writer = Writer::create(
        destination,
        WriteOptions {
            compression,
            ..WriteOptions::default()
        },
    )?;
    let stats = writer.finish()?;
    println!(
        "{{\"output_bytes\":{},\"bytes_written\":{},\"published\":{}}}",
        stats.output_bytes, stats.bytes_written, stats.published
    );
    Ok(())
}
