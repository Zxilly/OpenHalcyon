//! Build 147219 field layouts. A borrowed view keeps padding and exact float bits.
use crate::{Message, Opcode};

mod catalog;
pub use catalog::CATALOG;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    ClientToServer,
    ServerToClient,
    Bidirectional,
    Reserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Transport,
    Handshake,
    Draft,
    Loading,
    Combat,
    Settlement,
}

pub struct Entry {
    pub opcode: Opcode,
    pub name: &'static str,
    pub direction: Direction,
    pub phase: Phase,
    pub layout: Option<Layout>,
}

impl Entry {
    pub fn lookup(opcode: u16) -> Option<&'static Self> {
        let index = CATALOG
            .binary_search_by_key(&opcode, |entry| entry.opcode.raw())
            .ok()?;
        CATALOG.get(index)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub body_bytes: usize,
    pub fields: &'static [Field],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub offset: usize,
    pub width: usize,
    pub kind: FieldType,
}

impl Field {
    const fn new(name: &'static str, offset: usize, width: usize, kind: FieldType) -> Self {
        Self {
            name,
            offset,
            width,
            kind,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    U8,
    U16,
    U32,
    U32Le,
    I32,
    F16,
    F32,
    Text,
    Bytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value<'a> {
    Unsigned(u32),
    Signed(i32),
    Float16Bits(u16),
    Float32Bits(u32),
    Text(&'a [u8]),
    Bytes(&'a [u8]),
}

#[derive(Debug, PartialEq, Eq)]
pub struct View<'a> {
    layout: Layout,
    body: &'a [u8],
}

impl Message {
    pub fn spec_view(&self) -> Option<View<'_>> {
        let layout = Entry::lookup(self.opcode)?.layout?;
        let native = layout.body_bytes;
        let padded = (native + 2).next_multiple_of(8) - 2;
        (self.body.len() == native || self.body.len() == padded).then_some(View {
            layout,
            body: &self.body,
        })
    }
}

impl<'a> View<'a> {
    pub fn padding(&self) -> &'a [u8] {
        &self.body[self.layout.body_bytes..]
    }

    pub fn fields(&self) -> impl Iterator<Item = (&'static Field, Value<'a>)> + '_ {
        self.layout.fields.iter().map(|f| {
            let raw = &self.body[f.offset..f.offset + f.width];
            let value = match f.kind {
                FieldType::U8 => Value::Unsigned(raw[0].into()),
                FieldType::U16 => {
                    Value::Unsigned(u16::from_be_bytes(raw.try_into().unwrap()).into())
                }
                FieldType::U32 => Value::Unsigned(u32::from_be_bytes(raw.try_into().unwrap())),
                FieldType::U32Le => Value::Unsigned(u32::from_le_bytes(raw.try_into().unwrap())),
                FieldType::I32 => Value::Signed(i32::from_be_bytes(raw.try_into().unwrap())),
                FieldType::F16 => Value::Float16Bits(u16::from_be_bytes(raw.try_into().unwrap())),
                FieldType::F32 => Value::Float32Bits(u32::from_be_bytes(raw.try_into().unwrap())),
                FieldType::Text => Value::Text(raw.split(|&b| b == 0).next().unwrap_or_default()),
                FieldType::Bytes => Value::Bytes(raw),
            };
            (f, value)
        })
    }
}
