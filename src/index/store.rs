use super::*;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Event {
    CreateDirectory,
    Create,
    Write,
    Read,
    Seek,
    Sync,
    Publish,
    Remove,
}
#[cfg(not(test))]
pub(super) fn event(_: Event) -> io::Result<()> {
    Ok(())
}
#[cfg(test)]
pub(super) fn event(e: Event) -> io::Result<()> {
    tests::event(e)
}
pub(super) fn seek(file: &mut File, at: u64) -> Result<()> {
    event(Event::Seek).map_err(Error::io)?;
    file.seek(SeekFrom::Start(at)).map_err(Error::io)?;
    Ok(())
}
pub(super) fn write(file: &mut File, mut bytes: &[u8]) -> Result<()> {
    while !bytes.is_empty() {
        let n = chunk(bytes.len());
        match event(Event::Write).and_then(|_| file.write(&bytes[..n])) {
            Ok(0) => return Err(Error::io(io::ErrorKind::WriteZero.into())),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(Error::io(e)),
        }
    }
    Ok(())
}
pub(super) fn read(file: &mut File, mut bytes: &mut [u8]) -> Result<()> {
    while !bytes.is_empty() {
        let n = chunk(bytes.len());
        match event(Event::Read).and_then(|_| file.read(&mut bytes[..n])) {
            Ok(0) => return Err(invalid()),
            Ok(n) => bytes = &mut bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(Error::io(e)),
        }
    }
    Ok(())
}
#[cfg(not(test))]
fn chunk(n: usize) -> usize {
    n
}
#[cfg(test)]
fn chunk(n: usize) -> usize {
    tests::chunk(n)
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Hook {
        fault: Option<(Event, usize, bool)>,
        short: usize,
        interrupt: bool,
        deny_remove: bool,
    }
    thread_local! {static HOOK:RefCell<Hook>=RefCell::new(Hook::default());}
    pub fn event(e: Event) -> io::Result<()> {
        HOOK.with(|h| {
            let mut h = h.borrow_mut();
            if e == Event::Remove && h.deny_remove {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            if h.interrupt && e == Event::Write {
                h.interrupt = false;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if let Some((kind, left, sticky)) = h.fault
                && e == kind
            {
                if left == 0 {
                    if !sticky {
                        h.fault = None;
                    }
                    return Err(if e == Event::Write {
                        io::ErrorKind::StorageFull
                    } else {
                        io::ErrorKind::PermissionDenied
                    }
                    .into());
                }
                h.fault = Some((kind, left - 1, sticky));
            }
            Ok(())
        })
    }
    pub fn chunk(n: usize) -> usize {
        HOOK.with(|h| {
            let nmax = h.borrow().short;
            if nmax == 0 { n } else { n.min(nmax) }
        })
    }
    pub fn fault(kind: Event, left: usize, sticky: bool) {
        HOOK.with(|h| h.borrow_mut().fault = Some((kind, left, sticky)));
    }
    pub fn deny(value: bool) {
        HOOK.with(|h| h.borrow_mut().deny_remove = value);
    }
    pub fn short() {
        HOOK.with(|h| {
            h.borrow_mut().short = 7;
            h.borrow_mut().interrupt = true;
        });
    }
    pub struct Reset;
    impl Reset {
        pub fn new() -> Self {
            HOOK.with(|h| *h.borrow_mut() = Hook::default());
            Self
        }
    }
    impl Drop for Reset {
        fn drop(&mut self) {
            HOOK.with(|h| *h.borrow_mut() = Hook::default());
        }
    }
}
