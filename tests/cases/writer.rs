use gdxrs::*;
#[test]
fn public_empty_writer_shared_budget_unicode_paths_and_no_clobber() {
    let marker = super::common::TempFile::new(b"caller");
    let parent = marker.path.with_extension("writer-é-日本語");
    std::fs::create_dir(&parent).unwrap();
    let output = parent.join("новый.gdx");
    let r = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let before = r.budget_statistics().current_bytes;
    let mut writer = r.create_writer(&output, r.write_options()).unwrap();
    assert!(!output.exists());
    let temp = writer.temporary_path().to_path_buf();
    let stats = writer.finish().unwrap();
    assert_eq!(stats.status, WriteStatus::Complete);
    assert!(stats.published);
    assert_eq!((stats.output_bytes, stats.bytes_written), (197, 249));
    assert!(!temp.exists());
    drop(writer);
    assert_eq!(r.budget_statistics().current_bytes, before);
    drop(r);
    let copy = Reader::open(&output, OpenOptions::default()).unwrap();
    let info = copy.information().unwrap();
    assert_eq!(
        (
            info.version,
            info.compression_level,
            info.uel_count,
            info.symbol_count
        ),
        (7, 0, 0, 0)
    );
    assert_eq!(info.producer, b"gdxrs");
    assert_eq!(info.audit, b"gdxrs");
    assert!(copy.symbols().unwrap().next().unwrap().is_none());
    let universe = copy.symbol(SymbolId::UNIVERSE).unwrap();
    assert!(
        copy.scan(&universe, copy.scan_options())
            .unwrap()
            .next_batch()
            .unwrap()
            .is_none()
    );
    drop(universe);
    drop(copy);
    assert!(Writer::create(&output, WriteOptions::default()).is_err());
    assert_eq!(std::fs::metadata(&output).unwrap().len(), 197);
    std::fs::remove_file(&output).unwrap();
    std::fs::remove_dir(parent).unwrap();
}

#[cfg(feature = "compression")]
#[test]
fn public_compressed_empty_writer_shares_root_and_validates_metadata() {
    let marker = super::common::TempFile::new(b"caller");
    let output = marker.path.with_extension("zlib.gdx");
    let reader = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let before = reader.budget_statistics().current_bytes;
    let mut writer = reader
        .create_writer(
            &output,
            WriteOptions {
                compression: WriteCompression::Zlib,
                ..reader.write_options()
            },
        )
        .unwrap();
    assert!(reader.budget_statistics().current_bytes > before + 320 * 1024);
    assert!(!output.exists());
    let stats = writer.finish().unwrap();
    assert_eq!(stats.status, WriteStatus::Complete);
    assert_eq!(stats.bytes_written, stats.output_bytes + 52);
    drop(writer);
    assert_eq!(reader.budget_statistics().current_bytes, before);
    let copy = Reader::open(&output, OpenOptions::default()).unwrap();
    let info = copy.information().unwrap();
    assert_eq!(
        (
            info.version,
            info.compression_level,
            info.symbol_count,
            info.uel_count
        ),
        (7, 1, 0, 0)
    );
    assert_eq!(
        (info.producer, info.audit),
        (b"gdxrs".as_slice(), b"gdxrs".as_slice())
    );
    assert!(copy.symbols().unwrap().next().unwrap().is_none());
    assert!(copy.uels().unwrap().next().unwrap().is_none());
    assert!(copy.acronyms().unwrap().next().unwrap().is_none());
    let mut texts = copy.set_texts().unwrap();
    assert_eq!(texts.next().unwrap().unwrap().text, b"");
    assert!(texts.next().unwrap().is_none());
    drop(texts);
    drop(copy);
    std::fs::remove_file(output).unwrap();
}

#[test]
fn public_metadata_registration_shared_root_and_cleanup() {
    let marker = super::common::TempFile::new(b"caller");
    let parent = marker.path.with_extension("metadata-é");
    std::fs::create_dir(&parent).unwrap();
    let reader = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let before = reader.budget_statistics().current_bytes;
    let mut modes = vec![WriteCompression::None];
    if cfg!(feature = "compression") {
        modes.push(WriteCompression::Zlib);
    }
    for (n, compression) in modes.into_iter().enumerate() {
        let output = parent.join(format!("output-{n}.gdx"));
        let mut writer = reader
            .create_writer(
                &output,
                WriteOptions {
                    compression,
                    ..reader.write_options()
                },
            )
            .unwrap();
        let mut limits = MetadataOptions::new(&parent);
        limits.maximum_open_files = 6;
        limits.merge_fan_in = 2;
        limits.run_bytes = 1024;
        writer.begin_metadata(&limits).unwrap();
        writer
            .register_uels_fresh([Ok(b"z".as_slice()), Ok(b"a"), Ok(b"z")], |ordinal, id| {
                assert_eq!(id.get(), if ordinal == 2 { 2 } else { 1 });
                Ok(())
            })
            .unwrap();
        let i = writer
            .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
            .unwrap();
        writer.add_comment(i, b"comment").unwrap();
        writer.add_set_text(b"text").unwrap();
        writer.finish().unwrap();
        assert_eq!(writer.metadata_statistics().temporary_bytes, 0);
        drop(writer);
        assert_eq!(reader.budget_statistics().current_bytes, before);
        let copy = Reader::open(&output, OpenOptions::default()).unwrap();
        assert_eq!(
            (
                copy.information().unwrap().symbol_count,
                copy.information().unwrap().uel_count
            ),
            (1, 2)
        );
        let i = copy.find_symbol(b"i").unwrap().unwrap();
        assert_eq!(
            copy.comments(&i).unwrap().next().unwrap().unwrap().text,
            b"comment"
        );
        drop(i);
        drop(copy);
        std::fs::remove_file(output).unwrap();
    }
    std::fs::remove_dir(parent).unwrap();
}

#[test]
fn public_sorted_symbol_writer_shared_root_and_alias_domain() {
    let marker = super::common::TempFile::new(b"caller");
    let output = marker.path.with_extension("records.gdx");
    let reader = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions::default(),
    )
    .unwrap();
    let before = reader.budget_statistics().current_bytes;
    let mut writer = reader
        .create_writer(&output, reader.write_options())
        .unwrap();
    writer
        .begin_metadata(&MetadataOptions::new(output.parent().unwrap()))
        .unwrap();
    let key = writer.register_uel(b"registered").unwrap();
    let set = writer
        .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
        .unwrap();
    let mut spec = WriteSymbol::new(b"J", SymbolType::Alias, 1);
    spec.alias_target = Some(SymbolReference::Registered(set));
    let alias = writer.register_symbol(&spec).unwrap();
    let param = writer
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))
        .unwrap();
    writer
        .set_domains(
            param,
            &[WriteDomain::Strict(SymbolReference::Registered(alias))],
        )
        .unwrap();
    let mut rows = writer.begin_symbol(set, None).unwrap();
    rows.write(&[key], &[Value::ZERO]).unwrap();
    assert_eq!(rows.finish().unwrap(), 1);
    let mut rows = writer
        .begin_symbol(
            param,
            Some(&[KeyBounds {
                minimum: 1,
                maximum: 255,
            }]),
        )
        .unwrap();
    rows.write(&[key], &[Value::Special(SpecialValue::Epsilon)])
        .unwrap();
    assert_eq!(rows.finish().unwrap(), 1);
    writer.finish().unwrap();
    drop(writer);
    assert_eq!(reader.budget_statistics().current_bytes, before);
    let copy = Reader::open(&output, OpenOptions::default()).unwrap();
    let param = copy.find_symbol(b"P").unwrap().unwrap();
    let mut domains = copy.domains(&param).unwrap();
    assert_eq!(
        domains
            .next()
            .unwrap()
            .unwrap()
            .strict_symbol
            .unwrap()
            .get(),
        alias.get()
    );
    drop(domains);
    let mut scan = copy.scan(&param, copy.scan_options()).unwrap();
    let batch = scan.next_batch().unwrap().unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch.key_column(0).unwrap()[0].get(), key.get());
    assert_eq!(
        batch.value_column(ValueField::Level).unwrap(),
        &[Value::Special(SpecialValue::Epsilon)]
    );
    assert!(scan.next_batch().unwrap().is_none());
    drop(scan);
    drop(param);
    drop(copy);
    std::fs::remove_file(output).unwrap();
}

#[test]
fn public_unsorted_raw_writer_preserves_registration_ids_and_shared_root() {
    let marker = super::common::TempFile::new(b"caller");
    let output = marker.path.with_extension("unsorted.gdx");
    let reader = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let before = reader.budget_statistics().current_bytes;
    let mut writer = reader
        .create_writer(&output, reader.write_options())
        .unwrap();
    let mut limits = MetadataOptions::new(output.parent().unwrap());
    limits.run_bytes = 512;
    limits.maximum_open_files = 6;
    limits.merge_fan_in = 2;
    writer.begin_metadata(&limits).unwrap();
    let z = writer.register_uel(b"z").unwrap();
    let a = writer.register_uel(b"a").unwrap();
    let id = writer
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))
        .unwrap();
    let mut rows = writer.begin_unsorted_symbol(id, None).unwrap();
    rows.write(&[a], &[Value::NumberBits((-2.5f64).to_bits())])
        .unwrap();
    rows.write(&[z], &[Value::NumberBits(1.25f64.to_bits())])
        .unwrap();
    let stats = rows.finish().unwrap();
    assert_eq!(stats.records, 2);
    assert!(stats.spill.complete);
    assert_eq!(stats.spill.merge_passes, 1);
    writer.finish().unwrap();
    drop(writer);
    assert_eq!(reader.budget_statistics().current_bytes, before);
    let copy = Reader::open(&output, OpenOptions::default()).unwrap();
    let h = copy.symbol(SymbolId::new(id.get()).unwrap()).unwrap();
    let mut c = copy.scan(&h, copy.scan_options()).unwrap();
    let b = c.next_batch().unwrap().unwrap();
    assert_eq!(b.len(), 2);
    assert_eq!(
        b.key_column(0)
            .unwrap()
            .iter()
            .map(|id| id.get())
            .collect::<Vec<_>>(),
        vec![z.get(), a.get()]
    );
    assert_eq!(
        b.value_column(ValueField::Level).unwrap(),
        &[
            Value::NumberBits(1.25f64.to_bits()),
            Value::NumberBits((-2.5f64).to_bits())
        ]
    );
    drop(c);
    drop(h);
    drop(copy);
    std::fs::remove_file(output).unwrap();
}

#[test]
fn public_string_writer_extends_registration_without_retained_tokens() {
    let marker = super::common::TempFile::new(b"caller");
    let output = marker.path.with_extension("strings.gdx");
    let reader = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let before = reader.budget_statistics().current_bytes;
    let mut writer = reader
        .create_writer(&output, reader.write_options())
        .unwrap();
    let mut limits = MetadataOptions::new(output.parent().unwrap());
    limits.run_bytes = 512;
    limits.maximum_open_files = 7;
    limits.merge_fan_in = 2;
    writer.begin_metadata(&limits).unwrap();
    writer.register_uel(b"z").unwrap();
    let id = writer
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))
        .unwrap();
    let mut strings: StringRecordWriter<'_> = writer.begin_string_records().unwrap();
    strings.begin_symbol(id).unwrap();
    strings
        .write(&[b"a"], &[Value::Special(SpecialValue::Epsilon)])
        .unwrap();
    strings
        .write(&[b"z"], &[Value::NumberBits(2.0f64.to_bits())])
        .unwrap();
    let stats: StringWriteStatistics = strings.finish().unwrap();
    assert_eq!((stats.records, stats.uels, stats.label_requests), (2, 2, 2));
    writer.finish().unwrap();
    drop(writer);
    assert_eq!(reader.budget_statistics().current_bytes, before);
    let copy = Reader::open(&output, OpenOptions::default()).unwrap();
    let h = copy.symbol(SymbolId::new(id.get()).unwrap()).unwrap();
    let mut scan = copy.scan(&h, copy.scan_options()).unwrap();
    let b = scan.next_batch().unwrap().unwrap();
    assert_eq!(b.len(), 2);
    assert_eq!(b.key_column(0).unwrap()[0].get(), 1);
    assert_eq!(
        b.value_column(ValueField::Level).unwrap(),
        &[
            Value::NumberBits(2.0f64.to_bits()),
            Value::Special(SpecialValue::Epsilon)
        ]
    );
    drop(scan);
    drop(h);
    drop(copy);
    std::fs::remove_file(output).unwrap();
}

#[test]
fn public_mapped_adapters_preserve_original_ids_under_one_root() {
    let marker = super::common::TempFile::new(b"caller");
    let output = marker.path.with_extension("mapped.gdx");
    let mut writer = Writer::create(
        &output,
        WriteOptions {
            memory_budget_bytes: 1024 * 1024,
            ..WriteOptions::default()
        },
    )
    .unwrap();
    let mut options = MetadataOptions::new(output.parent().unwrap());
    options.maximum_open_files = 7;
    options.merge_fan_in = 2;
    options.run_bytes = 512;
    writer.begin_metadata(&options).unwrap();
    let symbol = writer
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))
        .unwrap();
    writer
        .register_mapped_uels([
            Ok((MappedUelId::new(9).unwrap(), b"z")),
            Ok((MappedUelId::new(0).unwrap(), b"a")),
        ])
        .unwrap();
    let mut rows: MappedSymbolWriter<'_> = writer.begin_mapped_symbol(symbol).unwrap();
    rows.write(
        &[MappedUelId::new(9).unwrap()],
        &[Value::NumberBits(1.25f64.to_bits())],
    )
    .unwrap();
    rows.write(
        &[MappedUelId::new(0).unwrap()],
        &[Value::Special(SpecialValue::Epsilon)],
    )
    .unwrap();
    rows.finish().unwrap();
    writer.finish().unwrap();
    drop(writer);
    let reader = Reader::open(
        &output,
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let h = reader.symbol(SymbolId::new(symbol.get()).unwrap()).unwrap();
    let before = reader.budget_statistics().current_bytes;
    let mut map: MappedUels = reader.mapped_uels(&options).unwrap();
    map.register([
        Ok((UelId::new(1).unwrap(), MappedUelId::new(9).unwrap())),
        Ok((UelId::new(2).unwrap(), MappedUelId::new(0).unwrap())),
    ])
    .unwrap();
    let mut scan: MappedCursor<'_> = reader
        .scan_mapped(&h, &mut map, MappedReadOptions::default())
        .unwrap();
    let first = scan.next().unwrap().unwrap();
    assert_eq!(first.raw_keys, &[UelId::new(2).unwrap()]);
    assert_eq!(first.mapped_keys, &[MappedUelId::new(0).unwrap()]);
    assert_eq!(first.values, &[Value::Special(SpecialValue::Epsilon)]);
    assert_eq!(
        scan.next().unwrap().unwrap().raw_keys,
        &[UelId::new(1).unwrap()]
    );
    assert!(scan.next().unwrap().is_none());
    scan.close().unwrap();
    drop(scan);
    map.close().unwrap();
    drop(map);
    assert_eq!(reader.budget_statistics().current_bytes, before);
    drop(h);
    drop(reader);
    std::fs::remove_file(output).unwrap();
}
