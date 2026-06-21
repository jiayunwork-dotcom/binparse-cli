use crate::checksum;
use crate::dsl::*;
use crate::parser::*;
use anyhow::{anyhow, Result};
use evalexpr::*;
use hex::FromHex;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct PatchInstruction {
    pub field_path: String,
    pub new_value_str: String,
}

#[derive(Debug, Clone)]
pub struct FieldChange {
    pub field_path: String,
    pub offset: usize,
    pub length: usize,
    pub original_bytes: Vec<u8>,
    pub new_bytes: Vec<u8>,
    pub original_value_display: String,
    pub new_value_display: String,
}

#[derive(Debug, Clone)]
pub struct ChecksumRecalc {
    pub field_path: String,
    pub offset: usize,
    pub algorithm: String,
    pub start: usize,
    pub end: usize,
    pub original_value: u64,
    pub new_value: u64,
}

pub struct PatchResult {
    pub changes: Vec<FieldChange>,
    pub checksum_recalcs: Vec<ChecksumRecalc>,
    pub validation_failures: Vec<(String, String, String)>,
}

pub struct PatchError;

impl PatchError {
    pub const FIELD_NOT_FOUND: i32 = 2;
    pub const FORMAT_ERROR: i32 = 2;
    pub const VALUE_ENCODING_ERROR: i32 = 3;
    pub const VALIDATION_FAILURE: i32 = 1;
    pub const SUCCESS: i32 = 0;
}

pub fn parse_patch_instructions(sets: &[String], patch_file: Option<&Path>) -> Result<Vec<PatchInstruction>> {
    let mut instructions: HashMap<String, String> = HashMap::new();

    if let Some(patch_file_path) = patch_file {
        let content = fs::read_to_string(patch_file_path)
            .map_err(|e| anyhow!("无法读取patch文件: {}", e))?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((path, val)) = line.split_once('=') {
                instructions.insert(path.trim().to_string(), val.trim().to_string());
            }
        }
    }

    for set in sets {
        if let Some((path, val)) = set.split_once('=') {
            instructions.insert(path.trim().to_string(), val.trim().to_string());
        } else {
            return Err(anyhow!("无效的--set参数格式: {}，应为\"字段路径=新值\"", set));
        }
    }

    Ok(instructions
        .into_iter()
        .map(|(field_path, new_value_str)| PatchInstruction { field_path, new_value_str })
        .collect())
}

pub fn find_field_by_path<'a>(root: &'a ParsedField, path: &str) -> Option<&'a ParsedField> {
    if root.path == path {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_field_by_path(child, path) {
            return Some(found);
        }
    }
    None
}

pub fn collect_all_field_paths(root: &ParsedField) -> Vec<String> {
    let mut paths = Vec::new();
    collect_paths_recursive(root, &mut paths);
    paths
}

fn collect_paths_recursive(field: &ParsedField, paths: &mut Vec<String>) {
    if field.children.is_empty() && !field.truncated && !field.skipped {
        paths.push(field.path.clone());
    }
    for child in &field.children {
        collect_paths_recursive(child, paths);
    }
}

fn build_value_context(root: &ParsedField) -> HashMap<String, Value> {
    let mut ctx = HashMap::new();
    build_context_recursive(root, &mut ctx);
    ctx
}

fn build_context_recursive(field: &ParsedField, ctx: &mut HashMap<String, Value>) {
    if !field.truncated && !field.skipped {
        if let Some(int_val) = field.value.to_i64() {
            ctx.insert(field.path.clone(), Value::Int(int_val));
            let short_key = field.path.rsplit('.').next().unwrap_or(&field.path).to_string();
            ctx.insert(short_key, Value::Int(int_val));
        }
        if let Some(float_val) = field.value.to_f64() {
            ctx.insert(field.path.clone(), Value::Float(float_val));
            let short_key = field.path.rsplit('.').next().unwrap_or(&field.path).to_string();
            ctx.insert(short_key, Value::Float(float_val));
        }
    }
    for child in &field.children {
        build_context_recursive(child, ctx);
    }
}

fn eval_expression_usize_with_ctx(expr: &str, ctx: &HashMap<String, Value>) -> Result<usize> {
    if let Ok(n) = expr.parse::<usize>() {
        return Ok(n);
    }
    let mut expr = expr.to_string();
    for (key, val) in ctx {
        expr = expr.replace(key, &val.to_string());
    }
    let result = eval(&expr).map_err(|e| anyhow!("表达式求值失败: {} ({})", expr, e))?;
    if let Ok(int_val) = result.as_int() {
        return Ok(int_val as usize);
    }
    if let Ok(float_val) = result.as_float() {
        return Ok(float_val as usize);
    }
    Err(anyhow!("表达式未返回数值: {}", expr))
}

fn parse_int(s: &str, min: i128, max: i128) -> Result<i128> {
    let s = s.trim();
    let val = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i128::from_str_radix(hex, 16).map_err(|e| anyhow!("十六进制解析失败: {}", e))?
    } else {
        s.parse::<i128>().map_err(|e| anyhow!("十进制解析失败: {}", e))?
    };
    if val < min || val > max {
        return Err(anyhow!("值 {} 超出范围 [{}, {}]", val, min, max));
    }
    Ok(val)
}

fn get_field_endian(field_def: Option<&Field>) -> Endian {
    field_def.map(|f| f.endian).unwrap_or(Endian::Little)
}

pub fn encode_value(
    field: &ParsedField,
    field_def: Option<&Field>,
    new_value_str: &str,
    format_def: &FormatDefinition,
) -> Result<(Vec<u8>, String)> {
    let endian = get_field_endian(field_def);

    match &field.value {
        ParsedValue::U8(_) => {
            let val = parse_int(new_value_str, u8::MIN as i128, u8::MAX as i128)
                .map_err(|e| anyhow!("u8值编码失败: {}", e))? as u8;
            Ok((vec![val], val.to_string()))
        }
        ParsedValue::U16(_) => {
            let val = parse_int(new_value_str, u16::MIN as i128, u16::MAX as i128)
                .map_err(|e| anyhow!("u16值编码失败: {}", e))? as u16;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, val.to_string()))
        }
        ParsedValue::U32(_) => {
            let val = parse_int(new_value_str, u32::MIN as i128, u32::MAX as i128)
                .map_err(|e| anyhow!("u32值编码失败: {}", e))? as u32;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, val.to_string()))
        }
        ParsedValue::U64(_) => {
            let val = parse_int(new_value_str, u64::MIN as i128, u64::MAX as i128)
                .map_err(|e| anyhow!("u64值编码失败: {}", e))? as u64;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, val.to_string()))
        }
        ParsedValue::I8(_) => {
            let val = parse_int(new_value_str, i8::MIN as i128, i8::MAX as i128)
                .map_err(|e| anyhow!("i8值编码失败: {}", e))? as i8;
            Ok((vec![val as u8], val.to_string()))
        }
        ParsedValue::I16(_) => {
            let val = parse_int(new_value_str, i16::MIN as i128, i16::MAX as i128)
                .map_err(|e| anyhow!("i16值编码失败: {}", e))? as i16;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, val.to_string()))
        }
        ParsedValue::I32(_) => {
            let val = parse_int(new_value_str, i32::MIN as i128, i32::MAX as i128)
                .map_err(|e| anyhow!("i32值编码失败: {}", e))? as i32;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, val.to_string()))
        }
        ParsedValue::I64(_) => {
            let val = parse_int(new_value_str, i64::MIN as i128, i64::MAX as i128)
                .map_err(|e| anyhow!("i64值编码失败: {}", e))? as i64;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, val.to_string()))
        }
        ParsedValue::F32(_) => {
            let val: f32 = new_value_str
                .parse()
                .map_err(|e| anyhow!("f32值编码失败: {}", e))?;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, val.to_string()))
        }
        ParsedValue::F64(_) => {
            let val: f64 = new_value_str
                .parse()
                .map_err(|e| anyhow!("f64值编码失败: {}", e))?;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, val.to_string()))
        }
        ParsedValue::Bytes(orig) => {
            let cleaned: String = new_value_str.chars().filter(|c| !c.is_whitespace()).collect();
            let bytes = Vec::<u8>::from_hex(&cleaned)
                .map_err(|e| anyhow!("bytes十六进制解析失败: {}", e))?;
            if bytes.len() != orig.len() {
                return Err(anyhow!(
                    "bytes长度不匹配: 期望{}字节，实际{}字节",
                    orig.len(),
                    bytes.len()
                ));
            }
            let display = bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
            Ok((bytes, format!("[{}]", display)))
        }
        ParsedValue::String(_orig) => {
            let mut utf8_bytes = new_value_str.as_bytes().to_vec();
            let target_len = field.length;
            if utf8_bytes.len() < target_len {
                utf8_bytes.resize(target_len, 0x00);
            } else if utf8_bytes.len() > target_len {
                utf8_bytes.truncate(target_len);
            }
            let display = String::from_utf8_lossy(&utf8_bytes).to_string();
            Ok((utf8_bytes, display))
        }
        ParsedValue::Enum { name, .. } => {
            let enum_def = format_def.get_enum(name)
                .ok_or_else(|| anyhow!("枚举定义未找到: {}", name))?;
            let int_val: i64 = if let Some(&v) = enum_def.values.get(new_value_str) {
                v
            } else {
                parse_int(new_value_str, i64::MIN as i128, i64::MAX as i128)
                    .map_err(|e| anyhow!("枚举值解析失败: {}. 可用值: {:?}", e, enum_def.values.keys().collect::<Vec<_>>()))? as i64
            };
            let display = enum_def.values.iter()
                .find(|(_, &v)| v == int_val)
                .map(|(k, _)| format!("{} ({})", k, int_val))
                .unwrap_or_else(|| int_val.to_string());
            let (bytes, _) = encode_enum_underlying(field_def, int_val)?;
            Ok((bytes, display))
        }
        ParsedValue::BitField(_) => {
            Err(anyhow!("BitField类型暂不支持直接修改"))
        }
        ParsedValue::Array(_) => {
            Err(anyhow!("Array类型暂不支持直接修改，请修改具体的数组元素"))
        }
        ParsedValue::Struct(_) => {
            Err(anyhow!("Struct类型不能直接修改，请修改具体的子字段"))
        }
    }
}

fn encode_enum_underlying(
    field_def: Option<&Field>,
    int_val: i64,
) -> Result<(Vec<u8>, String)> {
    if let Some(fdef) = field_def {
        if let DataType::Enum { underlying, .. } = &fdef.data_type {
            let endian = fdef.endian;
            return encode_int_by_type(underlying.as_ref(), int_val, endian);
        }
    }
    Err(anyhow!("缺少字段定义，无法编码枚举"))
}

fn encode_int_by_type(data_type: &DataType, int_val: i64, endian: Endian) -> Result<(Vec<u8>, String)> {
    match data_type {
        DataType::U8 => {
            if int_val < u8::MIN as i64 || int_val > u8::MAX as i64 {
                return Err(anyhow!("值超出u8范围: {}", int_val));
            }
            Ok((vec![int_val as u8], int_val.to_string()))
        }
        DataType::U16 => {
            if int_val < u16::MIN as i64 || int_val > u16::MAX as i64 {
                return Err(anyhow!("值超出u16范围: {}", int_val));
            }
            let val = int_val as u16;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, int_val.to_string()))
        }
        DataType::U32 => {
            if int_val < u32::MIN as i64 || int_val > u32::MAX as i64 {
                return Err(anyhow!("值超出u32范围: {}", int_val));
            }
            let val = int_val as u32;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, int_val.to_string()))
        }
        DataType::U64 => {
            let val = int_val as u64;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, int_val.to_string()))
        }
        DataType::I8 => {
            if int_val < i8::MIN as i64 || int_val > i8::MAX as i64 {
                return Err(anyhow!("值超出i8范围: {}", int_val));
            }
            Ok((vec![int_val as u8], int_val.to_string()))
        }
        DataType::I16 => {
            if int_val < i16::MIN as i64 || int_val > i16::MAX as i64 {
                return Err(anyhow!("值超出i16范围: {}", int_val));
            }
            let val = int_val as i16;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, int_val.to_string()))
        }
        DataType::I32 => {
            if int_val < i32::MIN as i64 || int_val > i32::MAX as i64 {
                return Err(anyhow!("值超出i32范围: {}", int_val));
            }
            let val = int_val as i32;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, int_val.to_string()))
        }
        DataType::I64 => {
            let val = int_val;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, int_val.to_string()))
        }
        _ => Err(anyhow!("不支持的整数类型")),
    }
}

pub fn find_field_definition<'a>(
    format_def: &'a FormatDefinition,
    struct_def: &'a StructDefinition,
    field_path: &str,
    parent_path: &str,
) -> Option<&'a Field> {
    let current_path = if parent_path.is_empty() {
        struct_def.name.clone()
    } else {
        format!("{}.{}", parent_path, struct_def.name)
    };

    for field in &struct_def.fields {
        let fq_path = format!("{}.{}", current_path, field.name);
        if fq_path == field_path {
            return Some(field);
        }
        match &field.data_type {
            DataType::Struct { name } => {
                if let Some(sd) = format_def.get_struct(name) {
                    if let Some(found) = find_field_definition(format_def, sd, field_path, &current_path) {
                        return Some(found);
                    }
                }
            }
            DataType::Array { element_type, .. } => {
                if let DataType::Struct { name } = element_type.as_ref() {
                    if let Some(sd) = format_def.get_struct(name) {
                        let stripped_path = strip_array_index(field_path);
                        if let Some(sp) = stripped_path {
                            if let Some(found) = find_field_definition(format_def, sd, &sp, &current_path) {
                                return Some(found);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn strip_array_index(path: &str) -> Option<String> {
    let start = path.find('[')?;
    let end = path.find(']')?;
    if end > start {
        let mut result = String::new();
        result.push_str(&path[..start]);
        result.push_str(&path[end + 1..]);
        Some(result)
    } else {
        None
    }
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 { return m; }
    if m == 0 { return n; }
    let mut d = vec![vec![0; m + 1]; n + 1];
    for i in 0..=n { d[i][0] = i; }
    for j in 0..=m { d[0][j] = j; }
    for i in 1..=n {
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            d[i][j] = *[d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + cost].iter().min().unwrap();
        }
    }
    d[n][m]
}

fn suggest_similar_path(target: &str, available: &[String]) -> String {
    let target_lower = target.to_lowercase();
    let mut best: Option<(usize, &String)> = None;
    for p in available {
        let p_lower = p.to_lowercase();
        if p_lower.contains(&target_lower) || target_lower.contains(&p_lower) {
            let dist = levenshtein_distance(&target_lower, &p_lower);
            if let Some((best_dist, _)) = best {
                if dist < best_dist {
                    best = Some((dist, p));
                }
            } else {
                best = Some((dist, p));
            }
        }
    }
    if let Some((_, suggestion)) = best {
        format!("\n\n您可能是想找: {}", suggestion)
    } else {
        String::new()
    }
}

fn encode_checksum_bytes(field: &ParsedField, field_def: Option<&Field>, value: u64) -> Result<(Vec<u8>, String)> {
    let endian = get_field_endian(field_def);
    match &field.value {
        ParsedValue::U32(_) => {
            let val = value as u32;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, value.to_string()))
        }
        ParsedValue::U16(_) => {
            let val = value as u16;
            let bytes = match endian {
                Endian::Little => val.to_le_bytes().to_vec(),
                Endian::Big => val.to_be_bytes().to_vec(),
            };
            Ok((bytes, value.to_string()))
        }
        ParsedValue::U8(_) => Ok((vec![value as u8], value.to_string())),
        ParsedValue::U64(_) => {
            let bytes = match endian {
                Endian::Little => value.to_le_bytes().to_vec(),
                Endian::Big => value.to_be_bytes().to_vec(),
            };
            Ok((bytes, value.to_string()))
        }
        ParsedValue::Enum { .. } => {
            if let Some(fdef) = field_def {
                if let DataType::Enum { underlying, .. } = &fdef.data_type {
                    return encode_int_by_type(underlying.as_ref(), value as i64, endian);
                }
            }
            Ok(((value as u32).to_be_bytes().to_vec(), value.to_string()))
        }
        _ => Ok((value.to_be_bytes().to_vec(), value.to_string())),
    }
}

fn collect_checksum_fields_with_defs(
    parsed: &ParsedField,
    format_def: &FormatDefinition,
    ctx: &HashMap<String, Value>,
) -> Vec<(ParsedField, usize, usize, String, Option<Field>)> {
    let mut results = Vec::new();
    collect_checksum_fields_recursive(parsed, format_def, &format_def.root, "", ctx, &mut results);
    results
}

fn collect_checksum_fields_recursive(
    parsed_field: &ParsedField,
    format_def: &FormatDefinition,
    struct_def: &StructDefinition,
    parent_path: &str,
    ctx: &HashMap<String, Value>,
    results: &mut Vec<(ParsedField, usize, usize, String, Option<Field>)>,
) {
    let current_path = if parent_path.is_empty() {
        struct_def.name.clone()
    } else {
        format!("{}.{}", parent_path, struct_def.name)
    };

    for (field_def, parsed_child) in struct_def.fields.iter().zip(parsed_field.children.iter()) {
        let fq_path = format!("{}.{}", current_path, field_def.name);
        if parsed_child.path == fq_path {
            if let Some(checksum_def) = &field_def.checksum {
                if let Ok(start) = eval_expression_usize_with_ctx(&checksum_def.start, ctx) {
                    if let Ok(end) = eval_expression_usize_with_ctx(&checksum_def.end, ctx) {
                        results.push((
                            parsed_child.clone(),
                            start,
                            end,
                            checksum_def.algorithm.clone(),
                            Some(field_def.clone()),
                        ));
                    }
                }
            }
            match &field_def.data_type {
                DataType::Struct { name } => {
                    if let Some(sd) = format_def.get_struct(name) {
                        collect_checksum_fields_recursive(parsed_child, format_def, sd, &current_path, ctx, results);
                    }
                }
                _ => {}
            }
        }
    }
}

fn recalc_checksums(
    parsed: &ParsedField,
    format_def: &FormatDefinition,
    ctx: &HashMap<String, Value>,
    modified_ranges: &[(usize, usize)],
    modified_data: &mut Vec<u8>,
    dry_run: bool,
) -> Result<Vec<ChecksumRecalc>> {
    let mut recalcs = Vec::new();
    let mut processed = std::collections::HashSet::new();

    let checksum_fields = collect_checksum_fields_with_defs(parsed, format_def, ctx);

    for (field, start, end, algo, field_def_opt) in &checksum_fields {
        let key = (field.offset, field.length);
        if processed.contains(&key) {
            continue;
        }

        let covers_modified = modified_ranges.iter().any(|&(m_start, m_end)| {
            m_start < *end && m_end > *start
        });

        if !covers_modified {
            continue;
        }

        if *start >= *end || *end > modified_data.len() {
            continue;
        }

        let data_range = &modified_data[*start..*end];
        let new_checksum: u64 = match algo.as_str() {
            "crc32" => checksum::crc32(data_range) as u64,
            "adler32" => checksum::adler32(data_range) as u64,
            "sum" => checksum::simple_sum(data_range) as u64,
            _ => continue,
        };

        let original_value = field.value.to_u64().unwrap_or(0);

        let (cs_bytes, _) = encode_checksum_bytes(field, field_def_opt.as_ref(), new_checksum)?;

        if cs_bytes.len() == field.length {
            if !dry_run {
                modified_data[field.offset..field.offset + field.length].copy_from_slice(&cs_bytes);
            }
            recalcs.push(ChecksumRecalc {
                field_path: field.path.clone(),
                offset: field.offset,
                algorithm: algo.clone(),
                start: *start,
                end: *end,
                original_value,
                new_value: new_checksum,
            });
        }

        processed.insert(key);
    }

    Ok(recalcs)
}

fn field_to_bytes(field: &ParsedField, field_def: Option<&Field>) -> Option<Vec<u8>> {
    let endian = get_field_endian(field_def);
    match &field.value {
        ParsedValue::U8(v) => Some(vec![*v]),
        ParsedValue::U16(v) => Some(match endian {
            Endian::Little => v.to_le_bytes().to_vec(),
            Endian::Big => v.to_be_bytes().to_vec(),
        }),
        ParsedValue::U32(v) => Some(match endian {
            Endian::Little => v.to_le_bytes().to_vec(),
            Endian::Big => v.to_be_bytes().to_vec(),
        }),
        ParsedValue::U64(v) => Some(match endian {
            Endian::Little => v.to_le_bytes().to_vec(),
            Endian::Big => v.to_be_bytes().to_vec(),
        }),
        ParsedValue::I8(v) => Some(vec![*v as u8]),
        ParsedValue::I16(v) => Some(match endian {
            Endian::Little => v.to_le_bytes().to_vec(),
            Endian::Big => v.to_be_bytes().to_vec(),
        }),
        ParsedValue::I32(v) => Some(match endian {
            Endian::Little => v.to_le_bytes().to_vec(),
            Endian::Big => v.to_be_bytes().to_vec(),
        }),
        ParsedValue::I64(v) => Some(match endian {
            Endian::Little => v.to_le_bytes().to_vec(),
            Endian::Big => v.to_be_bytes().to_vec(),
        }),
        ParsedValue::F32(v) => Some(match endian {
            Endian::Little => v.to_le_bytes().to_vec(),
            Endian::Big => v.to_be_bytes().to_vec(),
        }),
        ParsedValue::F64(v) => Some(match endian {
            Endian::Little => v.to_le_bytes().to_vec(),
            Endian::Big => v.to_be_bytes().to_vec(),
        }),
        ParsedValue::Bytes(b) => Some(b.clone()),
        ParsedValue::String(s) => {
            let mut bytes = s.as_bytes().to_vec();
            if bytes.len() < field.length {
                bytes.resize(field.length, 0x00);
            }
            Some(bytes)
        }
        ParsedValue::Enum { value, .. } => {
            if let Some(fdef) = field_def {
                if let DataType::Enum { underlying, .. } = &fdef.data_type {
                    if let Ok((bytes, _)) = encode_int_by_type(underlying.as_ref(), *value, endian) {
                        return Some(bytes);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn values_match(new_bytes: &[u8], field: &ParsedField, field_def: Option<&Field>) -> bool {
    if let Some(actual_bytes) = field_to_bytes(field, field_def) {
        actual_bytes.as_slice() == new_bytes
    } else {
        false
    }
}

pub fn run_patch(
    input_data: &[u8],
    output_path: &Path,
    format_def: &FormatDefinition,
    instructions: &[PatchInstruction],
    dry_run: bool,
) -> Result<(PatchResult, i32)> {
    let (parsed, _) = parse(input_data, format_def)?;
    let ctx = build_value_context(&parsed);

    let mut changes: Vec<FieldChange> = Vec::new();
    let mut modified_ranges: Vec<(usize, usize)> = Vec::new();
    let mut modified_data = input_data.to_vec();

    for instr in instructions {
        let field = find_field_by_path(&parsed, &instr.field_path)
            .ok_or_else(|| {
                let all_paths = collect_all_field_paths(&parsed);
                let suggestion = suggest_similar_path(&instr.field_path, &all_paths);
                let paths_str: String = all_paths
                    .iter()
                    .take(20)
                    .map(|p| format!("  - {}\n", p))
                    .collect();
                let more = if all_paths.len() > 20 {
                    format!("  ... 还有{}个字段\n", all_paths.len() - 20)
                } else {
                    String::new()
                };
                anyhow!(
                    "字段路径不存在: {}\n可用字段路径:\n{}{}{}",
                    instr.field_path,
                    paths_str,
                    more,
                    suggestion
                )
            })?;

        if field.truncated || field.skipped {
            return Err(anyhow!("字段 {} 被截断或跳过，无法修改", instr.field_path));
        }

        let field_def = find_field_definition(format_def, &format_def.root, &instr.field_path, "");

        let (new_bytes, new_display) = encode_value(field, field_def, &instr.new_value_str, format_def)
            .map_err(|e| anyhow!("字段 {} 的值编码失败: {}", instr.field_path, e))?;

        if new_bytes.len() != field.length {
            return Err(anyhow!(
                "编码后字节长度不匹配: 字段{}期望{}字节，实际得到{}字节",
                instr.field_path,
                field.length,
                new_bytes.len()
            ));
        }

        let original_bytes = input_data[field.offset..field.offset + field.length].to_vec();
        let original_display = field.value.display(field.display_format);

        modified_data[field.offset..field.offset + field.length].copy_from_slice(&new_bytes);

        modified_ranges.push((field.offset, field.offset + field.length));

        changes.push(FieldChange {
            field_path: instr.field_path.clone(),
            offset: field.offset,
            length: field.length,
            original_bytes,
            new_bytes,
            original_value_display: original_display,
            new_value_display: new_display,
        });
    }

    let checksum_recalcs = recalc_checksums(&parsed, format_def, &ctx, &modified_ranges, &mut modified_data, dry_run)?;

    let mut validation_failures = Vec::new();
    if !dry_run {
        fs::write(output_path, &modified_data)
            .map_err(|e| anyhow!("写入输出文件失败: {}", e))?;

        let verify_data = fs::read(output_path)
            .map_err(|e| anyhow!("读取输出文件验证失败: {}", e))?;
        let (reparsed, _) = parse(&verify_data, format_def)?;

        for change in &changes {
            let field_def = find_field_definition(format_def, &format_def.root, &change.field_path, "");
            if let Some(actual_field) = find_field_by_path(&reparsed, &change.field_path) {
                if !values_match(&change.new_bytes, actual_field, field_def) {
                    validation_failures.push((
                        change.field_path.clone(),
                        change.new_value_display.clone(),
                        actual_field.value.display(actual_field.display_format),
                    ));
                }
            } else {
                validation_failures.push((
                    change.field_path.clone(),
                    change.new_value_display.clone(),
                    "<字段未找到>".to_string(),
                ));
            }
        }
    }

    let result = PatchResult {
        changes,
        checksum_recalcs,
        validation_failures: validation_failures.clone(),
    };

    let exit_code = if !validation_failures.is_empty() {
        PatchError::VALIDATION_FAILURE
    } else {
        PatchError::SUCCESS
    };

    Ok((result, exit_code))
}
