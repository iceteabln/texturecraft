use super::ImportError;

pub const HEADER_LEN: usize = 512;
pub const PAYLOAD_PRELUDE_LEN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssbinHeader {
    pub format_major: u32,
    pub format_minor: u32,
    pub revision: u32,
    pub compile_flags: u32,
    pub shortened: bool,
    pub compressed: bool,
}

impl AssbinHeader {
    pub fn is_format_v17(self) -> bool {
        self.format_major == 1 && self.format_minor >= 7 && !self.shortened && !self.compressed
    }
}

pub fn assbin_header(bytes: &[u8]) -> Result<AssbinHeader, ImportError> {
    if bytes.len() < HEADER_LEN {
        return Err(ImportError::Assbin(format!("assbin too small ({} bytes)", bytes.len())));
    }
    if !bytes.starts_with(b"ASSIMP.binary-dump.") {
        return Err(ImportError::Assbin("missing ASSIMP.binary-dump magic".into()));
    }
    let format_major = read_u32(bytes, 44)?;
    let format_minor = read_u32(bytes, 48)?;
    let revision = read_u32(bytes, 52)?;
    let compile_flags = read_u32(bytes, 56)?;
    let shortened = read_u16(bytes, 60)? != 0;
    let compressed = read_u16(bytes, 62)? != 0;
    Ok(AssbinHeader { format_major, format_minor, revision, compile_flags, shortened, compressed })
}

pub fn payload_offset(header: &AssbinHeader) -> usize {
    if header.is_format_v17() { HEADER_LEN + PAYLOAD_PRELUDE_LEN } else { HEADER_LEN }
}

fn read_u16(bytes: &[u8], off: usize) -> Result<u16, ImportError> {
    let b = bytes.get(off..off + 2).ok_or_else(|| ImportError::Assbin("unexpected EOF".into()))?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(bytes: &[u8], off: usize) -> Result<u32, ImportError> {
    let b = bytes.get(off..off + 4).ok_or_else(|| ImportError::Assbin("unexpected EOF".into()))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
