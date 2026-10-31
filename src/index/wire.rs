use super::*;
pub(super) const PAGE: usize = 1024;
pub(super) const FAN: usize = 16;
pub(super) const LEVELS: usize = 16;
pub(super) type Page = [u8; PAGE];
pub(super) fn invalid() -> Error {
    Error::new(
        ErrorKind::InvalidIndex,
        "invalid or incompatible checkpoint sidecar",
    )
}
pub(super) fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
pub(super) fn seal(page: &mut Page) -> [u8; 32] {
    let h = hash(&page[..PAGE - 32]);
    page[PAGE - 32..].copy_from_slice(&h);
    h
}
pub(super) fn verify(page: &Page, expected: &[u8; 32]) -> Result<()> {
    if &hash(&page[..PAGE - 32]) != expected || &page[PAGE - 32..] != expected {
        return Err(invalid());
    }
    Ok(())
}
pub(super) fn put(page: &mut Page, at: usize, bytes: &[u8]) {
    page[at..at + bytes.len()].copy_from_slice(bytes);
}
pub(super) fn num(page: &Page, at: usize) -> u64 {
    let mut b = [0; 8];
    b.copy_from_slice(&page[at..at + 8]);
    u64::from_le_bytes(b)
}
#[derive(Clone, Copy, Default)]
pub(super) struct Entry {
    pub first: u64,
    pub end: u64,
    pub offset: u64,
    pub digest: [u8; 32],
}
impl Entry {
    pub fn write(&self, page: &mut Page, at: usize) {
        put(page, at, &self.first.to_le_bytes());
        put(page, at + 8, &self.end.to_le_bytes());
        put(page, at + 16, &self.offset.to_le_bytes());
        put(page, at + 24, &self.digest);
    }
    pub fn read(page: &Page, at: usize) -> Self {
        let mut digest = [0; 32];
        digest.copy_from_slice(&page[at + 24..at + 56]);
        Self {
            first: num(page, at),
            end: num(page, at + 8),
            offset: num(page, at + 16),
            digest,
        }
    }
}
pub(super) fn prefix(kind: u8, depth: u8, count: u8, first: u64, end: u64) -> Page {
    let mut p = [0; PAGE];
    put(&mut p, 0, b"GDXPAGE1");
    p[8] = kind;
    p[9] = depth;
    p[10] = count;
    put(&mut p, 16, &first.to_le_bytes());
    put(&mut p, 24, &end.to_le_bytes());
    p
}
#[derive(Clone, Copy)]
pub(super) struct Root {
    pub entry: Entry,
    pub depth: u8,
    pub intervals: u64,
    pub directory_pages: u64,
    pub bytes: u64,
}
pub(super) fn header(
    reader: &Reader,
    point: &DecoderPosition,
    source_hash: [u8; 32],
    options: &IndexOptions,
    root: Root,
) -> Result<Page> {
    let mut p = [0; PAGE];
    put(&mut p, 0, b"GDXSIDX1");
    put(&mut p, 8, &1u32.to_le_bytes());
    put(&mut p, 12, &(PAGE as u32).to_le_bytes());
    put(&mut p, 16, &reader.source.length.to_le_bytes());
    put(&mut p, 24, &source_hash);
    put(&mut p, 56, &root.intervals.to_le_bytes());
    put(&mut p, 64, &root.entry.offset.to_le_bytes());
    put(&mut p, 72, &root.entry.digest);
    p[104] = root.depth;
    p[105] = point.dimension();
    put(&mut p, 112, &options.record_stride.to_le_bytes());
    put(&mut p, 120, &options.decoded_byte_stride.to_le_bytes());
    put(&mut p, 128, &point.index_bytes()?);
    put(&mut p, 640, &root.bytes.to_le_bytes());
    put(&mut p, 648, &root.directory_pages.to_le_bytes());
    put(&mut p, 656, &7u32.to_le_bytes());
    seal(&mut p);
    Ok(p)
}
pub(super) fn source_digest(reader: &Reader) -> Result<[u8; 32]> {
    let mut bytes = crate::budget::Buffer::zeroed(&reader.source.budget, 32768)?;
    let mut h = Sha256::new();
    let mut at = 0;
    reader.source.check_unchanged()?;
    while at < reader.source.length {
        let n = (reader.source.length - at).min(32768) as usize;
        reader.source.read_exact(at, &mut bytes.data[..n])?;
        h.update(&bytes.data[..n]);
        at += n as u64;
    }
    reader.source.check_unchanged()?;
    Ok(h.finalize().into())
}
