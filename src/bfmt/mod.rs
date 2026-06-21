use crate::dsl::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use thiserror::Error;

pub const MAGIC: [u8; 4] = *b"BFMT";
pub const VERSION: u16 = 1;
pub const FLAG_DEBUG: u16 = 0x0001;
pub const INVALID_INDEX: u32 = 0xFFFFFFFF;

pub const TYPE_U8: u8 = 0;
pub const TYPE_U16: u8 = 1;
pub const TYPE_U32: u8 = 2;
pub const TYPE_U64: u8 = 3;
pub const TYPE_I8: u8 = 4;
pub const TYPE_I16: u8 = 5;
pub const TYPE_I32: u8 = 6;
pub const TYPE_I64: u8 = 7;
pub const TYPE_F32: u8 = 8;
pub const TYPE_F64: u8 = 9;
pub const TYPE_BYTES: u8 = 10;
pub const TYPE_STRING: u8 = 11;
pub const TYPE_BIT_FIELD: u8 = 12;
pub const TYPE_STRUCT: u8 = 13;
pub const TYPE_ARRAY: u8 = 14;
pub const TYPE_ENUM: u8 = 15;

pub const UNDERLYING_U8: u8 = 0;
pub const UNDERLYING_U16: u8 = 1;
pub const UNDERLYING_U32: u8 = 2;
pub const UNDERLYING_U64: u8 = 3;

pub const ENDIAN_LITTLE: u8 = 0;
pub const ENDIAN_BIG: u8 = 1;

pub const FMT_HEX: u8 = 0;
pub const FMT_DEC: u8 = 1;
pub const FMT_BIN: u8 = 2;
pub const FMT_ASCII: u8 = 3;
pub const FMT_UTF8: u8 = 4;

pub const CHECKSUM_NONE: u8 = 0xFF;

#[derive(Error, Debug)]
pub enum BfmtError {
    #[error("Invalid magic at offset 0x{offset:08X}: expected {expected:?}, got {got:?}")]
    InvalidMagic {
        offset: usize,
        expected: [u8; 4],
        got: [u8; 4],
    },
    #[error("Invalid version at offset 0x{offset:08X}: expected {expected}, got {got}")]
    InvalidVersion {
        offset: usize,
        expected: u16,
        got: u16,
    },
    #[error("Unexpected EOF at offset 0x{offset:08X}: needed {needed} bytes, only {available} available")]
    UnexpectedEof {
        offset: usize,
        needed: usize,
        available: usize,
    },
    #[error("Invalid string index at offset 0x{offset:08X}: index {index} out of bounds (string table size {table_size})")]
    InvalidStringIndex {
        offset: usize,
        index: u32,
        table_size: usize,
    },
    #[error("Invalid data type code {code} at offset 0x{offset:08X}")]
    InvalidDataType {
        offset: usize,
        code: u8,
    },
    #[error("Invalid underlying type {code} at offset 0x{offset:08X}")]
    InvalidUnderlyingType {
        offset: usize,
        code: u8,
    },
    #[error("Invalid endian code {code} at offset 0x{offset:08X}")]
    InvalidEndian {
        offset: usize,
        code: u8,
    },
    #[error("Invalid display format code {code} at offset 0x{offset:08X}")]
    InvalidDisplayFormat {
        offset: usize,
        code: u8,
    },
    #[error("Invalid checksum algorithm code {code} at offset 0x{offset:08X}")]
    InvalidChecksumAlgorithm {
        offset: usize,
        code: u8,
    },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("UTF-8 error at offset 0x{offset:08X}: {error}")]
    Utf8Error {
        offset: usize,
        error: std::str::Utf8Error,
    },
    #[error("Invalid string table at offset 0x{offset:08X}: {message}")]
    InvalidStringTable {
        offset: usize,
        message: String,
    },
}

struct StringTable {
    strings: Vec<String>,
    index_map: HashMap<String, u32>,
}

impl StringTable {
    fn new() -> Self {
        StringTable {
            strings: Vec::new(),
            index_map: HashMap::new(),
        }
    }

    fn insert(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.index_map.get(s) {
            return idx;
        }
        let idx = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.index_map.insert(s.to_string(), idx);
        idx
    }

    fn get(&self, index: u32, offset: usize) -> Result<&str, BfmtError> {
        self.strings
            .get(index as usize)
            .map(|s| s.as_str())
            .ok_or(BfmtError::InvalidStringIndex {
                offset,
                index,
                table_size: self.strings.len(),
            })
    }



    fn encoded_size(&self) -> usize {
        let mut size = 4;
        for s in &self.strings {
            size += 2 + s.len();
        }
        size
    }

    fn encode<W: Write>(&self, writer: &mut W) -> Result<(), BfmtError> {
        writer.write_all(&(self.strings.len() as u32).to_le_bytes())?;
        for s in &self.strings {
            let bytes = s.as_bytes();
            writer.write_all(&(bytes.len() as u16).to_le_bytes())?;
            writer.write_all(bytes)?;
        }
        Ok(())
    }

    fn decode<R: Read>(reader: &mut R, offset: &mut usize) -> Result<(Self, usize), BfmtError> {
        let mut count_buf = [0u8; 4];
        read_exact(reader, &mut count_buf, offset)?;
        let count = u32::from_le_bytes(count_buf) as usize;
        
        let mut table = StringTable::new();
        for _ in 0..count {
            let mut len_bytes = [0u8; 2];
            read_exact(reader, &mut len_bytes, offset)?;
            let len = u16::from_le_bytes(len_bytes) as usize;
            if len == 0 {
                return Err(BfmtError::InvalidStringTable {
                    offset: *offset - 2,
                    message: "zero-length string".to_string(),
                });
            }
            let mut bytes = vec![0u8; len];
            read_exact(reader, &mut bytes, offset)?;
            let s = String::from_utf8(bytes).map_err(|e| BfmtError::Utf8Error {
                offset: *offset - len,
                error: e.utf8_error(),
            })?;
            table.insert(&s);
        }
        Ok((table, count))
    }
}

fn checksum_algorithm_to_code(algo: &str) -> Result<u8, BfmtError> {
    match algo.to_lowercase().as_str() {
        "crc32" => Ok(0),
        "crc16" => Ok(1),
        "adler32" => Ok(2),
        _ => Err(BfmtError::InvalidChecksumAlgorithm {
            offset: 0,
            code: 0,
        }),
    }
}

fn code_to_checksum_algorithm(code: u8, offset: usize) -> Result<String, BfmtError> {
    match code {
        0 => Ok("crc32".to_string()),
        1 => Ok("crc16".to_string()),
        2 => Ok("adler32".to_string()),
        _ => Err(BfmtError::InvalidChecksumAlgorithm { offset, code }),
    }
}

fn underlying_type_to_code(dt: &DataType, offset: usize) -> Result<u8, BfmtError> {
    match dt {
        DataType::U8 => Ok(UNDERLYING_U8),
        DataType::U16 => Ok(UNDERLYING_U16),
        DataType::U32 => Ok(UNDERLYING_U32),
        DataType::U64 => Ok(UNDERLYING_U64),
        _ => Err(BfmtError::InvalidUnderlyingType { offset, code: 0 }),
    }
}

fn code_to_underlying_type(code: u8, offset: usize) -> Result<DataType, BfmtError> {
    match code {
        UNDERLYING_U8 => Ok(DataType::U8),
        UNDERLYING_U16 => Ok(DataType::U16),
        UNDERLYING_U32 => Ok(DataType::U32),
        UNDERLYING_U64 => Ok(DataType::U64),
        _ => Err(BfmtError::InvalidUnderlyingType { offset, code }),
    }
}

fn endian_to_code(e: Endian) -> u8 {
    match e {
        Endian::Little => ENDIAN_LITTLE,
        Endian::Big => ENDIAN_BIG,
    }
}

fn code_to_endian(code: u8, offset: usize) -> Result<Endian, BfmtError> {
    match code {
        ENDIAN_LITTLE => Ok(Endian::Little),
        ENDIAN_BIG => Ok(Endian::Big),
        _ => Err(BfmtError::InvalidEndian { offset, code }),
    }
}

fn display_format_to_code(df: DisplayFormat) -> u8 {
    match df {
        DisplayFormat::Hex => FMT_HEX,
        DisplayFormat::Dec => FMT_DEC,
        DisplayFormat::Bin => FMT_BIN,
        DisplayFormat::Ascii => FMT_ASCII,
        DisplayFormat::Utf8 => FMT_UTF8,
    }
}

fn code_to_display_format(code: u8, offset: usize) -> Result<DisplayFormat, BfmtError> {
    match code {
        FMT_HEX => Ok(DisplayFormat::Hex),
        FMT_DEC => Ok(DisplayFormat::Dec),
        FMT_BIN => Ok(DisplayFormat::Bin),
        FMT_ASCII => Ok(DisplayFormat::Ascii),
        FMT_UTF8 => Ok(DisplayFormat::Utf8),
        _ => Err(BfmtError::InvalidDisplayFormat { offset, code }),
    }
}

fn data_type_to_code(dt: &DataType) -> u8 {
    match dt {
        DataType::U8 => TYPE_U8,
        DataType::U16 => TYPE_U16,
        DataType::U32 => TYPE_U32,
        DataType::U64 => TYPE_U64,
        DataType::I8 => TYPE_I8,
        DataType::I16 => TYPE_I16,
        DataType::I32 => TYPE_I32,
        DataType::I64 => TYPE_I64,
        DataType::F32 => TYPE_F32,
        DataType::F64 => TYPE_F64,
        DataType::Bytes { .. } => TYPE_BYTES,
        DataType::String { .. } => TYPE_STRING,
        DataType::BitField { .. } => TYPE_BIT_FIELD,
        DataType::Struct { .. } => TYPE_STRUCT,
        DataType::Array { .. } => TYPE_ARRAY,
        DataType::Enum { .. } => TYPE_ENUM,
    }
}

fn collect_strings_from_data_type(dt: &DataType, table: &mut StringTable) {
    match dt {
        DataType::Bytes { length } => {
            table.insert(length);
        }
        DataType::String { length, .. } => {
            table.insert(length);
        }
        DataType::Struct { name } => {
            table.insert(name);
        }
        DataType::Array { element_type, length } => {
            collect_strings_from_data_type(element_type, table);
            table.insert(length);
        }
        DataType::Enum { name, .. } => {
            table.insert(name);
        }
        _ => {}
    }
}

fn collect_strings_from_field(field: &Field, table: &mut StringTable) {
    table.insert(&field.name);
    if let Some(offset) = &field.offset {
        if offset != "relative" {
            table.insert(offset);
        }
    }
    collect_strings_from_data_type(&field.data_type, table);
    if let Some(cond) = &field.condition {
        table.insert(&cond.expression);
    }
    if let Some(cs) = &field.checksum {
        table.insert(&cs.start);
        table.insert(&cs.end);
    }
}

fn collect_strings_from_struct(s: &StructDefinition, table: &mut StringTable) {
    table.insert(&s.name);
    for field in &s.fields {
        collect_strings_from_field(field, table);
    }
}

fn collect_strings_from_enum(e: &EnumDefinition, table: &mut StringTable) {
    table.insert(&e.name);
    for name in e.values.keys() {
        table.insert(name);
    }
}

fn build_string_table(def: &FormatDefinition) -> StringTable {
    let mut table = StringTable::new();
    table.insert(&def.name);
    for e in &def.enums {
        collect_strings_from_enum(e, &mut table);
    }
    for s in &def.structs {
        collect_strings_from_struct(s, &mut table);
    }
    collect_strings_from_struct(&def.root, &mut table);
    table
}

fn infer_enum_underlying_type(def: &FormatDefinition, enum_name: &str) -> u8 {
    let mut underlying = UNDERLYING_U32;
    let all_structs: Vec<&StructDefinition> = def
        .structs
        .iter()
        .chain(std::iter::once(&def.root))
        .collect();
    for s in all_structs {
        for field in &s.fields {
            if let DataType::Enum { name, underlying: dt } = &field.data_type {
                if name == enum_name {
                    if let Ok(code) = underlying_type_to_code(dt, 0) {
                        underlying = code;
                    }
                }
            }
            if let DataType::Array { element_type, .. } = &field.data_type {
                if let DataType::Enum { name, underlying: dt } = &**element_type {
                    if name == enum_name {
                        if let Ok(code) = underlying_type_to_code(dt, 0) {
                            underlying = code;
                        }
                    }
                }
            }
        }
    }
    underlying
}

fn encode_data_type<W: Write>(
    dt: &DataType,
    table: &StringTable,
    writer: &mut W,
    offset: &mut usize,
) -> Result<(), BfmtError> {
    let code = data_type_to_code(dt);
    writer.write_all(&[code])?;
    *offset += 1;
    match dt {
        DataType::Bytes { length } => {
            let idx = table.index_map.get(length).copied().unwrap_or(INVALID_INDEX);
            writer.write_all(&idx.to_le_bytes())?;
            *offset += 4;
        }
        DataType::String { length, .. } => {
            let idx = table.index_map.get(length).copied().unwrap_or(INVALID_INDEX);
            writer.write_all(&idx.to_le_bytes())?;
            *offset += 4;
        }
        DataType::BitField { bit_start, bit_length } => {
            writer.write_all(&[*bit_start, *bit_length])?;
            *offset += 2;
        }
        DataType::Struct { name } => {
            let idx = table.index_map.get(name).copied().unwrap_or(INVALID_INDEX);
            writer.write_all(&idx.to_le_bytes())?;
            *offset += 4;
        }
        DataType::Array { element_type, length } => {
            encode_data_type(element_type, table, writer, offset)?;
            let idx = table.index_map.get(length).copied().unwrap_or(INVALID_INDEX);
            writer.write_all(&idx.to_le_bytes())?;
            *offset += 4;
        }
        DataType::Enum { name, underlying } => {
            let idx = table.index_map.get(name).copied().unwrap_or(INVALID_INDEX);
            writer.write_all(&idx.to_le_bytes())?;
            *offset += 4;
            let underlying_code = underlying_type_to_code(underlying, *offset)?;
            writer.write_all(&[underlying_code])?;
            *offset += 1;
        }
        _ => {}
    }
    Ok(())
}

fn encode_field<W: Write>(
    field: &Field,
    table: &StringTable,
    writer: &mut W,
    offset: &mut usize,
) -> Result<(), BfmtError> {
    let name_idx = table.index_map.get(&field.name).copied().unwrap_or(0);
    writer.write_all(&name_idx.to_le_bytes())?;
    *offset += 4;
    encode_data_type(&field.data_type, table, writer, offset)?;
    writer.write_all(&[endian_to_code(field.endian)])?;
    *offset += 1;
    writer.write_all(&[display_format_to_code(field.display_format)])?;
    *offset += 1;
    let offset_idx = match field.offset.as_deref() {
        Some("relative") | None => INVALID_INDEX,
        Some(o) => table.index_map.get(o).copied().unwrap_or(INVALID_INDEX),
    };
    writer.write_all(&offset_idx.to_le_bytes())?;
    *offset += 4;
    let cond_idx = field
        .condition
        .as_ref()
        .and_then(|c| table.index_map.get(&c.expression))
        .copied()
        .unwrap_or(INVALID_INDEX);
    writer.write_all(&cond_idx.to_le_bytes())?;
    *offset += 4;
    if let Some(cs) = &field.checksum {
        let algo_code = checksum_algorithm_to_code(&cs.algorithm)?;
        writer.write_all(&[algo_code])?;
        *offset += 1;
        let start_idx = table.index_map.get(&cs.start).copied().unwrap_or(0);
        writer.write_all(&start_idx.to_le_bytes())?;
        *offset += 4;
        let end_idx = table.index_map.get(&cs.end).copied().unwrap_or(0);
        writer.write_all(&end_idx.to_le_bytes())?;
        *offset += 4;
    } else {
        writer.write_all(&[CHECKSUM_NONE])?;
        *offset += 1;
        writer.write_all(&[0u8; 8])?;
        *offset += 8;
    }
    Ok(())
}

fn encode_struct<W: Write>(
    s: &StructDefinition,
    table: &StringTable,
    writer: &mut W,
    offset: &mut usize,
) -> Result<(), BfmtError> {
    let name_idx = table.index_map.get(&s.name).copied().unwrap_or(0);
    writer.write_all(&name_idx.to_le_bytes())?;
    *offset += 4;
    writer.write_all(&(s.fields.len() as u16).to_le_bytes())?;
    *offset += 2;
    for field in &s.fields {
        encode_field(field, table, writer, offset)?;
    }
    Ok(())
}

fn encode_enum<W: Write>(
    e: &EnumDefinition,
    table: &StringTable,
    writer: &mut W,
    offset: &mut usize,
    underlying_code: u8,
) -> Result<(), BfmtError> {
    let name_idx = table.index_map.get(&e.name).copied().unwrap_or(0);
    writer.write_all(&name_idx.to_le_bytes())?;
    *offset += 4;
    writer.write_all(&[underlying_code])?;
    *offset += 1;
    writer.write_all(&(e.values.len() as u16).to_le_bytes())?;
    *offset += 2;
    for (name, value) in &e.values {
        let name_idx = table.index_map.get(name).copied().unwrap_or(0);
        writer.write_all(&name_idx.to_le_bytes())?;
        *offset += 4;
        writer.write_all(&value.to_le_bytes())?;
        *offset += 8;
    }
    Ok(())
}

fn read_exact<R: Read>(
    reader: &mut R,
    buf: &mut [u8],
    offset: &mut usize,
) -> Result<(), BfmtError> {
    let needed = buf.len();
    match reader.read_exact(buf) {
        Ok(()) => {
            *offset += needed;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            Err(BfmtError::UnexpectedEof {
                offset: *offset,
                needed,
                available: 0,
            })
        }
        Err(e) => Err(e.into()),
    }
}

pub fn compile_to_bfmt<W: Write>(
    def: &FormatDefinition,
    writer: &mut W,
    include_debug: bool,
) -> Result<usize, BfmtError> {
    let table = build_string_table(def);
    let mut offset = 0usize;
    writer.write_all(&MAGIC)?;
    offset += 4;
    writer.write_all(&VERSION.to_le_bytes())?;
    offset += 2;
    let flags = if include_debug { FLAG_DEBUG } else { 0 };
    writer.write_all(&flags.to_le_bytes())?;
    offset += 2;
    let struct_count = (def.structs.len() as u32) + 1;
    writer.write_all(&struct_count.to_le_bytes())?;
    offset += 4;
    writer.write_all(&(def.enums.len() as u32).to_le_bytes())?;
    offset += 4;
    table.encode(writer)?;
    offset += table.encoded_size();
    for e in &def.enums {
        let underlying_code = infer_enum_underlying_type(def, &e.name);
        encode_enum(e, &table, writer, &mut offset, underlying_code)?;
    }
    for s in &def.structs {
        encode_struct(s, &table, writer, &mut offset)?;
    }
    encode_struct(&def.root, &table, writer, &mut offset)?;
    let root_name_idx = table.index_map.get(&def.root.name).copied().unwrap_or(0);
    writer.write_all(&root_name_idx.to_le_bytes())?;
    offset += 4;
    let format_name_idx = table.index_map.get(&def.name).copied().unwrap_or(0);
    writer.write_all(&format_name_idx.to_le_bytes())?;
    offset += 4;
    if let Some(magic) = &def.magic {
        writer.write_all(&[magic.len() as u8])?;
        offset += 1;
        writer.write_all(magic)?;
        offset += magic.len();
    } else {
        writer.write_all(&[0u8])?;
        offset += 1;
    }
    if include_debug {
        let debug_info = build_debug_info(def);
        for (_, line) in debug_info {
            writer.write_all(&line.to_le_bytes())?;
            offset += 4;
        }
    }
    Ok(offset)
}

fn build_debug_info(def: &FormatDefinition) -> Vec<(String, u32)> {
    let mut info = Vec::new();
    let all_structs: Vec<&StructDefinition> = def
        .structs
        .iter()
        .chain(std::iter::once(&def.root))
        .collect();
    for s in all_structs {
        for field in &s.fields {
            info.push((format!("{}.{}", s.name, field.name), 0));
        }
    }
    info
}

fn decode_data_type<R: Read>(
    reader: &mut R,
    table: &StringTable,
    offset: &mut usize,
) -> Result<DataType, BfmtError> {
    let mut code_buf = [0u8; 1];
    read_exact(reader, &mut code_buf, offset)?;
    let code = code_buf[0];
    match code {
        TYPE_U8 => Ok(DataType::U8),
        TYPE_U16 => Ok(DataType::U16),
        TYPE_U32 => Ok(DataType::U32),
        TYPE_U64 => Ok(DataType::U64),
        TYPE_I8 => Ok(DataType::I8),
        TYPE_I16 => Ok(DataType::I16),
        TYPE_I32 => Ok(DataType::I32),
        TYPE_I64 => Ok(DataType::I64),
        TYPE_F32 => Ok(DataType::F32),
        TYPE_F64 => Ok(DataType::F64),
        TYPE_BYTES => {
            let mut idx_buf = [0u8; 4];
            read_exact(reader, &mut idx_buf, offset)?;
            let idx = u32::from_le_bytes(idx_buf);
            let length = table.get(idx, *offset - 4)?.to_string();
            Ok(DataType::Bytes { length })
        }
        TYPE_STRING => {
            let mut idx_buf = [0u8; 4];
            read_exact(reader, &mut idx_buf, offset)?;
            let idx = u32::from_le_bytes(idx_buf);
            let length = table.get(idx, *offset - 4)?.to_string();
            Ok(DataType::String {
                length,
                encoding: None,
            })
        }
        TYPE_BIT_FIELD => {
            let mut bl_buf = [0u8; 2];
            read_exact(reader, &mut bl_buf, offset)?;
            Ok(DataType::BitField {
                bit_start: bl_buf[0],
                bit_length: bl_buf[1],
            })
        }
        TYPE_STRUCT => {
            let mut idx_buf = [0u8; 4];
            read_exact(reader, &mut idx_buf, offset)?;
            let idx = u32::from_le_bytes(idx_buf);
            let name = table.get(idx, *offset - 4)?.to_string();
            Ok(DataType::Struct { name })
        }
        TYPE_ARRAY => {
            let element_type = decode_data_type(reader, table, offset)?;
            let mut idx_buf = [0u8; 4];
            read_exact(reader, &mut idx_buf, offset)?;
            let idx = u32::from_le_bytes(idx_buf);
            let length = table.get(idx, *offset - 4)?.to_string();
            Ok(DataType::Array {
                element_type: Box::new(element_type),
                length,
            })
        }
        TYPE_ENUM => {
            let mut idx_buf = [0u8; 4];
            read_exact(reader, &mut idx_buf, offset)?;
            let idx = u32::from_le_bytes(idx_buf);
            let name = table.get(idx, *offset - 4)?.to_string();
            let mut underlying_buf = [0u8; 1];
            read_exact(reader, &mut underlying_buf, offset)?;
            let underlying = Box::new(code_to_underlying_type(underlying_buf[0], *offset - 1)?);
            Ok(DataType::Enum { name, underlying })
        }
        _ => Err(BfmtError::InvalidDataType {
            offset: *offset - 1,
            code,
        }),
    }
}

fn decode_field<R: Read>(
    reader: &mut R,
    table: &StringTable,
    offset: &mut usize,
) -> Result<Field, BfmtError> {
    let mut name_idx_buf = [0u8; 4];
    read_exact(reader, &mut name_idx_buf, offset)?;
    let name_idx = u32::from_le_bytes(name_idx_buf);
    let name = table.get(name_idx, *offset - 4)?.to_string();
    let data_type = decode_data_type(reader, table, offset)?;
    let mut endian_buf = [0u8; 1];
    read_exact(reader, &mut endian_buf, offset)?;
    let endian = code_to_endian(endian_buf[0], *offset - 1)?;
    let mut fmt_buf = [0u8; 1];
    read_exact(reader, &mut fmt_buf, offset)?;
    let display_format = code_to_display_format(fmt_buf[0], *offset - 1)?;
    let mut offset_idx_buf = [0u8; 4];
    read_exact(reader, &mut offset_idx_buf, offset)?;
    let offset_idx = u32::from_le_bytes(offset_idx_buf);
    let field_offset = if offset_idx == INVALID_INDEX {
        Some("relative".to_string())
    } else {
        Some(table.get(offset_idx, *offset - 4)?.to_string())
    };
    let mut cond_idx_buf = [0u8; 4];
    read_exact(reader, &mut cond_idx_buf, offset)?;
    let cond_idx = u32::from_le_bytes(cond_idx_buf);
    let condition = if cond_idx == INVALID_INDEX {
        None
    } else {
        Some(Condition {
            expression: table.get(cond_idx, *offset - 4)?.to_string(),
        })
    };
    let mut algo_buf = [0u8; 1];
    read_exact(reader, &mut algo_buf, offset)?;
    let checksum = if algo_buf[0] == CHECKSUM_NONE {
        let mut _dummy = [0u8; 8];
        read_exact(reader, &mut _dummy, offset)?;
        None
    } else {
        let algorithm = code_to_checksum_algorithm(algo_buf[0], *offset - 1)?;
        let mut start_idx_buf = [0u8; 4];
        read_exact(reader, &mut start_idx_buf, offset)?;
        let start_idx = u32::from_le_bytes(start_idx_buf);
        let start = table.get(start_idx, *offset - 4)?.to_string();
        let mut end_idx_buf = [0u8; 4];
        read_exact(reader, &mut end_idx_buf, offset)?;
        let end_idx = u32::from_le_bytes(end_idx_buf);
        let end = table.get(end_idx, *offset - 4)?.to_string();
        Some(ChecksumField {
            algorithm,
            start,
            end,
        })
    };
    Ok(Field {
        name,
        offset: field_offset,
        data_type,
        endian,
        display_format,
        condition,
        checksum,
        description: None,
    })
}

fn decode_struct<R: Read>(
    reader: &mut R,
    table: &StringTable,
    offset: &mut usize,
) -> Result<StructDefinition, BfmtError> {
    let mut name_idx_buf = [0u8; 4];
    read_exact(reader, &mut name_idx_buf, offset)?;
    let name_idx = u32::from_le_bytes(name_idx_buf);
    let name = table.get(name_idx, *offset - 4)?.to_string();
    let mut field_count_buf = [0u8; 2];
    read_exact(reader, &mut field_count_buf, offset)?;
    let field_count = u16::from_le_bytes(field_count_buf);
    let mut fields = Vec::with_capacity(field_count as usize);
    for _ in 0..field_count {
        fields.push(decode_field(reader, table, offset)?);
    }
    Ok(StructDefinition {
        name,
        magic: None,
        fields,
    })
}

fn decode_enum<R: Read>(
    reader: &mut R,
    table: &StringTable,
    offset: &mut usize,
) -> Result<EnumDefinition, BfmtError> {
    let mut name_idx_buf = [0u8; 4];
    read_exact(reader, &mut name_idx_buf, offset)?;
    let name_idx = u32::from_le_bytes(name_idx_buf);
    let name = table.get(name_idx, *offset - 4)?.to_string();
    let mut underlying_buf = [0u8; 1];
    read_exact(reader, &mut underlying_buf, offset)?;
    let _underlying_code = underlying_buf[0];
    let mut value_count_buf = [0u8; 2];
    read_exact(reader, &mut value_count_buf, offset)?;
    let value_count = u16::from_le_bytes(value_count_buf);
    let mut values = HashMap::new();
    for _ in 0..value_count {
        let mut name_idx_buf = [0u8; 4];
        read_exact(reader, &mut name_idx_buf, offset)?;
        let name_idx = u32::from_le_bytes(name_idx_buf);
        let value_name = table.get(name_idx, *offset - 4)?.to_string();
        let mut value_buf = [0u8; 8];
        read_exact(reader, &mut value_buf, offset)?;
        let value = i64::from_le_bytes(value_buf);
        values.insert(value_name, value);
    }
    Ok(EnumDefinition { name, values })
}

pub fn load_from_bfmt<R: Read>(reader: &mut R) -> Result<FormatDefinition, BfmtError> {
    let mut offset = 0usize;
    let mut magic_buf = [0u8; 4];
    read_exact(reader, &mut magic_buf, &mut offset)?;
    if magic_buf != MAGIC {
        return Err(BfmtError::InvalidMagic {
            offset: 0,
            expected: MAGIC,
            got: magic_buf,
        });
    }
    let mut version_buf = [0u8; 2];
    read_exact(reader, &mut version_buf, &mut offset)?;
    let version = u16::from_le_bytes(version_buf);
    if version != VERSION {
        return Err(BfmtError::InvalidVersion {
            offset: 4,
            expected: VERSION,
            got: version,
        });
    }
    let mut flags_buf = [0u8; 2];
    read_exact(reader, &mut flags_buf, &mut offset)?;
    let _flags = u16::from_le_bytes(flags_buf);
    let mut struct_count_buf = [0u8; 4];
    read_exact(reader, &mut struct_count_buf, &mut offset)?;
    let struct_count = u32::from_le_bytes(struct_count_buf);
    let mut enum_count_buf = [0u8; 4];
    read_exact(reader, &mut enum_count_buf, &mut offset)?;
    let enum_count = u32::from_le_bytes(enum_count_buf);
    let (table, _) = StringTable::decode(reader, &mut offset)?;
    let mut enums = Vec::with_capacity(enum_count as usize);
    for _ in 0..enum_count {
        enums.push(decode_enum(reader, &table, &mut offset)?);
    }
    let mut structs = Vec::with_capacity((struct_count - 1) as usize);
    for i in 0..struct_count {
        let s = decode_struct(reader, &table, &mut offset)?;
        if i == struct_count - 1 {
            let mut root_name_idx_buf = [0u8; 4];
            read_exact(reader, &mut root_name_idx_buf, &mut offset)?;
            let _root_name_idx = u32::from_le_bytes(root_name_idx_buf);
            let mut format_name_idx_buf = [0u8; 4];
            read_exact(reader, &mut format_name_idx_buf, &mut offset)?;
            let format_name_idx = u32::from_le_bytes(format_name_idx_buf);
            let format_name = table.get(format_name_idx, offset - 4)?.to_string();
            let mut magic_len_buf = [0u8; 1];
            read_exact(reader, &mut magic_len_buf, &mut offset)?;
            let magic_len = magic_len_buf[0] as usize;
            let magic = if magic_len > 0 {
                let mut magic_bytes = vec![0u8; magic_len];
                read_exact(reader, &mut magic_bytes, &mut offset)?;
                Some(magic_bytes)
            } else {
                None
            };
            return Ok(FormatDefinition {
                name: format_name,
                magic,
                enums,
                structs,
                root: s,
            });
        } else {
            structs.push(s);
        }
    }
    Err(BfmtError::UnexpectedEof {
        offset,
        needed: 1,
        available: 0,
    })
}

pub fn decompile_to_yaml(def: &FormatDefinition) -> Result<String, BfmtError> {
    Ok(serde_yaml::to_string(def)?)
}

pub fn count_fields(def: &FormatDefinition) -> usize {
    let mut count = 0;
    for s in &def.structs {
        count += s.fields.len();
    }
    count += def.root.fields.len();
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_format() -> FormatDefinition {
        let yaml = r#"
name: TestFormat
magic: [0xDE, 0xAD, 0xBE, 0xEF]
enums:
  - name: TestEnum
    values:
      VALUE_A: 0
      VALUE_B: 1
      VALUE_C: 2
structs:
  - name: SubStruct
    fields:
      - name: sub_field1
        type: u16
        offset: "0"
        endian: big
        format: hex
      - name: sub_field2
        type: u32
        offset: relative
        format: dec
root:
  name: RootStruct
  fields:
    - name: magic
      type:
        bytes:
          length: "4"
      offset: "0"
      format: hex
    - name: length
      type: u32
      offset: relative
      endian: little
      format: dec
    - name: type_code
      type:
        enum:
          name: TestEnum
          underlying: u8
      offset: relative
    - name: data
      type:
        struct: SubStruct
      offset: relative
      condition:
        when: type_code == 1
    - name: checksum
      type: u32
      offset: relative
      format: hex
      checksum:
        algorithm: crc32
        start: "4"
        end: "12"
"#;
        FormatDefinition::from_yaml(yaml).unwrap()
    }

    #[test]
    fn test_roundtrip_compile_decompile() {
        let def = create_test_format();
        let mut buf = Vec::new();
        let size = compile_to_bfmt(&def, &mut buf, false).unwrap();
        assert!(size > 0);
        assert_eq!(buf.len(), size);
        let def2 = load_from_bfmt(&mut buf.as_slice()).unwrap();
        assert_eq!(def.name, def2.name);
        assert_eq!(def.magic, def2.magic);
        assert_eq!(def.enums.len(), def2.enums.len());
        assert_eq!(def.structs.len(), def2.structs.len());
        assert_eq!(def.root.name, def2.root.name);
        assert_eq!(def.root.fields.len(), def2.root.fields.len());
    }

    #[test]
    fn test_invalid_magic() {
        let invalid_data = b"INVALID";
        let result = load_from_bfmt(&mut invalid_data.as_slice());
        assert!(result.is_err());
        assert!(format!("{}", result.err().unwrap()).contains("Invalid magic"));
    }

    #[test]
    fn test_truncated_header() {
        let invalid_data = b"BFMT";
        let result = load_from_bfmt(&mut invalid_data.as_slice());
        assert!(result.is_err());
        assert!(format!("{}", result.err().unwrap()).contains("Unexpected EOF"));
    }

    #[test]
    fn test_invalid_version() {
        let mut data = Vec::new();
        data.extend_from_slice(&MAGIC);
        data.extend_from_slice(&9999u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        let result = load_from_bfmt(&mut data.as_slice());
        assert!(result.is_err());
        assert!(format!("{}", result.err().unwrap()).contains("Invalid version"));
    }

    #[test]
    fn test_debug_flag() {
        let def = create_test_format();
        let mut buf_debug = Vec::new();
        let size_debug = compile_to_bfmt(&def, &mut buf_debug, true).unwrap();
        let mut buf_normal = Vec::new();
        let size_normal = compile_to_bfmt(&def, &mut buf_normal, false).unwrap();
        assert!(size_debug > size_normal);
        let field_count = count_fields(&def);
        assert_eq!(size_debug - size_normal, field_count * 4);
    }

    #[test]
    fn test_relative_offset_encodes_as_invalid_index() {
        let def = create_test_format();
        let mut buf = Vec::new();
        compile_to_bfmt(&def, &mut buf, false).unwrap();
        let relative_fields: Vec<_> = def.root.fields.iter()
            .filter(|f| f.offset.as_deref() == Some("relative"))
            .collect();
        assert!(!relative_fields.is_empty(), "test format should have relative offset fields");
        let def2 = load_from_bfmt(&mut buf.as_slice()).unwrap();
        for orig in &relative_fields {
            let restored = def2.root.fields.iter().find(|f| f.name == orig.name).unwrap();
            assert_eq!(restored.offset.as_deref(), Some("relative"),
                "field '{}' should have offset=relative after roundtrip", orig.name);
        }
    }

    #[test]
    fn test_decompile_produces_relative_offset_in_yaml() {
        let def = create_test_format();
        let yaml = decompile_to_yaml(&def).unwrap();
        assert!(yaml.contains("offset: relative"),
            "decompiled YAML should contain 'offset: relative', got:\n{}", yaml);
    }
}
