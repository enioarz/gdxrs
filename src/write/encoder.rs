//! Reusable bounded section framing; never seeks or publishes its sink.
#[cfg(feature = "compression")]
use crate::budget::Reservation;
use crate::{
    Error, ErrorKind, Result,
    budget::{Budget, Buffer},
};
#[cfg(feature = "compression")]
use miniz_oxide::{
    DataFormat,
    deflate::{
        CompressionLevel,
        core::{CompressorOxide, TDEFLFlush, TDEFLStatus, compress},
    },
};
use std::sync::Arc;

const BLOCK: usize = 32768;
#[cfg(feature = "compression")]
const PRIVATE_STATE: usize = 320 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WriteCompression {
    #[default]
    None,
    /// Independent <=32 KiB zlib blocks, level 1, with stored fallback.
    Zlib,
}
impl WriteCompression {
    pub(super) fn validate(self) -> Result<()> {
        #[cfg(not(feature = "compression"))]
        if self == Self::Zlib {
            return Err(Error::new(
                ErrorKind::CompressionUnavailable,
                "compression feature is disabled",
            ));
        }
        Ok(())
    }
}

pub(super) struct Encoder {
    input: Option<Buffer<u8>>,
    output: Option<Buffer<u8>>,
    #[cfg(feature = "compression")]
    codec: Option<Buffer<CompressorOxide>>,
    #[cfg(feature = "compression")]
    _private: Option<Reservation>,
    failed: bool,
}
impl Encoder {
    pub(super) fn new(budget: &Arc<Budget>, mode: WriteCompression) -> Result<Self> {
        mode.validate()?;
        #[cfg(feature = "compression")]
        if mode == WriteCompression::Zlib {
            if std::mem::size_of::<CompressorOxide>() > 128 * 1024 {
                return Err(Error::new(
                    ErrorKind::BudgetTooSmall,
                    "encoder state exceeds audited ceiling",
                ));
            }
            let private = budget.reserve(PRIVATE_STATE)?;
            let mut codec = Buffer::new(budget, 1)?;
            let input = Buffer::new(budget, BLOCK)?;
            let output = Buffer::zeroed(budget, BLOCK + 3)?;
            codec.push(CompressorOxide::with_format_and_level(
                DataFormat::Zlib,
                CompressionLevel::BestSpeed,
            ))?;
            return Ok(Self {
                input: Some(input),
                output: Some(output),
                codec: Some(codec),
                _private: Some(private),
                failed: false,
            });
        }
        let _ = budget;
        Ok(Self {
            input: None,
            output: None,
            #[cfg(feature = "compression")]
            codec: None,
            #[cfg(feature = "compression")]
            _private: None,
            failed: false,
        })
    }
    fn ready(&self) -> Result<()> {
        if self.failed {
            Err(Error::new(
                ErrorKind::InvalidWriteState,
                "section encoder has failed",
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn bytes(
        &mut self,
        mut bytes: &[u8],
        mut emit: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        self.ready()?;
        let result = (|| {
            if self.input.is_none() {
                return emit(bytes);
            }
            while !bytes.is_empty() {
                let input = self.input.as_mut().ok_or_else(|| {
                    Error::new(ErrorKind::InvalidWriteState, "missing section input")
                })?;
                let count = (BLOCK - input.data.len()).min(bytes.len());
                input.data.extend_from_slice(&bytes[..count]);
                bytes = &bytes[count..];
                if input.data.len() == BLOCK {
                    self.block(&mut emit)?;
                }
            }
            Ok(())
        })();
        self.failed = result.is_err();
        result
    }
    pub(super) fn string(
        &mut self,
        bytes: &[u8],
        mut emit: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        self.ready()?;
        let length = u8::try_from(bytes.len()).map_err(|_| {
            self.failed = true;
            Error::new(ErrorKind::InvalidOptions, "writer string exceeds 255 bytes")
        })?;
        self.bytes(&[length], &mut emit)?;
        self.bytes(bytes, emit)
    }
    pub(super) fn i32(&mut self, value: i32, emit: impl FnMut(&[u8]) -> Result<()>) -> Result<()> {
        self.bytes(&value.to_le_bytes(), emit)
    }
    pub(super) fn end(&mut self, mut emit: impl FnMut(&[u8]) -> Result<()>) -> Result<()> {
        self.ready()?;
        let result = self.block(&mut emit);
        self.failed = result.is_err();
        result
    }
    fn block(&mut self, emit: &mut impl FnMut(&[u8]) -> Result<()>) -> Result<()> {
        let Some(input) = self.input.as_mut() else {
            return Ok(());
        };
        if input.data.is_empty() {
            return Ok(());
        }
        let output = self
            .output
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "missing section output"))?;
        let raw_length = input.data.len();
        #[cfg(feature = "compression")]
        let (length, tag) = {
            let state = self
                .codec
                .as_mut()
                .and_then(|b| b.data.first_mut())
                .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "missing encoder state"))?;
            super::event(super::Event::Codec).map_err(crate::Error::io)?;
            state.reset();
            // Bounded attempt: a useful zlib stream must fit below the raw size.
            // Output exhaustion is stored fallback, never Vec growth.
            let capacity = raw_length - 1;
            let (status, used, written) = compress(
                state,
                &input.data,
                &mut output.data[3..3 + capacity],
                TDEFLFlush::Finish,
            );
            match status {
                TDEFLStatus::Done if used == raw_length && written > 0 && written < raw_length => {
                    (written, 1)
                }
                TDEFLStatus::Okay if written == capacity => (raw_length, 0),
                _ => {
                    return Err(Error::new(
                        ErrorKind::InvalidCompression,
                        "bounded encoder failed or made incomplete progress",
                    ));
                }
            }
        };
        #[cfg(not(feature = "compression"))]
        let (length, tag) = (raw_length, 0);
        if tag == 0 {
            output.data[3..3 + length].copy_from_slice(&input.data);
        }
        let encoded = u16::try_from(length).map_err(|_| {
            Error::new(
                ErrorKind::InvalidCompression,
                "encoded block length overflow",
            )
        })?;
        output.data[0] = tag;
        output.data[1..3].copy_from_slice(&encoded.to_be_bytes());
        emit(&output.data[..3 + length])?;
        input.data.clear();
        Ok(())
    }
}
