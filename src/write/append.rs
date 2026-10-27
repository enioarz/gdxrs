//! Explicit O(file-size) checked rewrite transactions; never in-place append.
use super::*;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppendPolicy {
    /// Publish to a different, absent destination with an atomic no-clobber link.
    CreateNew,
    /// Replace exactly the opened source pathname by atomic rename. The caller
    /// must hold exclusive ownership of that pathname through finish/abort.
    ReplaceSource,
}
#[derive(Clone, Debug)]
pub struct AppendOptions {
    pub policy: AppendPolicy,
    /// Combined admitted temporary output and metadata/sort scratch ceilings.
    /// The existing source is excluded. No second output copy is made.
    pub maximum_transaction_bytes: u64,
    /// Aggregate library handles: source + output + configured metadata handles.
    pub maximum_open_files: usize,
}
impl AppendOptions {
    pub fn new(policy: AppendPolicy) -> Self {
        Self {
            policy,
            maximum_transaction_bytes: u64::MAX,
            maximum_open_files: 11,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AppendStatistics {
    pub source_bytes: u64,
    pub inherited_symbols: u32,
    pub inherited_uels: u32,
    pub rewritten_records: u64,
    pub initialized: bool,
    pub admitted_transaction_bytes: u64,
    pub admitted_open_files: usize,
}
pub(super) struct State {
    pub reader: Option<Reader>,
    path: PathBuf,
    modified: Option<SystemTime>,
    pub options: AppendOptions,
    pub ready: bool,
    pub preparing: bool,
    statistics: AppendStatistics,
    _control: Reservation,
    _paths: Reservation,
}
fn invalid(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidOptions, message)
}
fn path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().len() > 4096 {
        return Err(invalid("append path exceeds bounded length"));
    }
    let filename = path
        .file_name()
        .ok_or_else(|| invalid("append path must name a file"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(Error::io)?;
    let path = parent.join(filename);
    if path.as_os_str().len() > 4096 || path.capacity() > 16384 {
        return Err(invalid("canonical append path exceeds allowance"));
    }
    Ok(path)
}
impl Writer {
    /// Open a pending append transaction under one shared reader/writer root.
    /// Call `begin_append` explicitly to import; ownership is retained for abort
    /// retry if initialization, decoding or scratch cleanup fails.
    pub fn open_append(
        source: impl AsRef<Path>,
        destination: impl AsRef<Path>,
        read_options: OpenOptions,
        write_options: WriteOptions,
        options: AppendOptions,
    ) -> Result<Self> {
        write_options.validate()?;
        read_options.validate()?;
        if read_options.memory_budget_bytes != write_options.memory_budget_bytes
            || options.maximum_open_files < 2
        {
            return Err(invalid(
                "append needs matching root caps and two base handles",
            ));
        }
        // Bound and charge both path conversions before opening any transaction output.
        if source.as_ref().as_os_str().len() > 4096 || destination.as_ref().as_os_str().len() > 4096
        {
            return Err(invalid("append path exceeds bounded length"));
        }
        let reader = Reader::open(source.as_ref(), read_options)?;
        let control = reader.source.budget.reserve(CONTROL)?;
        if size_of::<State>() > CONTROL {
            return Err(invalid("append controls exceed allowance"));
        }
        let paths = reader.source.budget.reserve(128 * 1024)?;
        let source = path(source.as_ref())?;
        let destination = path(destination.as_ref())?;
        let original = fs::symlink_metadata(&source).map_err(Error::io)?;
        if !original.file_type().is_file() {
            return Err(invalid("append source must be a regular non-symlink file"));
        }
        if (source == destination) != (options.policy == AppendPolicy::ReplaceSource) {
            return Err(invalid(
                "ReplaceSource requires the source pathname; CreateNew requires a different pathname",
            ));
        }
        let header = reader.source.header()?;
        let minimum =
            116 + header.audit.as_slice().len() as u64 + header.producer.as_slice().len() as u64;
        if write_options.maximum_output_bytes < minimum
            || write_options.maximum_output_bytes > options.maximum_transaction_bytes
        {
            return Err(Error::new(
                ErrorKind::OutputLimitExceeded,
                "append output/header exceeds transaction admission",
            ));
        }
        let counts = reader.source.counts()?;
        let state = State {
            statistics: AppendStatistics {
                source_bytes: reader.source.length,
                inherited_symbols: counts.symbols,
                inherited_uels: counts.uels,
                ..AppendStatistics::default()
            },
            path: source,
            modified: original.modified().ok(),
            options,
            ready: false,
            preparing: false,
            reader: Some(reader.clone()),
            _control: control,
            _paths: paths,
        };
        let mut writer = Self::with_budget_mode(
            &destination,
            &write_options,
            Arc::clone(&reader.source.budget),
            Some(header),
            state.options.policy == AppendPolicy::ReplaceSource,
        )?;
        writer.append = Some(state);
        Ok(writer)
    }
    pub fn append_statistics(&self) -> Option<AppendStatistics> {
        self.append.as_ref().map(|a| a.statistics)
    }
    pub(super) fn check_append_source(&self) -> Result<()> {
        if let Some(a) = &self.append {
            let reader = a
                .reader
                .as_ref()
                .ok_or_else(|| invalid("append source is closed"))?;
            reader.source.check_unchanged()?;
            let current = fs::symlink_metadata(&a.path).map_err(Error::io)?;
            if !current.file_type().is_file()
                || current.len() != a.statistics.source_bytes
                || current.modified().ok() != a.modified
            {
                return Err(Error::new(
                    ErrorKind::SourceChanged,
                    "append source pathname changed",
                ));
            }
        }
        Ok(())
    }
    /// Import original IDs, metadata and checked physical records in bounded
    /// batches. No new symbols can be written until this completes successfully.
    pub fn begin_append(&mut self, metadata: &MetadataOptions) -> Result<()> {
        let result = (|| {
            if self.statistics.status != WriteStatus::Open {
                return self.open();
            }
            let a = self
                .append
                .as_mut()
                .ok_or_else(|| invalid("not an append transaction"))?;
            if a.ready || a.preparing {
                return Err(invalid("append already initialized"));
            }
            metadata.validate()?;
            let disk = self
                .limit
                .checked_add(metadata.maximum_temporary_bytes)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::TemporarySpaceExceeded,
                        "append aggregate disk admission overflow",
                    )
                })?;
            let handles = metadata
                .maximum_open_files
                .checked_add(2)
                .ok_or_else(|| invalid("append handle admission overflow"))?;
            if disk > a.options.maximum_transaction_bytes {
                return Err(Error::new(
                    ErrorKind::TemporarySpaceExceeded,
                    "append output and scratch exceed aggregate quota",
                ));
            }
            if handles > a.options.maximum_open_files {
                return Err(invalid(
                    "append source/output/scratch exceed aggregate handle cap",
                ));
            }
            a.statistics.admitted_transaction_bytes = disk;
            a.statistics.admitted_open_files = handles;
            a.preparing = true;
            let reader = a
                .reader
                .as_ref()
                .ok_or_else(|| invalid("append source is closed"))?
                .clone();
            self.check_append_source()?;
            self.begin_metadata(metadata)?;
            self.import_append(&reader)?;
            self.check_append_source()?;
            let a = self
                .append
                .as_mut()
                .ok_or_else(|| invalid("append state missing"))?;
            a.preparing = false;
            a.ready = true;
            a.statistics.initialized = true;
            Ok(())
        })();
        result.map_err(|e| {
            if self.error.is_some() {
                e
            } else {
                self.fail(e)
            }
        })
    }
    /// Bind a prior symbol ID for new strict domains or aliases. Prior symbols
    /// cannot be overwritten or have their metadata changed in append mode.
    pub fn append_symbol_id(&mut self, id: SymbolId) -> Result<WriterSymbolId> {
        self.open()?;
        let maximum = self
            .append
            .as_ref()
            .ok_or_else(|| invalid("not an append transaction"))?
            .statistics
            .inherited_symbols;
        if id == SymbolId::UNIVERSE || id.get() > maximum {
            return Err(self.fail(invalid("ID outside inherited symbol range")));
        }
        self.metadata_call(|m| m.string_symbol(id.get()))
    }
    /// Bind a prior raw UEL ID without a label lookup (including duplicate labels).
    pub fn append_uel_id(&mut self, id: UelId) -> Result<WriterUelId> {
        self.open()?;
        let maximum = self
            .append
            .as_ref()
            .ok_or_else(|| invalid("not an append transaction"))?
            .statistics
            .inherited_uels;
        if id.get() > maximum {
            return Err(self.fail(invalid("ID outside inherited UEL range")));
        }
        self.metadata_call(|m| m.bound_uel(id.get()))
    }
    fn import_append(&mut self, reader: &Reader) -> Result<()> {
        event(Event::AppendRead).map_err(Error::io)?;
        self.import_append_metadata(reader)?;
        // Import with implicit domains; the complete inherited domain catalog is
        // installed and validated after all set memberships are available.
        for id in 1..=reader.source.counts()?.symbols {
            let id = SymbolId::new(id)?;
            let record = reader.lookup_record(id)?;
            if record.kind == SymbolType::Alias {
                reader.resolve(&record)?;
                continue;
            }
            let handle = reader.symbol(id)?;
            let mut options = reader.scan_options();
            options.batch_bytes = options.batch_bytes.min(4096);
            let mut scan = reader.scan(&handle, options)?;
            let token = self
                .metadata
                .as_mut()
                .ok_or_else(|| invalid("metadata missing"))?
                .string_symbol(id.get())?;
            let mut rows = self.begin_symbol(token, None)?;
            let mut count = 0u64;
            loop {
                event(Event::AppendRead).map_err(Error::io)?;
                let Some(batch) = scan.next_batch()? else {
                    break;
                };
                for index in 0..batch.len() {
                    // Reader synthesizes scalar defaults; the physical empty
                    // representation and count must remain empty in the output.
                    if record.dimension == 0 && record.count == 0 {
                        continue;
                    }
                    let row = batch
                        .row(index)
                        .ok_or_else(|| invalid("missing inherited row"))?;
                    let mut keys = [WriterUelId::for_append(token, UelId::new(1)?); 20];
                    for (d, key) in keys
                        .iter_mut()
                        .enumerate()
                        .take(usize::from(record.dimension))
                    {
                        let raw = row
                            .key(d as u8)
                            .ok_or_else(|| invalid("missing inherited key"))?;
                        *key = WriterUelId::for_append(token, raw);
                    }
                    let mut values = [Value::ZERO; 5];
                    for (f, value) in values.iter_mut().enumerate().take(record.kind.fields()) {
                        *value = row
                            .value(ValueField::ALL[f])
                            .ok_or_else(|| invalid("missing inherited value"))?;
                    }
                    rows.write(
                        &keys[..usize::from(record.dimension)],
                        &values[..record.kind.fields()],
                    )?;
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| invalid("append record counter overflow"))?;
                }
            }
            if count != u64::from(record.count) {
                return Err(invalid("inherited physical record count changed"));
            }
            rows.finish()?;
            let a = self
                .append
                .as_mut()
                .ok_or_else(|| invalid("append state missing"))?;
            a.statistics.rewritten_records = a
                .statistics
                .rewritten_records
                .checked_add(count)
                .ok_or_else(|| invalid("append counter overflow"))?;
        }
        self.import_append_domains(reader)?;
        Ok(())
    }
}
