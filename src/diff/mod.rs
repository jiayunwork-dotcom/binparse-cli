use crate::parser::ParsedField;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffField {
    pub path: String,
    pub name: String,
    pub offset: usize,
    pub length: usize,
    pub value1: String,
    pub value2: String,
    pub is_different: bool,
    pub truncated: bool,
    pub undecidable: bool,
    pub skipped: bool,
    pub children: Vec<DiffField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    pub fields: Vec<DiffField>,
    pub total_fields: usize,
    pub different_fields: usize,
    pub diff_rate: f64,
}

pub fn compare_fields(field1: &ParsedField, field2: &ParsedField) -> DiffField {
    let value1 = if field1.truncated {
        "<truncated>".to_string()
    } else if field1.undecidable {
        "<undecidable>".to_string()
    } else if field1.skipped {
        "<skipped>".to_string()
    } else {
        field1.value.display(field1.display_format)
    };

    let value2 = if field2.truncated {
        "<truncated>".to_string()
    } else if field2.undecidable {
        "<undecidable>".to_string()
    } else if field2.skipped {
        "<skipped>".to_string()
    } else {
        field2.value.display(field2.display_format)
    };

    let is_different = value1 != value2 
        || field1.truncated != field2.truncated
        || field1.undecidable != field2.undecidable
        || field1.skipped != field2.skipped;

    let children = compare_children(&field1.children, &field2.children);

    DiffField {
        path: field1.path.clone(),
        name: field1.name.clone(),
        offset: field1.offset,
        length: field1.length,
        value1,
        value2,
        is_different,
        truncated: field1.truncated || field2.truncated,
        undecidable: field1.undecidable || field2.undecidable,
        skipped: field1.skipped || field2.skipped,
        children,
    }
}

fn compare_children(children1: &[ParsedField], children2: &[ParsedField]) -> Vec<DiffField> {
    let map1: HashMap<&str, &ParsedField> = children1.iter().map(|f| (f.name.as_str(), f)).collect();
    let map2: HashMap<&str, &ParsedField> = children2.iter().map(|f| (f.name.as_str(), f)).collect();

    let mut result = Vec::new();
    let all_names: std::collections::HashSet<&str> = map1.keys().chain(map2.keys()).cloned().collect();

    for name in all_names {
        match (map1.get(name), map2.get(name)) {
            (Some(f1), Some(f2)) => {
                result.push(compare_fields(f1, f2));
            }
            (Some(f1), None) => {
                result.push(DiffField {
                    path: f1.path.clone(),
                    name: f1.name.clone(),
                    offset: f1.offset,
                    length: f1.length,
                    value1: f1.value.display(f1.display_format),
                    value2: "<missing>".to_string(),
                    is_different: true,
                    truncated: f1.truncated,
                    undecidable: f1.undecidable,
                    skipped: f1.skipped,
                    children: f1.children.iter().map(|c| DiffField {
                        path: c.path.clone(),
                        name: c.name.clone(),
                        offset: c.offset,
                        length: c.length,
                        value1: c.value.display(c.display_format),
                        value2: "<missing>".to_string(),
                        is_different: true,
                        truncated: c.truncated,
                        undecidable: c.undecidable,
                        skipped: c.skipped,
                        children: Vec::new(),
                    }).collect(),
                });
            }
            (None, Some(f2)) => {
                result.push(DiffField {
                    path: f2.path.clone(),
                    name: f2.name.clone(),
                    offset: f2.offset,
                    length: f2.length,
                    value1: "<missing>".to_string(),
                    value2: f2.value.display(f2.display_format),
                    is_different: true,
                    truncated: f2.truncated,
                    undecidable: f2.undecidable,
                    skipped: f2.skipped,
                    children: f2.children.iter().map(|c| DiffField {
                        path: c.path.clone(),
                        name: c.name.clone(),
                        offset: c.offset,
                        length: c.length,
                        value1: "<missing>".to_string(),
                        value2: c.value.display(c.display_format),
                        is_different: true,
                        truncated: c.truncated,
                        undecidable: c.undecidable,
                        skipped: c.skipped,
                        children: Vec::new(),
                    }).collect(),
                });
            }
            (None, None) => unreachable!(),
        }
    }

    result.sort_by(|a, b| a.offset.cmp(&b.offset));
    result
}

pub fn diff(root1: &ParsedField, root2: &ParsedField) -> DiffResult {
    let root_diff = compare_fields(root1, root2);
    let root_diff_clone = root_diff.clone();
    
    let total_fields = count_fields(&[root_diff_clone.clone()]);
    let different_fields = count_different_fields(&[root_diff_clone]);
    let diff_rate = if total_fields > 0 {
        different_fields as f64 / total_fields as f64
    } else {
        0.0
    };

    DiffResult {
        fields: root_diff.children,
        total_fields,
        different_fields,
        diff_rate,
    }
}

fn count_fields(fields: &[DiffField]) -> usize {
    let mut count = 0;
    for field in fields {
        let has_struct_children = !field.children.is_empty();
        if !has_struct_children {
            count += 1;
        }
        count += count_fields(&field.children);
    }
    count
}

fn count_different_fields(fields: &[DiffField]) -> usize {
    let mut count = 0;
    for field in fields {
        let has_struct_children = !field.children.is_empty();
        if field.is_different && !has_struct_children {
            count += 1;
        }
        count += count_different_fields(&field.children);
    }
    count
}

pub fn export_markdown(diff: &DiffResult, file1: &str, file2: &str) -> String {
    let mut md = String::new();
    md.push_str(&format!("# Binary Diff Report\n\n"));
    md.push_str(&format!("**File 1:** {}\n\n", file1));
    md.push_str(&format!("**File 2:** {}\n\n", file2));
    md.push_str(&format!("## Summary\n\n"));
    md.push_str(&format!("| Metric | Value |\n"));
    md.push_str(&format!("|--------|-------|\n"));
    md.push_str(&format!("| Total Fields | {} |\n", diff.total_fields));
    md.push_str(&format!("| Different Fields | {} |\n", diff.different_fields));
    md.push_str(&format!("| Diff Rate | {:.2}% |\n\n", diff.diff_rate * 100.0));
    
    md.push_str("## Detailed Differences\n\n");
    md.push_str("| Field Path | Offset | Length | File 1 Value | File 2 Value | Status |\n");
    md.push_str("|------------|--------|--------|--------------|--------------|--------|\n");
    
    export_markdown_rows(&diff.fields, &mut md);
    
    md
}

fn export_markdown_rows(fields: &[DiffField], md: &mut String) {
    for field in fields {
        if field.is_different {
            let status = if field.truncated {
                "Truncated"
            } else if field.undecidable {
                "Undecidable"
            } else if field.skipped {
                "Skipped"
            } else {
                "Different"
            };
            
            md.push_str(&format!(
                "| {} | 0x{:08X} | {} | `{}` | `{}` | {} |\n",
                field.path,
                field.offset,
                field.length,
                field.value1.replace("|", "\\|"),
                field.value2.replace("|", "\\|"),
                status
            ));
        }
        export_markdown_rows(&field.children, md);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::*;
    use crate::parser::*;

    fn create_test_parsed_field(name: &str, value: u32) -> ParsedField {
        ParsedField {
            name: name.to_string(),
            path: format!("root.{}", name),
            offset: 0,
            length: 4,
            value: ParsedValue::U32(value),
            display_format: DisplayFormat::Hex,
            truncated: false,
            undecidable: false,
            skipped: false,
            checksum_result: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn test_compare_same_fields() {
        let f1 = create_test_parsed_field("test", 0x1234);
        let f2 = create_test_parsed_field("test", 0x1234);
        let diff = compare_fields(&f1, &f2);
        assert!(!diff.is_different);
        assert_eq!(diff.value1, diff.value2);
    }

    #[test]
    fn test_compare_different_fields() {
        let f1 = create_test_parsed_field("test", 0x1234);
        let f2 = create_test_parsed_field("test", 0x5678);
        let diff = compare_fields(&f1, &f2);
        assert!(diff.is_different);
        assert_ne!(diff.value1, diff.value2);
    }

    #[test]
    fn test_diff_stats() {
        let root1 = ParsedField {
            name: "root".to_string(),
            path: "root".to_string(),
            offset: 0,
            length: 8,
            value: ParsedValue::Struct(vec![
                create_test_parsed_field("a", 0x0001),
                create_test_parsed_field("b", 0x0002),
            ]),
            display_format: DisplayFormat::Hex,
            truncated: false,
            undecidable: false,
            skipped: false,
            checksum_result: None,
            children: vec![
                create_test_parsed_field("a", 0x0001),
                create_test_parsed_field("b", 0x0002),
            ],
        };

        let root2 = ParsedField {
            name: "root".to_string(),
            path: "root".to_string(),
            offset: 0,
            length: 8,
            value: ParsedValue::Struct(vec![
                create_test_parsed_field("a", 0x0001),
                create_test_parsed_field("b", 0xFFFF),
            ]),
            display_format: DisplayFormat::Hex,
            truncated: false,
            undecidable: false,
            skipped: false,
            checksum_result: None,
            children: vec![
                create_test_parsed_field("a", 0x0001),
                create_test_parsed_field("b", 0xFFFF),
            ],
        };

        let result = diff(&root1, &root2);
        assert_eq!(result.total_fields, 2);
        assert_eq!(result.different_fields, 1);
        assert_eq!(result.diff_rate, 0.5);
    }
}
