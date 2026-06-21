use crate::parser::{ParsedField, ParsedValue, ChecksumResult};
use std::collections::BTreeMap;

pub struct ParseStats {
    pub total_leaf_fields: usize,
    pub type_counts: BTreeMap<String, usize>,
    pub checksum_passed: usize,
    pub checksum_failed: usize,
    pub checksum_none: usize,
    pub parsed_bytes: usize,
    pub file_size: usize,
}

impl ParseStats {
    pub fn from_parsed_field(root: &ParsedField, file_size: usize) -> Self {
        let mut stats = ParseStats {
            total_leaf_fields: 0,
            type_counts: BTreeMap::new(),
            checksum_passed: 0,
            checksum_failed: 0,
            checksum_none: 0,
            parsed_bytes: 0,
            file_size,
        };
        stats.collect_recursive(root);
        stats
    }

    fn collect_recursive(&mut self, field: &ParsedField) {
        if field.children.is_empty() {
            self.total_leaf_fields += 1;

            let type_name = value_type_name(&field.value);
            *self.type_counts.entry(type_name).or_insert(0) += 1;

            if !field.truncated && !field.skipped {
                self.parsed_bytes += field.length;
            }

            match &field.checksum_result {
                Some(ChecksumResult::Passed) => self.checksum_passed += 1,
                Some(ChecksumResult::Failed { .. }) => self.checksum_failed += 1,
                None => self.checksum_none += 1,
            }
        } else {
            for child in &field.children {
                self.collect_recursive(child);
            }
        }
    }

    pub fn coverage_percent(&self) -> f64 {
        if self.file_size == 0 {
            0.0
        } else {
            (self.parsed_bytes as f64 / self.file_size as f64) * 100.0
        }
    }

    pub fn format_to_stderr(&self) -> String {
        let mut lines = Vec::new();
        lines.push("=== 解析统计摘要 ===".to_string());
        lines.push(format!("总字段数(叶子): {}", self.total_leaf_fields));

        let type_counts_str: Vec<String> = self
            .type_counts
            .iter()
            .map(|(k, v)| format!("{}:{}", k, v))
            .collect();
        lines.push(format!("各类型字段计数: {}", type_counts_str.join(", ")));

        lines.push(format!(
            "校验: 通过={}, 失败={}, 未校验={}",
            self.checksum_passed, self.checksum_failed, self.checksum_none
        ));

        lines.push(format!(
            "文件覆盖率: {:.2}% ({}/{} 字节)",
            self.coverage_percent(),
            self.parsed_bytes,
            self.file_size
        ));

        lines.join("\n")
    }
}

fn value_type_name(value: &ParsedValue) -> String {
    match value {
        ParsedValue::U8(_) => "u8".to_string(),
        ParsedValue::U16(_) => "u16".to_string(),
        ParsedValue::U32(_) => "u32".to_string(),
        ParsedValue::U64(_) => "u64".to_string(),
        ParsedValue::I8(_) => "i8".to_string(),
        ParsedValue::I16(_) => "i16".to_string(),
        ParsedValue::I32(_) => "i32".to_string(),
        ParsedValue::I64(_) => "i64".to_string(),
        ParsedValue::F32(_) => "f32".to_string(),
        ParsedValue::F64(_) => "f64".to_string(),
        ParsedValue::Bytes(_) => "bytes".to_string(),
        ParsedValue::String(_) => "string".to_string(),
        ParsedValue::BitField(_) => "bitfield".to_string(),
        ParsedValue::Enum { .. } => "enum".to_string(),
        ParsedValue::Array(items) => {
            if let Some(first) = items.first() {
                format!("array<{}>", value_type_name(first))
            } else {
                "array".to_string()
            }
        }
        ParsedValue::Struct(_) => "struct".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::DisplayFormat;

    fn make_leaf(name: &str, value: ParsedValue, checksum: Option<ChecksumResult>) -> ParsedField {
        ParsedField {
            name: name.to_string(),
            path: format!("root.{}", name),
            offset: 0,
            length: 4,
            value,
            display_format: DisplayFormat::Hex,
            truncated: false,
            undecidable: false,
            skipped: false,
            checksum_result: checksum,
            children: Vec::new(),
        }
    }

    #[test]
    fn test_stats_basic() {
        let root = ParsedField {
            name: "root".to_string(),
            path: "root".to_string(),
            offset: 0,
            length: 12,
            value: ParsedValue::Struct(vec![]),
            display_format: DisplayFormat::Hex,
            truncated: false,
            undecidable: false,
            skipped: false,
            checksum_result: None,
            children: vec![
                make_leaf("a", ParsedValue::U32(1), None),
                make_leaf("b", ParsedValue::U16(2), Some(ChecksumResult::Passed)),
                make_leaf("c", ParsedValue::U8(3), Some(ChecksumResult::Failed { expected: 10, actual: 20 })),
            ],
        };
        let stats = ParseStats::from_parsed_field(&root, 100);
        assert_eq!(stats.total_leaf_fields, 3);
        assert_eq!(stats.checksum_passed, 1);
        assert_eq!(stats.checksum_failed, 1);
        assert_eq!(stats.checksum_none, 1);
        assert_eq!(stats.parsed_bytes, 12);
    }
}
