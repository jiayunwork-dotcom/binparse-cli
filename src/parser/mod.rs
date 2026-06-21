use crate::dsl::*;
use anyhow::{anyhow, Result};
use evalexpr::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParserError {
    #[error("File truncated at offset {offset}, needed {needed} bytes but only {available} available")]
    Truncated {
        offset: usize,
        needed: usize,
        available: usize,
    },
    #[error("Field '{0}' depends on truncated field")]
    DependsOnTruncated(String),
    #[error("Expression evaluation failed: {0}")]
    ExpressionError(String),
    #[error("Condition undecidable for field '{0}'")]
    UndecidableCondition(String),
    #[error("Invalid value: {0}")]
    InvalidValue(String),
    #[error("Enum value {value} not found in enum '{enum_name}'")]
    EnumValueNotFound { value: i64, enum_name: String },
    #[error("Unknown struct: {0}")]
    UnknownStruct(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParsedValue {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Bytes(Vec<u8>),
    String(String),
    BitField(u64),
    Enum { name: String, value: i64, display: String },
    Array(Vec<ParsedValue>),
    Struct(Vec<ParsedField>),
}

impl ParsedValue {
    pub fn to_i64(&self) -> Option<i64> {
        match self {
            ParsedValue::U8(v) => Some(*v as i64),
            ParsedValue::U16(v) => Some(*v as i64),
            ParsedValue::U32(v) => Some(*v as i64),
            ParsedValue::U64(v) => Some(*v as i64),
            ParsedValue::I8(v) => Some(*v as i64),
            ParsedValue::I16(v) => Some(*v as i64),
            ParsedValue::I32(v) => Some(*v as i64),
            ParsedValue::I64(v) => Some(*v),
            ParsedValue::BitField(v) => Some(*v as i64),
            ParsedValue::Enum { value, .. } => Some(*value),
            _ => None,
        }
    }

    pub fn to_u64(&self) -> Option<u64> {
        match self {
            ParsedValue::U8(v) => Some(*v as u64),
            ParsedValue::U16(v) => Some(*v as u64),
            ParsedValue::U32(v) => Some(*v as u64),
            ParsedValue::U64(v) => Some(*v),
            ParsedValue::BitField(v) => Some(*v),
            ParsedValue::Enum { value, .. } => Some(*value as u64),
            _ => self.to_i64().map(|v| v as u64),
        }
    }

    pub fn to_f64(&self) -> Option<f64> {
        match self {
            ParsedValue::F32(v) => Some(*v as f64),
            ParsedValue::F64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn display(&self, format: DisplayFormat) -> String {
        match format {
            DisplayFormat::Hex => self.to_hex(),
            DisplayFormat::Dec => self.to_dec(),
            DisplayFormat::Bin => self.to_bin(),
            DisplayFormat::Ascii => self.to_ascii(),
            DisplayFormat::Utf8 => self.to_utf8(),
        }
    }

    fn to_hex(&self) -> String {
        match self {
            ParsedValue::U8(v) => format!("0x{:02X}", v),
            ParsedValue::U16(v) => format!("0x{:04X}", v),
            ParsedValue::U32(v) => format!("0x{:08X}", v),
            ParsedValue::U64(v) => format!("0x{:016X}", v),
            ParsedValue::I8(v) => format!("0x{:02X}", v),
            ParsedValue::I16(v) => format!("0x{:04X}", v),
            ParsedValue::I32(v) => format!("0x{:08X}", v),
            ParsedValue::I64(v) => format!("0x{:016X}", v),
            ParsedValue::F32(v) => format!("{}", v),
            ParsedValue::F64(v) => format!("{}", v),
            ParsedValue::Bytes(v) => format!("[{}]", v.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")),
            ParsedValue::String(v) => v.clone(),
            ParsedValue::BitField(v) => format!("0x{:X}", v),
            ParsedValue::Enum { display, .. } => display.clone(),
            ParsedValue::Array(items) => format!("[{}]", items.iter().map(|i| i.to_hex()).collect::<Vec<_>>().join(", ")),
            ParsedValue::Struct(_) => "<struct>".to_string(),
        }
    }

    fn to_dec(&self) -> String {
        match self {
            ParsedValue::U8(v) => format!("{}", v),
            ParsedValue::U16(v) => format!("{}", v),
            ParsedValue::U32(v) => format!("{}", v),
            ParsedValue::U64(v) => format!("{}", v),
            ParsedValue::I8(v) => format!("{}", v),
            ParsedValue::I16(v) => format!("{}", v),
            ParsedValue::I32(v) => format!("{}", v),
            ParsedValue::I64(v) => format!("{}", v),
            ParsedValue::F32(v) => format!("{}", v),
            ParsedValue::F64(v) => format!("{}", v),
            ParsedValue::Bytes(v) => format!("[{} bytes]", v.len()),
            ParsedValue::String(v) => v.clone(),
            ParsedValue::BitField(v) => format!("{}", v),
            ParsedValue::Enum { display, .. } => display.clone(),
            ParsedValue::Array(items) => format!("[{} items]", items.len()),
            ParsedValue::Struct(_) => "<struct>".to_string(),
        }
    }

    fn to_bin(&self) -> String {
        match self {
            ParsedValue::U8(v) => format!("0b{:08b}", v),
            ParsedValue::U16(v) => format!("0b{:016b}", v),
            ParsedValue::U32(v) => format!("0b{:032b}", v),
            ParsedValue::BitField(v) => format!("0b{:b}", v),
            ParsedValue::Enum { display, .. } => display.clone(),
            _ => self.to_hex(),
        }
    }

    fn to_ascii(&self) -> String {
        let bytes = match self {
            ParsedValue::Bytes(b) => b.as_slice(),
            ParsedValue::String(s) => s.as_bytes(),
            _ => return self.to_dec(),
        };
        bytes.iter().map(|&b| {
            if b >= 0x20 && b < 0x7F {
                b as char
            } else {
                '.'
            }
        }).collect()
    }

    fn to_utf8(&self) -> String {
        match self {
            ParsedValue::String(v) => v.clone(),
            ParsedValue::Bytes(v) => String::from_utf8_lossy(v).to_string(),
            _ => self.to_dec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedField {
    pub name: String,
    pub path: String,
    pub offset: usize,
    pub length: usize,
    pub value: ParsedValue,
    pub display_format: DisplayFormat,
    pub truncated: bool,
    pub undecidable: bool,
    pub skipped: bool,
    pub checksum_result: Option<ChecksumResult>,
    pub children: Vec<ParsedField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChecksumResult {
    Passed,
    Failed { expected: u64, actual: u64 },
}

pub struct Parser<'a> {
    data: &'a [u8],
    format: &'a FormatDefinition,
    context: HashMap<String, Value>,
    current_offset: usize,
    has_checksum_failure: bool,
    truncated_occurred: bool,
}

impl<'a> Parser<'a> {
    pub fn new(data: &'a [u8], format: &'a FormatDefinition) -> Self {
        Self {
            data,
            format,
            context: HashMap::new(),
            current_offset: 0,
            has_checksum_failure: false,
            truncated_occurred: false,
        }
    }

    pub fn parse(&mut self) -> Result<ParsedField> {
        let root_struct = &self.format.root;
        self.parse_struct(root_struct, "", 0)
    }

    pub fn has_checksum_failure(&self) -> bool {
        self.has_checksum_failure
    }

    fn parse_struct(
        &mut self,
        struct_def: &StructDefinition,
        parent_path: &str,
        base_offset: usize,
    ) -> Result<ParsedField> {
        let path = if parent_path.is_empty() {
            struct_def.name.clone()
        } else {
            format!("{}.{}", parent_path, struct_def.name)
        };

        let mut fields = Vec::new();
        let mut struct_offset = base_offset;

        for field_def in &struct_def.fields {
            let field_path = format!("{}.{}", path, field_def.name);
            
            if self.truncated_occurred {
                fields.push(ParsedField {
                    name: field_def.name.clone(),
                    path: field_path,
                    offset: struct_offset,
                    length: 0,
                    value: ParsedValue::U8(0),
                    display_format: field_def.display_format,
                    truncated: true,
                    undecidable: false,
                    skipped: false,
                    checksum_result: None,
                    children: Vec::new(),
                });
                continue;
            }
            
            if let Some(cond) = &field_def.condition {
                match self.eval_condition(&cond.expression) {
                    Ok(true) => {}
                    Ok(false) => {
                        fields.push(ParsedField {
                            name: field_def.name.clone(),
                            path: field_path,
                            offset: struct_offset,
                            length: 0,
                            value: ParsedValue::U8(0),
                            display_format: field_def.display_format,
                            truncated: false,
                            undecidable: false,
                            skipped: true,
                            checksum_result: None,
                            children: Vec::new(),
                        });
                        continue;
                    }
                    Err(_) => {
                        fields.push(ParsedField {
                            name: field_def.name.clone(),
                            path: field_path,
                            offset: struct_offset,
                            length: 0,
                            value: ParsedValue::U8(0),
                            display_format: field_def.display_format,
                            truncated: false,
                            undecidable: true,
                            skipped: true,
                            checksum_result: None,
                            children: Vec::new(),
                        });
                        continue;
                    }
                }
            }

            let field_offset = match &field_def.offset {
                Some(offset_str) if offset_str == "relative" => struct_offset,
                Some(offset_str) => match self.eval_expression_usize(offset_str) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(ParserError::DependsOnTruncated(field_def.name.clone()).into());
                    }
                },
                None => struct_offset,
            };

            let field_path_clone = field_path.clone();
            let parsed_field = match self.parse_field(field_def, &field_path, field_offset) {
                Ok(f) => f,
                Err(e) => {
                    if let Some(ParserError::Truncated { .. }) = e.downcast_ref::<ParserError>() {
                        self.truncated_occurred = true;
                        ParsedField {
                            name: field_def.name.clone(),
                            path: field_path_clone,
                            offset: field_offset,
                            length: 0,
                            value: ParsedValue::U8(0),
                            display_format: field_def.display_format,
                            truncated: true,
                            undecidable: false,
                            skipped: false,
                            checksum_result: None,
                            children: Vec::new(),
                        }
                    } else {
                        return Err(e);
                    }
                }
            };

            if !parsed_field.truncated && !parsed_field.skipped {
                if let Some(int_val) = parsed_field.value.to_i64() {
                    self.context.insert(field_path.clone(), Value::Int(int_val));
                }
                if let Some(float_val) = parsed_field.value.to_f64() {
                    self.context.insert(field_path.clone(), Value::Float(float_val));
                }
            }

            struct_offset = parsed_field.offset + parsed_field.length;
            self.current_offset = struct_offset;
            fields.push(parsed_field);
        }

        let total_length = if fields.is_empty() {
            0
        } else {
            fields.last().unwrap().offset + fields.last().unwrap().length - fields.first().unwrap().offset
        };

        Ok(ParsedField {
            name: struct_def.name.clone(),
            path,
            offset: base_offset,
            length: total_length,
            value: ParsedValue::Struct(fields.clone()),
            display_format: DisplayFormat::Hex,
            truncated: false,
            undecidable: false,
            skipped: false,
            checksum_result: None,
            children: fields,
        })
    }

    fn parse_field(
        &mut self,
        field_def: &Field,
        field_path: &str,
        offset: usize,
    ) -> Result<ParsedField> {
        let (value, length) = self.parse_value(&field_def.data_type, field_def.endian, offset, field_path)?;

        let checksum_result = if let Some(checksum_def) = &field_def.checksum {
            let start = self.eval_expression_usize(&checksum_def.start)?;
            let end = self.eval_expression_usize(&checksum_def.end)?;
            
            if start < end && end <= self.data.len() {
                let data_range = &self.data[start..end];
                let actual = match checksum_def.algorithm.as_str() {
                    "crc32" => crate::checksum::crc32(data_range) as u64,
                    "adler32" => crate::checksum::adler32(data_range) as u64,
                    "sum" => crate::checksum::simple_sum(data_range) as u64,
                    _ => return Err(anyhow!("Unknown checksum algorithm: {}", checksum_def.algorithm)),
                };
                let expected = value.to_u64().unwrap_or(0);
                
                let result = if actual == expected {
                    ChecksumResult::Passed
                } else {
                    self.has_checksum_failure = true;
                    ChecksumResult::Failed { expected, actual }
                };
                Some(result)
            } else {
                None
            }
        } else {
            None
        };

        let display_value = match &field_def.data_type {
            DataType::Enum { name, .. } => {
                if let Some(enum_def) = self.format.get_enum(name) {
                    if let Some(int_val) = value.to_i64() {
                        let mut display = int_val.to_string();
                        for (k, v) in &enum_def.values {
                            if *v == int_val {
                                display = format!("{} ({})", k, int_val);
                                break;
                            }
                        }
                        ParsedValue::Enum {
                            name: name.clone(),
                            value: int_val,
                            display,
                        }
                    } else {
                        value
                    }
                } else {
                    value
                }
            }
            _ => value,
        };

        Ok(ParsedField {
            name: field_def.name.clone(),
            path: field_path.to_string(),
            offset,
            length,
            value: display_value,
            display_format: field_def.display_format,
            truncated: false,
            undecidable: false,
            skipped: false,
            checksum_result,
            children: Vec::new(),
        })
    }

    fn parse_value(
        &mut self,
        data_type: &DataType,
        endian: Endian,
        offset: usize,
        path: &str,
    ) -> Result<(ParsedValue, usize)> {
        match data_type {
            DataType::U8 => {
                self.check_available(offset, 1)?;
                Ok((ParsedValue::U8(self.data[offset]), 1))
            }
            DataType::U16 => {
                self.check_available(offset, 2)?;
                let bytes = &self.data[offset..offset + 2];
                let val = match endian {
                    Endian::Little => u16::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => u16::from_be_bytes(bytes.try_into().unwrap()),
                };
                Ok((ParsedValue::U16(val), 2))
            }
            DataType::U32 => {
                self.check_available(offset, 4)?;
                let bytes = &self.data[offset..offset + 4];
                let val = match endian {
                    Endian::Little => u32::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => u32::from_be_bytes(bytes.try_into().unwrap()),
                };
                Ok((ParsedValue::U32(val), 4))
            }
            DataType::U64 => {
                self.check_available(offset, 8)?;
                let bytes = &self.data[offset..offset + 8];
                let val = match endian {
                    Endian::Little => u64::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => u64::from_be_bytes(bytes.try_into().unwrap()),
                };
                Ok((ParsedValue::U64(val), 8))
            }
            DataType::I8 => {
                self.check_available(offset, 1)?;
                Ok((ParsedValue::I8(self.data[offset] as i8), 1))
            }
            DataType::I16 => {
                self.check_available(offset, 2)?;
                let bytes = &self.data[offset..offset + 2];
                let val = match endian {
                    Endian::Little => i16::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => i16::from_be_bytes(bytes.try_into().unwrap()),
                };
                Ok((ParsedValue::I16(val), 2))
            }
            DataType::I32 => {
                self.check_available(offset, 4)?;
                let bytes = &self.data[offset..offset + 4];
                let val = match endian {
                    Endian::Little => i32::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => i32::from_be_bytes(bytes.try_into().unwrap()),
                };
                Ok((ParsedValue::I32(val), 4))
            }
            DataType::I64 => {
                self.check_available(offset, 8)?;
                let bytes = &self.data[offset..offset + 8];
                let val = match endian {
                    Endian::Little => i64::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => i64::from_be_bytes(bytes.try_into().unwrap()),
                };
                Ok((ParsedValue::I64(val), 8))
            }
            DataType::F32 => {
                self.check_available(offset, 4)?;
                let bytes = &self.data[offset..offset + 4];
                let val = match endian {
                    Endian::Little => f32::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => f32::from_be_bytes(bytes.try_into().unwrap()),
                };
                Ok((ParsedValue::F32(val), 4))
            }
            DataType::F64 => {
                self.check_available(offset, 8)?;
                let bytes = &self.data[offset..offset + 8];
                let val = match endian {
                    Endian::Little => f64::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => f64::from_be_bytes(bytes.try_into().unwrap()),
                };
                Ok((ParsedValue::F64(val), 8))
            }
            DataType::Bytes { length } => {
                let len = self.eval_expression_usize(length)?;
                self.check_available(offset, len)?;
                let bytes = self.data[offset..offset + len].to_vec();
                Ok((ParsedValue::Bytes(bytes), len))
            }
            DataType::String { length, encoding } => {
                let len = self.eval_expression_usize(length)?;
                self.check_available(offset, len)?;
                let bytes = &self.data[offset..offset + len];
                let s = match encoding.as_deref().unwrap_or("utf8") {
                    "ascii" => bytes.iter().map(|&b| b as char).collect(),
                    "utf8" => String::from_utf8_lossy(bytes).to_string(),
                    _ => String::from_utf8_lossy(bytes).to_string(),
                };
                Ok((ParsedValue::String(s), len))
            }
            DataType::BitField { bit_start, bit_length } => {
                self.check_available(offset, 1)?;
                let byte = self.data[offset];
                let mask = (1 << bit_length) - 1;
                let val = (byte >> bit_start) & mask;
                Ok((ParsedValue::BitField(val as u64), 1))
            }
            DataType::Struct { name } => {
                let struct_def = self.format.get_struct(name)
                    .ok_or_else(|| ParserError::UnknownStruct(name.clone()))?;
                let parsed = self.parse_struct(struct_def, path.rsplitn(2, '.').nth(1).unwrap_or(""), offset)?;
                let len = parsed.length;
                let children = parsed.children;
                Ok((ParsedValue::Struct(children.clone()), len))
            }
            DataType::Array { element_type, length } => {
                let array_len = self.eval_expression_usize(length)?;
                let mut elements = Vec::new();
                let mut current_offset = offset;
                for i in 0..array_len {
                    let elem_path = format!("{}[{}]", path, i);
                    let (val, len) = self.parse_value(element_type, endian, current_offset, &elem_path)?;
                    elements.push(val);
                    current_offset += len;
                }
                Ok((ParsedValue::Array(elements), current_offset - offset))
            }
            DataType::Enum { underlying, .. } => {
                self.parse_value(underlying, endian, offset, path)
            }
        }
    }

    fn check_available(&self, offset: usize, needed: usize) -> Result<()> {
        if offset + needed > self.data.len() {
            return Err(ParserError::Truncated {
                offset,
                needed,
                available: self.data.len() - offset,
            }.into());
        }
        Ok(())
    }

    fn eval_expression_usize(&self, expr: &str) -> Result<usize> {
        if let Ok(n) = expr.parse::<usize>() {
            return Ok(n);
        }

        let mut expr = expr.to_string();
        for (key, val) in &self.context {
            let short_key = key.rsplit('.').next().unwrap_or(key);
            expr = expr.replace(short_key, &val.to_string());
            expr = expr.replace(key, &val.to_string());
        }

        let result = eval(&expr)
            .map_err(|e| ParserError::ExpressionError(format!("{}", e)))?;
        
        if let Ok(int_val) = result.as_int() {
            return Ok(int_val as usize);
        }
        if let Ok(float_val) = result.as_float() {
            return Ok(float_val as usize);
        }
            
        Err(ParserError::ExpressionError(format!("Expression did not return a number: {}", expr)).into())
    }

    fn eval_condition(&self, expr: &str) -> Result<bool> {
        let mut expr = expr.to_string();
        for (key, val) in &self.context {
            let short_key = key.rsplit('.').next().unwrap_or(key);
            expr = expr.replace(short_key, &val.to_string());
            expr = expr.replace(key, &val.to_string());
        }

        let result = eval(&expr)
            .map_err(|e| ParserError::ExpressionError(format!("{}", e)))?;
        
        result.as_boolean()
            .map_err(|_| ParserError::ExpressionError(format!("Condition did not return a boolean: {}", expr)).into())
    }
}

pub fn parse(data: &[u8], format: &FormatDefinition) -> Result<(ParsedField, bool)> {
    let mut parser = Parser::new(data, format);
    let result = parser.parse()?;
    Ok((result, parser.has_checksum_failure()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_format() -> FormatDefinition {
        let yaml = r#"
name: test
enums:
  - name: TestEnum
    values:
      VALUE_A: 1
      VALUE_B: 2
root:
  name: root
  fields:
    - name: magic
      type: u32
      offset: "0"
      endian: big
      format: hex
    - name: length
      type: u16
      offset: relative
      endian: little
    - name: count
      type: u8
      offset: relative
    - name: items
      type:
        array:
          element_type: u8
          length: count
      offset: relative
    - name: enum_val
      type:
        enum:
          name: TestEnum
          underlying: u8
      offset: relative
"#;
        FormatDefinition::from_yaml(yaml).unwrap()
    }

    #[test]
    fn test_parse_simple() {
        let format = create_test_format();
        let data: Vec<u8> = vec![
            0x12, 0x34, 0x56, 0x78, // magic (big endian)
            0x04, 0x00,             // length (little endian) = 4
            0x03,                   // count = 3
            0x10, 0x20, 0x30,       // items[3]
            0x02,                   // enum_val = VALUE_B
        ];

        let (result, has_failures) = parse(&data, &format).unwrap();
        assert!(!has_failures);
        
        if let ParsedValue::Struct(fields) = &result.value {
            assert_eq!(fields.len(), 5);
            assert_eq!(fields[0].name, "magic");
            assert_eq!(fields[0].value, ParsedValue::U32(0x12345678));
            assert_eq!(fields[1].value, ParsedValue::U16(4));
            assert_eq!(fields[2].value, ParsedValue::U8(3));
            
            if let ParsedValue::Array(items) = &fields[3].value {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0], ParsedValue::U8(0x10));
                assert_eq!(items[1], ParsedValue::U8(0x20));
                assert_eq!(items[2], ParsedValue::U8(0x30));
            } else {
                panic!("Expected array");
            }

            if let ParsedValue::Enum { name, value, display } = &fields[4].value {
                assert_eq!(name, "TestEnum");
                assert_eq!(*value, 2);
                assert_eq!(display, "VALUE_B (2)");
            } else {
                panic!("Expected enum");
            }
        } else {
            panic!("Expected struct");
        }
    }

    #[test]
    fn test_truncated_file() {
        let format = create_test_format();
        let data: Vec<u8> = vec![0x12, 0x34]; // Only 2 bytes

        let (result, _) = parse(&data, &format).unwrap();
        if let ParsedValue::Struct(fields) = &result.value {
            assert!(fields[0].truncated);
        }
    }
}
