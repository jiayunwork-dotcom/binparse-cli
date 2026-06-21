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
    pub condition: Option<String>,
    pub condition_satisfied: bool,
}

#[derive(Debug, Clone)]
pub struct SkippedInstruction {
    pub field_path: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct OffsetWarning {
    pub dependent_field: String,
    pub modified_field: String,
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub offset: usize,
    pub length: usize,
    pub original_hex: String,
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
    pub skipped: Vec<SkippedInstruction>,
    pub offset_warnings: Vec<OffsetWarning>,
}

pub struct PatchError;

impl PatchError {
    pub const FIELD_NOT_FOUND: i32 = 2;
    pub const FORMAT_ERROR: i32 = 2;
    pub const VALUE_ENCODING_ERROR: i32 = 3;
    pub const VALIDATION_FAILURE: i32 = 1;
    pub const SUCCESS: i32 = 0;
    pub const UNDEFINED_VARIABLE: i32 = 2;
}

pub fn parse_patch_instructions(
    sets: &[String],
    patch_file: Option<&Path>,
    vars: &[String],
) -> Result<Vec<PatchInstruction>> {
    let var_map = parse_vars(vars)?;
    let mut instructions: Vec<PatchInstruction> = Vec::new();
    let mut seen_paths: HashMap<String, usize> = HashMap::new();

    if let Some(patch_file_path) = patch_file {
        let content = fs::read_to_string(patch_file_path)
            .map_err(|e| anyhow!("无法读取patch文件: {}", e))?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = substitute_template_vars(line, &var_map)?;
            if let Some(instr) = parse_instruction_line(&line)? {
                if let Some(idx) = seen_paths.get(&instr.field_path) {
                    instructions[*idx] = instr;
                } else {
                    seen_paths.insert(instr.field_path.clone(), instructions.len());
                    instructions.push(instr);
                }
            }
        }
    }

    for set in sets {
        if let Some(instr) = parse_instruction_line(set)? {
            if let Some(idx) = seen_paths.get(&instr.field_path) {
                instructions[*idx] = instr;
            } else {
                seen_paths.insert(instr.field_path.clone(), instructions.len());
                instructions.push(instr);
            }
        } else {
            return Err(anyhow!("无效的--set参数格式: {}，应为\"字段路径=新值[@条件表达式]\"", set));
        }
    }

    Ok(instructions)
}

fn parse_vars(vars: &[String]) -> Result<HashMap<String, String>> {
    let mut var_map = HashMap::new();
    for var in vars {
        if let Some((name, val)) = var.split_once('=') {
            var_map.insert(name.trim().to_string(), val.trim().to_string());
        } else {
            return Err(anyhow!("无效的--var参数格式: {}，应为\"name=value\"", var));
        }
    }
    Ok(var_map)
}

fn substitute_template_vars(line: &str, vars: &HashMap<String, String>) -> Result<String> {
    let mut result = line.to_string();
    let re = regex::Regex::new(r"\$\{([a-zA-Z_][a-zA-Z0-9_]*)\}").unwrap();
    for cap in re.captures_iter(line) {
        let var_name = &cap[1];
        if let Some(val) = vars.get(var_name) {
            result = result.replace(&format!("${{{}}}", var_name), val);
        } else {
            return Err(anyhow!("未定义的模板变量: ${}", var_name));
        }
    }
    Ok(result)
}

fn parse_instruction_line(line: &str) -> Result<Option<PatchInstruction>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }

    let (value_part, condition) = if let Some((val, cond)) = line.split_once('@') {
        (val.trim(), Some(cond.trim().to_string()))
    } else {
        (line, None)
    };

    if let Some((path, val)) = value_part.split_once('=') {
        Ok(Some(PatchInstruction {
            field_path: path.trim().to_string(),
            new_value_str: val.trim().to_string(),
            condition,
            condition_satisfied: true,
        }))
    } else {
        Err(anyhow!("无效的指令格式: {}，应为\"字段路径=新值[@条件表达式]\"", line))
    }
}

fn evaluate_condition(
    condition: &str,
    root: &ParsedField,
    ctx: &HashMap<String, Value>,
) -> Result<bool> {
    let re = regex::Regex::new(r"^([a-zA-Z_][a-zA-Z0-9_\.\[\]]+)\s*(==|!=|>=|<=|>|<)\s*(-?\d+|0x[0-9a-fA-F]+)$").unwrap();
    let caps = re.captures(condition)
        .ok_or_else(|| anyhow!("无效的条件表达式格式: {}。应为\"字段路径 操作符 整数字面量\"，支持==、!=、>、<、>=、<=", condition))?;

    let field_path = &caps[1];
    let op = &caps[2];
    let literal_str = &caps[3];

    let field_value = if let Some(val) = ctx.get(field_path) {
        if let Ok(int_val) = val.as_int() {
            int_val
        } else if let Ok(float_val) = val.as_float() {
            float_val as i64
        } else {
            return Err(anyhow!("字段 {} 的值无法转换为整数", field_path));
        }
    } else if let Some(field) = find_field_by_path(root, field_path) {
        field.value.to_i64()
            .ok_or_else(|| anyhow!("字段 {} 的值无法转换为整数", field_path))?
    } else {
        return Err(anyhow!("条件表达式中引用的字段不存在: {}", field_path));
    };

    let literal_val = if let Some(hex) = literal_str.strip_prefix("0x").or_else(|| literal_str.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).map_err(|e| anyhow!("十六进制解析失败: {}", e))?
    } else {
        literal_str.parse::<i64>().map_err(|e| anyhow!("整数解析失败: {}", e))?
    };

    let result = match op {
        "==" => field_value == literal_val,
        "!=" => field_value != literal_val,
        ">" => field_value > literal_val,
        "<" => field_value < literal_val,
        ">=" => field_value >= literal_val,
        "<=" => field_value <= literal_val,
        _ => return Err(anyhow!("不支持的操作符: {}", op)),
    };

    Ok(result)
}

fn collect_offset_dependencies(format_def: &FormatDefinition) -> HashMap<String, Vec<String>> {
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    let all_structs = collect_all_structs(format_def);

    for struct_def in &all_structs {
        collect_field_deps(struct_def, &format_def.root.name, "", format_def, &all_structs, &mut deps);
    }

    deps
}

fn collect_all_structs(format_def: &FormatDefinition) -> Vec<StructDefinition> {
    let mut structs = format_def.structs.clone();
    structs.push(format_def.root.clone());
    structs
}

fn collect_field_deps(
    struct_def: &StructDefinition,
    struct_name: &str,
    parent_path: &str,
    format_def: &FormatDefinition,
    all_structs: &[StructDefinition],
    deps: &mut HashMap<String, Vec<String>>,
) {
    let current_path = if parent_path.is_empty() {
        struct_name.to_string()
    } else {
        format!("{}.{}", parent_path, struct_name)
    };

    for field in &struct_def.fields {
        let field_path = format!("{}.{}", current_path, field.name);
        if let Some(offset_expr) = &field.offset {
            if offset_expr != "relative" {
                let referenced_fields = extract_field_references(offset_expr, format_def, all_structs, &current_path);
                for ref_field in referenced_fields {
                    deps.entry(ref_field)
                        .or_insert_with(Vec::new)
                        .push(field_path.clone());
                }
            }
        }

        match &field.data_type {
            DataType::Struct { name } => {
                if let Some(sd) = all_structs.iter().find(|s| s.name == *name) {
                    collect_field_deps(sd, name, &current_path, format_def, all_structs, deps);
                }
            }
            DataType::Array { element_type, .. } => {
                if let DataType::Struct { name } = element_type.as_ref() {
                    if let Some(sd) = all_structs.iter().find(|s| s.name == *name) {
                        collect_field_deps(sd, name, &current_path, format_def, all_structs, deps);
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_field_references(
    expr: &str,
    format_def: &FormatDefinition,
    all_structs: &[StructDefinition],
    current_path: &str,
) -> Vec<String> {
    let mut references = Vec::new();
    let re = regex::Regex::new(r"([a-zA-Z_][a-zA-Z0-9_]+)").unwrap();
    for cap in re.captures_iter(expr) {
        let ident = &cap[1];
        if ident == "relative" {
            continue;
        }
        if let Ok(_) = ident.parse::<i64>() {
            continue;
        }

        let possible_paths = resolve_identifier(ident, current_path, format_def, all_structs);
        for path in possible_paths {
            if !references.contains(&path) {
                references.push(path);
            }
        }
    }
    references
}

fn resolve_identifier(
    ident: &str,
    current_path: &str,
    format_def: &FormatDefinition,
    all_structs: &[StructDefinition],
) -> Vec<String> {
    let mut paths = Vec::new();

    let mut path_parts: Vec<&str> = current_path.split('.').collect();
    while !path_parts.is_empty() {
        let candidate = format!("{}.{}", path_parts.join("."), ident);
        if field_exists(&candidate, format_def, all_structs) {
            paths.push(candidate);
        }
        path_parts.pop();
    }

    let root_candidate = format!("{}.{}", format_def.root.name, ident);
    if field_exists(&root_candidate, format_def, all_structs) {
        paths.push(root_candidate);
    }

    for struct_def in all_structs {
        if struct_def.fields.iter().any(|f| f.name == ident) {
            let candidate = format!("{}.{}", struct_def.name, ident);
            if !paths.contains(&candidate) {
                paths.push(candidate);
            }
        }
    }

    paths
}

fn field_exists(
    path: &str,
    format_def: &FormatDefinition,
    all_structs: &[StructDefinition],
) -> bool {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return false;
    }

    let struct_name = parts[0];
    let struct_def = if let Some(sd) = all_structs.iter().find(|s| s.name == struct_name) {
        sd
    } else {
        return false;
    };

    check_field_in_struct(struct_def, &parts[1..], format_def, all_structs)
}

fn check_field_in_struct(
    struct_def: &StructDefinition,
    remaining_parts: &[&str],
    format_def: &FormatDefinition,
    all_structs: &[StructDefinition],
) -> bool {
    if remaining_parts.is_empty() {
        return false;
    }

    let field_name = remaining_parts[0];
    let field = match struct_def.fields.iter().find(|f| f.name == field_name) {
        Some(f) => f,
        None => return false,
    };

    if remaining_parts.len() == 1 {
        return true;
    }

    match &field.data_type {
        DataType::Struct { name } => {
            if let Some(sd) = all_structs.iter().find(|s| s.name == *name) {
                check_field_in_struct(sd, &remaining_parts[1..], format_def, all_structs)
            } else {
                false
            }
        }
        DataType::Array { element_type, .. } => {
            if let DataType::Struct { name } = element_type.as_ref() {
                if let Some(sd) = all_structs.iter().find(|s| s.name == *name) {
                    let mut rest = remaining_parts[1..].to_vec();
                    if !rest.is_empty() {
                        let re = regex::Regex::new(r"^\[\d+\]$").unwrap();
                        if re.is_match(rest[0]) {
                            rest = rest[1..].to_vec();
                        }
                    }
                    check_field_in_struct(sd, &rest, format_def, all_structs)
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

fn check_offset_dependencies(
    modified_fields: &[String],
    format_def: &FormatDefinition,
) -> Vec<OffsetWarning> {
    let deps = collect_offset_dependencies(format_def);
    let mut warnings = Vec::new();

    for modified in modified_fields {
        if let Some(dependents) = deps.get(modified) {
            for dep in dependents {
                warnings.push(OffsetWarning {
                    dependent_field: dep.clone(),
                    modified_field: modified.clone(),
                });
            }
        }
    }

    warnings
}

pub fn read_patch_history(output_path: &Path) -> Result<Vec<Vec<HistoryEntry>>> {
    let history_path = get_history_path(output_path);
    if !history_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&history_path)
        .map_err(|e| anyhow!("无法读取历史文件: {}", e))?;

    let mut batches = Vec::new();
    let mut current_batch = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current_batch.is_empty() {
                batches.push(current_batch);
                current_batch = Vec::new();
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() == 3 {
            let offset = parts[0].parse::<usize>()
                .map_err(|e| anyhow!("历史文件格式错误 - 偏移解析失败: {}", e))?;
            let length = parts[1].parse::<usize>()
                .map_err(|e| anyhow!("历史文件格式错误 - 长度解析失败: {}", e))?;
            let original_hex = parts[2].to_string();
            current_batch.push(HistoryEntry { offset, length, original_hex });
        } else {
            return Err(anyhow!("历史文件格式错误: {}", line));
        }
    }

    if !current_batch.is_empty() {
        batches.push(current_batch);
    }

    Ok(batches)
}

pub fn write_patch_history(output_path: &Path, changes: &[FieldChange]) -> Result<()> {
    let history_path = get_history_path(output_path);

    let mut content = String::new();

    if history_path.exists() {
        let existing = fs::read_to_string(&history_path)
            .map_err(|e| anyhow!("无法读取历史文件: {}", e))?;
        if !existing.is_empty() && !existing.ends_with("\n\n") {
            content.push_str(&existing);
            if !existing.ends_with('\n') {
                content.push('\n');
            }
            content.push('\n');
        } else {
            content.push_str(&existing);
        }
    }

    for change in changes {
        let hex_str = change.original_bytes.iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join("");
        content.push_str(&format!("{}:{}:{}\n", change.offset, change.length, hex_str));
    }

    fs::write(&history_path, content)
        .map_err(|e| anyhow!("无法写入历史文件: {}", e))?;

    Ok(())
}

pub fn remove_last_history_batch(output_path: &Path) -> Result<()> {
    let history_path = get_history_path(output_path);
    if !history_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&history_path)
        .map_err(|e| anyhow!("无法读取历史文件: {}", e))?;

    let lines: Vec<&str> = content.lines().collect();
    let mut last_non_empty = None;
    let mut batch_start = None;

    for i in (0..lines.len()).rev() {
        let line = lines[i].trim();
        if !line.is_empty() && !line.starts_with('#') {
            if last_non_empty.is_none() {
                last_non_empty = Some(i);
            }
            batch_start = Some(i);
        } else if last_non_empty.is_some() {
            break;
        }
    }

    if batch_start.is_none() {
        fs::remove_file(&history_path)
            .map_err(|e| anyhow!("无法删除空的历史文件: {}", e))?;
        return Ok(());
    }

    let batch_start = batch_start.unwrap();
    let mut new_lines = lines[..batch_start].to_vec();

    while !new_lines.is_empty() && new_lines.last().map_or(false, |l| l.trim().is_empty()) {
        new_lines.pop();
    }

    let new_content = new_lines.join("\n");
    if new_content.trim().is_empty() {
        fs::remove_file(&history_path)
            .map_err(|e| anyhow!("无法删除空的历史文件: {}", e))?;
    } else {
        fs::write(&history_path, new_content + "\n")
            .map_err(|e| anyhow!("无法写入历史文件: {}", e))?;
    }

    Ok(())
}

fn get_history_path(output_path: &Path) -> std::path::PathBuf {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(".binpatch_history")
}

pub fn undo_last_patch(
    output_path: &Path,
    _format_def: &FormatDefinition,
) -> Result<(Vec<FieldChange>, i32)> {
    let batches = read_patch_history(output_path)?;
    if batches.is_empty() {
        return Err(anyhow!("没有找到可撤销的patch操作记录"));
    }

    let last_batch = batches.last().unwrap();
    let mut data = fs::read(output_path)
        .map_err(|e| anyhow!("无法读取输出文件: {}", e))?;

    let mut changes = Vec::new();

    for entry in last_batch {
        let original_bytes = Vec::<u8>::from_hex(&entry.original_hex)
            .map_err(|e| anyhow!("历史文件中原始字节解析失败: {}", e))?;

        if entry.offset + entry.length > data.len() {
            return Err(anyhow!("历史记录偏移超出文件范围: offset={}, length={}, file_size={}",
                entry.offset, entry.length, data.len()));
        }

        let new_bytes = data[entry.offset..entry.offset + entry.length].to_vec();
        let new_bytes_clone = new_bytes.clone();
        data[entry.offset..entry.offset + entry.length].copy_from_slice(&original_bytes);

        changes.push(FieldChange {
            field_path: format!("<从历史恢复 @ 0x{:08X}>", entry.offset),
            offset: entry.offset,
            length: entry.length,
            original_bytes: new_bytes,
            new_bytes: original_bytes,
            original_value_display: format!("[{}]", new_bytes_clone.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")),
            new_value_display: format!("[{}]", entry.original_hex.chars()
                .collect::<Vec<char>>()
                .chunks(2)
                .map(|c| c.iter().collect::<String>())
                .collect::<Vec<_>>()
                .join(" ")),
        });
    }

    fs::write(output_path, &data)
        .map_err(|e| anyhow!("写入撤销后的数据失败: {}", e))?;

    remove_last_history_batch(output_path)?;

    Ok((changes, PatchError::SUCCESS))
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
    let mut skipped: Vec<SkippedInstruction> = Vec::new();
    let mut modified_ranges: Vec<(usize, usize)> = Vec::new();
    let mut modified_data = input_data.to_vec();
    let mut modified_field_paths: Vec<String> = Vec::new();

    for instr in instructions {
        if let Some(condition) = &instr.condition {
            match evaluate_condition(condition, &parsed, &ctx) {
                Ok(true) => {}
                Ok(false) => {
                    skipped.push(SkippedInstruction {
                        field_path: instr.field_path.clone(),
                        reason: format!("条件不满足，已跳过 ({})", condition),
                    });
                    continue;
                }
                Err(e) => {
                    return Err(anyhow!("条件表达式求值失败: {}", e));
                }
            }
        }

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
        modified_field_paths.push(instr.field_path.clone());

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

    let offset_warnings = check_offset_dependencies(&modified_field_paths, format_def);

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
        skipped,
        offset_warnings,
    };

    let exit_code = if !validation_failures.is_empty() {
        PatchError::VALIDATION_FAILURE
    } else {
        PatchError::SUCCESS
    };

    Ok((result, exit_code))
}
