//! Reader for the layer document stored in a project, smart material or smart mask.
//!
//! The document is one binary tree of named records. A 12-byte header selects the layout:
//! registry records share a type table, inline records carry their type name each time.
//! This module only builds that tree. Mapping a record onto a TextureCraft layer, generator
//! or filter happens in `stack`.

use std::io::{Cursor, Read};

pub const MAGIC: u32 = 0x1B7C2FDD;

const MAX_RECURSION: u32 = 128;
const MAX_ARRAY_ITEMS: u32 = 1_000_000;
const MAX_STRING: u32 = 16 * 1024 * 1024;
const NEW_TYPE: u32 = 0xFFFF_FFFF;

#[derive(Debug, Clone)]
pub struct UNode {
    pub type_name: String,
    pub fields: Vec<(String, Value)>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Object(Option<UNode>),
    Array(Vec<Value>),
    String(Vec<u8>),
    /// Numeric field. The code is the document's type tag; the bytes are little-endian.
    Primitive(#[allow(dead_code)] u32, Vec<u8>),
    Null,
}

#[derive(Debug, Clone)]
struct TypeDef {
    name: String,
    members: Vec<(String, u32)>,
}

struct Reader<'a> {
    src: Cursor<&'a [u8]>,
    types: Vec<TypeDef>,
}

pub fn read_header(raw: &[u8]) -> Option<(u32, u32, u32)> {
    if raw.len() < 12 {
        return None;
    }
    let magic = crate::pod::le_u32(raw, 0)?;
    let layout = crate::pod::le_u32(raw, 4)?;
    let version = crate::pod::le_u32(raw, 8)?;
    (magic == MAGIC).then_some((magic, layout, version))
}

pub fn decode(raw: &[u8]) -> Result<(UNode, &'static str, u32), String> {
    let (_, layout, version) = read_header(raw).ok_or("not an HBO stream (bad magic or short)")?;
    match layout {
        0 => read_inline(raw).map(|node| (node, "inline", version)),
        1 => read_registry(raw).map(|node| (node, "registry", version)),
        other => Err(format!("unknown HBO version_check {other}")),
    }
}

fn read_registry(raw: &[u8]) -> Result<UNode, String> {
    if raw.len() < 12 {
        return Err("HBO too short".into());
    }
    let mut reader = Reader { src: Cursor::new(&raw[12..]), types: Vec::new() };
    let tag = reader.u32()?;
    if tag != 0x12 && tag != 0x13 {
        return Err("registry stream does not start with an object/root tag".into());
    }
    let type_count = reader.u32()?;
    if let Some(table) = reader.type_table(type_count)? {
        reader.types = table;
    }
    let (name, fields) = reader.record(0)?;
    Ok(UNode { type_name: name, fields })
}

impl Reader<'_> {
    fn u32(&mut self) -> Result<u32, String> {
        let mut buf = [0u8; 4];
        self.src.read_exact(&mut buf).map_err(|_| "unexpected EOF reading u32".to_string())?;
        Ok(u32::from_le_bytes(buf))
    }

    fn text(&mut self) -> Result<String, String> {
        let length = self.u32()?;
        if length == 0 {
            return Ok(String::new());
        }
        // A hostile length must fail before the allocation. A failed allocation aborts the process.
        if length > MAX_STRING || length as usize > self.remaining() {
            return Err(format!("string length {length} exceeds the remaining bytes"));
        }
        let mut buf = vec![0u8; length as usize];
        self.src.read_exact(&mut buf).map_err(|_| "unexpected EOF reading string".to_string())?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn remaining(&self) -> usize {
        self.src.get_ref().len().saturating_sub(self.src.position() as usize)
    }

    fn count(&mut self) -> Result<u32, String> {
        let count = self.u32()?;
        if count > MAX_ARRAY_ITEMS {
            return Err(format!("array item count {count} exceeds limit {MAX_ARRAY_ITEMS}"));
        }
        if count as usize > self.remaining() {
            return Err(format!("array item count {count} exceeds remaining bytes {}", self.remaining()));
        }
        Ok(count)
    }

    fn bytes(&mut self, size: usize, what: &str) -> Result<Vec<u8>, String> {
        let mut raw = vec![0u8; size];
        self.src.read_exact(&mut raw).map_err(|_| format!("EOF reading {what}"))?;
        Ok(raw)
    }

    fn type_table(&mut self, type_count: u32) -> Result<Option<Vec<TypeDef>>, String> {
        if type_count == 0 {
            return Ok(None);
        }
        let start = self.src.position();
        let mut table = Vec::new();
        for _ in 0..type_count {
            if self.u32()? != NEW_TYPE {
                self.src.set_position(start);
                return Ok(None);
            }
            let name = self.text()?;
            let member_count = self.count()?;
            let mut members = Vec::new();
            for _ in 0..member_count {
                members.push((self.text()?, self.u32()?));
            }
            table.push(TypeDef { name, members });
        }
        Ok(Some(table))
    }

    fn record(&mut self, depth: u32) -> Result<(String, Vec<(String, Value)>), String> {
        if depth > MAX_RECURSION {
            return Err("max recursion depth exceeded".into());
        }
        if self.u32()? == 0 {
            return Ok((String::new(), Vec::new()));
        }
        let def = self.type_def()?;
        self.fields(&def, depth)
    }

    fn type_def(&mut self) -> Result<TypeDef, String> {
        let index = self.u32()?;
        if index != NEW_TYPE {
            return self.types.get(index as usize).cloned().ok_or_else(|| format!("type index out of range: {index}"));
        }
        let name = self.text()?;
        let member_count = self.count()?;
        let mut members = Vec::new();
        for _ in 0..member_count {
            members.push((self.text()?, self.u32()?));
        }
        let def = TypeDef { name, members };
        self.types.push(def.clone());
        Ok(def)
    }

    fn fields(&mut self, def: &TypeDef, depth: u32) -> Result<(String, Vec<(String, Value)>), String> {
        let mut fields = Vec::new();
        for (name, code) in &def.members {
            fields.push((name.clone(), self.value(*code, depth + 1)?));
        }
        Ok((def.name.clone(), fields))
    }

    fn value(&mut self, code: u32, depth: u32) -> Result<Value, String> {
        if depth > MAX_RECURSION {
            return Err("max recursion depth exceeded".into());
        }
        match code {
            0x12 | 0x14 => {
                let (name, fields) = self.record(depth)?;
                if name.is_empty() && fields.is_empty() { Ok(Value::Object(None)) } else { Ok(Value::Object(Some(UNode { type_name: name, fields }))) }
            }
            0x13 => self.record_list(depth),
            0x11 => self.typed_list(depth),
            0x10 => Ok(Value::String(self.text()?.into_bytes())),
            other => {
                let Some(size) = registry_width(other) else {
                    return Err(format!("unhandled v11 type code: {other}"));
                };
                Ok(Value::Primitive(other, self.bytes(size, "primitive")?))
            }
        }
    }

    fn record_list(&mut self, depth: u32) -> Result<Value, String> {
        let count = self.count()?;
        let mut values = Vec::new();
        for _ in 0..count {
            let start = self.src.position();
            let (name, fields) = self.record(depth + 1)?;
            if self.src.position() == start {
                return Err("no progress while parsing v11 array (0x13)".into());
            }
            if name.is_empty() && fields.is_empty() {
                values.push(Value::Object(None));
            } else {
                values.push(Value::Object(Some(UNode { type_name: name, fields })));
            }
        }
        Ok(Value::Array(values))
    }

    fn typed_list(&mut self, depth: u32) -> Result<Value, String> {
        let count = self.count()?;
        let mut values = Vec::new();
        if count > 0 {
            let def = self.type_def()?;
            for _ in 0..count {
                let start = self.src.position();
                let (name, fields) = self.fields(&def, depth + 1)?;
                if self.src.position() == start {
                    return Err("no progress while parsing v11 array (0x11)".into());
                }
                values.push(Value::Object(Some(UNode { type_name: name, fields })));
            }
        }
        Ok(Value::Array(values))
    }
}

/// Byte width of a numeric registry field. Record, list and text tags are not numeric.
fn registry_width(code: u32) -> Option<usize> {
    Some(match code {
        1 | 5 | 9 | 11 => 4,
        2 | 6 | 12 | 15 | 21 => 8,
        3 | 7 => 12,
        4 | 8 => 16,
        10 => 1,
        13 => 36,
        14 => 64,
        16 => 32,
        22 => 16,
        _ => return None,
    })
}

fn read_inline(raw: &[u8]) -> Result<UNode, String> {
    let mut inline = Inline { data: raw, pos: 12 };
    if inline.u8()? != 0x12 {
        return Err("inline stream does not start with root object".into());
    }
    inline.record(0)
}

struct Inline<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Inline<'a> {
    fn u8(&mut self) -> Result<u8, String> {
        let value = *self.data.get(self.pos).ok_or("inline EOF")?;
        self.pos += 1;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, String> {
        let value = crate::pod::le_u16(self.data, self.pos).ok_or("inline EOF")?;
        self.pos += 2;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let value = crate::pod::le_u32(self.data, self.pos).ok_or("inline EOF")?;
        self.pos += 4;
        Ok(value)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(len).filter(|end| *end <= self.data.len()).ok_or("inline EOF")?;
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn name(&mut self, len: usize) -> Result<String, String> {
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }

    fn number(&mut self, tag: u8) -> Result<Value, String> {
        let Some(size) = inline_width(tag) else {
            return Err(format!("unknown inline primitive {tag}"));
        };
        Ok(Value::Primitive(u32::from(tag), self.take(size)?.to_vec()))
    }

    fn value(&mut self, depth: u32) -> Result<Value, String> {
        if depth > MAX_RECURSION {
            return Err("max recursion depth exceeded".into());
        }
        match self.u8()? {
            0x10 => {
                let len = self.u32()? as usize;
                Ok(Value::String(self.take(len)?.to_vec()))
            }
            0x12 | 0x14 => self.maybe_record(depth),
            0x13 | 0x11 => self.list(depth),
            0x00 => Ok(Value::Null),
            tag => self.number(tag),
        }
    }

    fn maybe_record(&mut self, depth: u32) -> Result<Value, String> {
        if self.data.get(self.pos) == Some(&0xFF) {
            self.pos += 1;
            return Ok(Value::Object(None));
        }
        Ok(Value::Object(Some(self.record(depth + 1)?)))
    }

    fn list(&mut self, depth: u32) -> Result<Value, String> {
        let _ = self.u32()?;
        let count = self.u32()? as usize;
        if count > MAX_ARRAY_ITEMS as usize {
            return Err("inline array too large".into());
        }
        let mut items = Vec::new();
        for _ in 0..count {
            match self.u8()? {
                0x12 | 0x14 => items.push(self.maybe_record(depth)?),
                0x10 => {
                    let len = self.u32()? as usize;
                    items.push(Value::String(self.take(len)?.to_vec()));
                }
                tag => items.push(self.number(tag)?),
            }
        }
        Ok(Value::Array(items))
    }

    fn record(&mut self, depth: u32) -> Result<UNode, String> {
        if depth > MAX_RECURSION {
            return Err("max recursion depth exceeded".into());
        }
        let _ = self.u8()?;
        let _ = self.u32()?;
        let type_len = self.u32()? as usize;
        let type_name = self.name(type_len)?;
        let field_count = self.u16()? as usize;
        let mut fields = Vec::new();
        for _ in 0..field_count {
            let field_len = self.u32()? as usize;
            let field_name = self.name(field_len)?;
            fields.push((field_name, self.value(depth + 1)?));
        }
        Ok(UNode { type_name, fields })
    }
}

fn inline_width(tag: u8) -> Option<usize> {
    Some(match tag {
        1 | 8 => 1,
        2 => 2,
        3 | 4 | 7 | 9 | 11 => 4,
        5 | 6 | 10 | 12 | 15 => 8,
        13 => 36,
        14 => 64,
        _ => return None,
    })
}

pub fn field<'a>(node: &'a UNode, name: &str) -> Option<&'a Value> {
    node.fields.iter().find(|(found, _)| found == name).map(|(_, value)| value)
}

pub fn object_val(val: &Value) -> Option<&UNode> {
    match val {
        Value::Object(Some(node)) => Some(node),
        _ => None,
    }
}

pub fn array_items(val: &Value) -> &[Value] {
    match val {
        Value::Array(items) => items.as_slice(),
        _ => &[],
    }
}

pub fn string_val(val: &Value) -> Option<String> {
    match val {
        Value::String(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    }
}

pub fn prim_bytes(val: &Value) -> Option<&[u8]> {
    match val {
        Value::Primitive(_, bytes) => Some(bytes.as_slice()),
        _ => None,
    }
}

pub fn prim_int(val: &Value) -> Option<i64> {
    let bytes = prim_bytes(val)?;
    match bytes.len() {
        1 => Some(bytes[0] as i8 as i64),
        2 => crate::pod::le_i16(bytes, 0).map(|value| value as i64),
        4 => crate::pod::le_i32(bytes, 0).map(|value| value as i64),
        8 => crate::pod::le_i64(bytes, 0),
        _ => {
            let mut wide = [0u8; 8];
            let n = bytes.len().min(8);
            wide[..n].copy_from_slice(&bytes[..n]);
            Some(i64::from_le_bytes(wide))
        }
    }
}

pub fn prim_bool(val: &Value) -> Option<bool> {
    prim_int(val).map(|value| value != 0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn hostile_string_length_is_an_error() {
        let raw = u32::MAX.to_le_bytes();
        let mut reader = Reader { src: Cursor::new(raw.as_slice()), types: Vec::new() };
        let err = reader.text().unwrap_err();
        assert!(err.contains("string length"), "{err}");
    }

    #[test]
    fn truncated_inline_object_does_not_panic() {
        // 12-byte prefix, a root object with one field whose tag is an object at the last byte.
        let mut raw = vec![0u8; 12];
        raw.push(0x12);
        raw.extend([0u8; 4]);
        raw.extend(0u32.to_le_bytes());
        raw.extend(1u16.to_le_bytes());
        raw.extend(0u32.to_le_bytes());
        raw.push(0x12);
        let err = read_inline(&raw).unwrap_err();
        assert!(err.contains("EOF") || err.contains("recursion") || err.contains("object"), "{err}");
    }
}
