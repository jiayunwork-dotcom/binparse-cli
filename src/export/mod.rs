use crate::parser::ParsedField;
use anyhow::Result;
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Serialize)]
struct FlatField {
    path: String,
    offset: usize,
    length: usize,
    value: String,
    truncated: bool,
    undecidable: bool,
    skipped: bool,
    checksum: Option<String>,
}

pub fn to_json(root: &ParsedField) -> String {
    serde_json::to_string_pretty(&field_to_json(root)).unwrap()
}

fn field_to_json(field: &ParsedField) -> serde_json::Value {
    let mut obj = json!({
        "name": field.name,
        "path": field.path,
        "offset": field.offset,
        "length": field.length,
        "display": field.value.display(field.display_format),
        "truncated": field.truncated,
        "undecidable": field.undecidable,
        "skipped": field.skipped,
    });

    if let Some(checksum) = &field.checksum_result {
        match checksum {
            crate::parser::ChecksumResult::Passed => {
                obj["checksum"] = json!({ "status": "passed" });
            }
            crate::parser::ChecksumResult::Failed { expected, actual } => {
                obj["checksum"] = json!({
                    "status": "failed",
                    "expected": format!("0x{:08X}", expected),
                    "actual": format!("0x{:08X}", actual),
                });
            }
        }
    }

    if !field.children.is_empty() {
        let children: Vec<serde_json::Value> = field.children.iter().map(field_to_json).collect();
        obj["children"] = json!(children);
    }

    obj
}

pub fn to_csv(root: &ParsedField) -> Result<String> {
    let mut wtr = csv::Writer::from_writer(Vec::new());
    
    wtr.write_record(&["path", "offset", "length", "value", "truncated", "undecidable", "skipped", "checksum"])?;
    
    let mut flat_fields = Vec::new();
    flatten_fields(root, &mut flat_fields);
    
    for field in flat_fields {
        let checksum_str = field.checksum.unwrap_or_default();
        wtr.write_record(&[
            field.path,
            format!("0x{:08X}", field.offset),
            field.length.to_string(),
            field.value,
            field.truncated.to_string(),
            field.undecidable.to_string(),
            field.skipped.to_string(),
            checksum_str,
        ])?;
    }
    
    wtr.flush()?;
    let data = wtr.into_inner()?;
    Ok(String::from_utf8_lossy(&data).to_string())
}

fn flatten_fields(field: &ParsedField, result: &mut Vec<FlatField>) {
    let value = if field.truncated {
        "<truncated>".to_string()
    } else if field.undecidable {
        "<undecidable>".to_string()
    } else if field.skipped {
        "<skipped>".to_string()
    } else {
        field.value.display(field.display_format)
    };

    let checksum = field.checksum_result.as_ref().map(|c| match c {
        crate::parser::ChecksumResult::Passed => "PASSED".to_string(),
        crate::parser::ChecksumResult::Failed { expected, actual } => 
            format!("FAILED(expected=0x{:08X}, actual=0x{:08X})", expected, actual),
    });

    if !matches!(field.value, crate::parser::ParsedValue::Struct(_)) || field.children.is_empty() {
        result.push(FlatField {
            path: field.path.clone(),
            offset: field.offset,
            length: field.length,
            value,
            truncated: field.truncated,
            undecidable: field.undecidable,
            skipped: field.skipped,
            checksum,
        });
    }

    for child in &field.children {
        flatten_fields(child, result);
    }
}

pub fn to_markdown(root: &ParsedField, format_name: &str) -> String {
    let mut md = String::new();
    md.push_str(&format!("# {} Binary Structure Report\n\n", format_name));
    md.push_str(&format!("**Total Size:** {} bytes (0x{:08X})\n\n", root.length, root.length));
    
    md.push_str("## Structure\n\n");
    md.push_str("| Field | Offset | Length | Value | Status |\n");
    md.push_str("|-------|--------|--------|-------|--------|\n");
    
    markdown_rows(root, &mut md, 0);
    
    md
}

fn markdown_rows(field: &ParsedField, md: &mut String, indent: usize) {
    let prefix = "  ".repeat(indent);
    let name = if indent > 0 {
        format!("{}{}", prefix, field.name)
    } else {
        field.name.clone()
    };

    let value = if field.truncated {
        "*<truncated>*".to_string()
    } else if field.undecidable {
        "*<undecidable>*".to_string()
    } else if field.skipped {
        "*<skipped>*".to_string()
    } else {
        field.value.display(field.display_format)
    };

    let status = if let Some(checksum) = &field.checksum_result {
        match checksum {
            crate::parser::ChecksumResult::Passed => "✓ Checksum OK".to_string(),
            crate::parser::ChecksumResult::Failed { expected, actual } => 
                format!("✗ Checksum failed (expected=0x{:08X}, actual=0x{:08X})", expected, actual),
        }
    } else if field.truncated {
        "Truncated".to_string()
    } else if field.undecidable {
        "Undecidable".to_string()
    } else if field.skipped {
        "Skipped".to_string()
    } else {
        "OK".to_string()
    };

    if !matches!(field.value, crate::parser::ParsedValue::Struct(_)) || field.children.is_empty() {
        md.push_str(&format!(
            "| {} | 0x{:08X} | {} | `{}` | {} |\n",
            name,
            field.offset,
            field.length,
            value.replace("|", "\\|"),
            status
        ));
    }

    for child in &field.children {
        markdown_rows(child, md, indent + 1);
    }
}

pub fn to_terminal_summary(root: &ParsedField, format_name: &str, use_color: bool) -> String {
    use colored::*;
    
    let mut output = String::new();
    
    if use_color {
        output.push_str(&format!("{} {}\n", "Format:".bold().cyan(), format_name.bold()));
        output.push_str(&format!("{} {} bytes (0x{:08X})\n\n", "Size:".bold().cyan(), root.length, root.length));
        output.push_str(&format!("{}\n", "Structure:".bold().cyan()));
    } else {
        output.push_str(&format!("Format: {}\n", format_name));
        output.push_str(&format!("Size: {} bytes (0x{:08X})\n\n", root.length, root.length));
        output.push_str("Structure:\n");
    }
    
    terminal_rows(root, &mut output, 0, use_color);
    
    output
}

fn terminal_rows(field: &ParsedField, output: &mut String, indent: usize, use_color: bool) {
    use colored::*;
    
    let prefix = "  ".repeat(indent);
    
    let name = if use_color {
        field.name.bold().to_string()
    } else {
        field.name.clone()
    };

    let value = if field.truncated {
        if use_color {
            "<truncated>".red().to_string()
        } else {
            "<truncated>".to_string()
        }
    } else if field.undecidable {
        if use_color {
            "<undecidable>".yellow().to_string()
        } else {
            "<undecidable>".to_string()
        }
    } else if field.skipped {
        if use_color {
            "<skipped>".blue().to_string()
        } else {
            "<skipped>".to_string()
        }
    } else {
        field.value.display(field.display_format)
    };

    let status = if let Some(checksum) = &field.checksum_result {
        match checksum {
            crate::parser::ChecksumResult::Passed => {
                if use_color {
                    " [✓]".green().to_string()
                } else {
                    " [✓]".to_string()
                }
            }
            crate::parser::ChecksumResult::Failed { expected, actual } => {
                if use_color {
                    format!(" [✗ expected=0x{:08X}, actual=0x{:08X}]", expected, actual).red().to_string()
                } else {
                    format!(" [✗ expected=0x{:08X}, actual=0x{:08X}]", expected, actual)
                }
            }
        }
    } else {
        String::new()
    };

    if !matches!(field.value, crate::parser::ParsedValue::Struct(_)) || field.children.is_empty() {
        output.push_str(&format!(
            "{}  {}@0x{:08X}[{}] = {}{}\n",
            prefix,
            name,
            field.offset,
            field.length,
            value,
            status
        ));
    } else if use_color {
        output.push_str(&format!(
            "{}  {} ({} fields):\n",
            prefix,
            name,
            field.children.len()
        ));
    } else {
        output.push_str(&format!(
            "{}  {} ({} fields):\n",
            prefix,
            name,
            field.children.len()
        ));
    }

    for child in &field.children {
        terminal_rows(child, output, indent + 1, use_color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::*;
    use crate::parser::*;

    fn create_test_parsed_field() -> ParsedField {
        ParsedField {
            name: "root".to_string(),
            path: "root".to_string(),
            offset: 0,
            length: 8,
            value: ParsedValue::Struct(vec![
                ParsedField {
                    name: "magic".to_string(),
                    path: "root.magic".to_string(),
                    offset: 0,
                    length: 4,
                    value: ParsedValue::U32(0x12345678),
                    display_format: DisplayFormat::Hex,
                    truncated: false,
                    undecidable: false,
                    skipped: false,
                    checksum_result: None,
                    children: Vec::new(),
                },
                ParsedField {
                    name: "length".to_string(),
                    path: "root.length".to_string(),
                    offset: 4,
                    length: 4,
                    value: ParsedValue::U32(100),
                    display_format: DisplayFormat::Dec,
                    truncated: false,
                    undecidable: false,
                    skipped: false,
                    checksum_result: Some(ChecksumResult::Passed),
                    children: Vec::new(),
                },
            ]),
            display_format: DisplayFormat::Hex,
            truncated: false,
            undecidable: false,
            skipped: false,
            checksum_result: None,
            children: vec![
                ParsedField {
                    name: "magic".to_string(),
                    path: "root.magic".to_string(),
                    offset: 0,
                    length: 4,
                    value: ParsedValue::U32(0x12345678),
                    display_format: DisplayFormat::Hex,
                    truncated: false,
                    undecidable: false,
                    skipped: false,
                    checksum_result: None,
                    children: Vec::new(),
                },
                ParsedField {
                    name: "length".to_string(),
                    path: "root.length".to_string(),
                    offset: 4,
                    length: 4,
                    value: ParsedValue::U32(100),
                    display_format: DisplayFormat::Dec,
                    truncated: false,
                    undecidable: false,
                    skipped: false,
                    checksum_result: Some(ChecksumResult::Passed),
                    children: Vec::new(),
                },
            ],
        }
    }

    #[test]
    fn test_to_json() {
        let root = create_test_parsed_field();
        let json = to_json(&root);
        assert!(json.contains("\"name\": \"root\""));
        assert!(json.contains("\"magic\""));
        assert!(json.contains("0x12345678"));
    }

    #[test]
    fn test_to_csv() {
        let root = create_test_parsed_field();
        let csv = to_csv(&root).unwrap();
        assert!(csv.contains("root.magic"));
        assert!(csv.contains("root.length"));
        assert!(csv.contains("0x12345678"));
    }

    #[test]
    fn test_to_markdown() {
        let root = create_test_parsed_field();
        let md = to_markdown(&root, "Test");
        assert!(md.contains("# Test Binary Structure Report"));
        assert!(md.contains("| Field | Offset | Length | Value | Status |"));
        assert!(md.contains("magic"));
    }

    #[test]
    fn test_to_terminal_summary() {
        let root = create_test_parsed_field();
        let summary = to_terminal_summary(&root, "Test", false);
        assert!(summary.contains("Format: Test"));
        assert!(summary.contains("magic"));
        assert!(summary.contains("0x12345678"));
    }
}
