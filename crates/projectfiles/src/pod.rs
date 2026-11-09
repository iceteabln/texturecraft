//! Checked integer reads. A short buffer is `None`, never a panic.

pub(crate) fn le_bytes<const N: usize>(bytes: &[u8], off: usize) -> Option<[u8; N]> {
    let end = off.checked_add(N)?;
    let slice = bytes.get(off..end)?;
    let mut out = [0u8; N];
    out.copy_from_slice(slice);
    Some(out)
}

pub(crate) fn le_u16(bytes: &[u8], off: usize) -> Option<u16> {
    le_bytes(bytes, off).map(u16::from_le_bytes)
}

pub(crate) fn le_u32(bytes: &[u8], off: usize) -> Option<u32> {
    le_bytes(bytes, off).map(u32::from_le_bytes)
}

pub(crate) fn le_u64(bytes: &[u8], off: usize) -> Option<u64> {
    le_bytes(bytes, off).map(u64::from_le_bytes)
}

pub(crate) fn le_i16(bytes: &[u8], off: usize) -> Option<i16> {
    le_bytes(bytes, off).map(i16::from_le_bytes)
}

pub(crate) fn le_i32(bytes: &[u8], off: usize) -> Option<i32> {
    le_bytes(bytes, off).map(i32::from_le_bytes)
}

pub(crate) fn le_i64(bytes: &[u8], off: usize) -> Option<i64> {
    le_bytes(bytes, off).map(i64::from_le_bytes)
}

pub(crate) fn le_f32(bytes: &[u8], off: usize) -> Option<f32> {
    le_bytes(bytes, off).map(f32::from_le_bytes)
}
