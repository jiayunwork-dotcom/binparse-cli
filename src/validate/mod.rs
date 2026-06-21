use crate::dsl::*;

#[derive(Debug)]
pub struct ValidationError {
    pub location: String,
    pub reason: String,
}

pub fn validate_format_definition(def: &FormatDefinition) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let struct_names: std::collections::HashSet<String> = {
        let mut names: std::collections::HashSet<String> =
            def.structs.iter().map(|s| s.name.clone()).collect();
        names.insert(def.root.name.clone());
        names
    };
    let enum_names: std::collections::HashSet<String> =
        def.enums.iter().map(|e| e.name.clone()).collect();

    validate_struct_fields(
        &def.root,
        def,
        &struct_names,
        &enum_names,
        &mut errors,
    );
    for s in &def.structs {
        validate_struct_fields(s, def, &struct_names, &enum_names, &mut errors);
    }

    for enum_def in &def.enums {
        validate_enum(enum_def, def, &mut errors);
    }

    errors
}

fn validate_struct_fields(
    struct_def: &StructDefinition,
    _def: &FormatDefinition,
    struct_names: &std::collections::HashSet<String>,
    enum_names: &std::collections::HashSet<String>,
    errors: &mut Vec<ValidationError>,
) {
    let mut defined_fields: Vec<(String, DataType)> = Vec::new();

    for field in &struct_def.fields {
        let location = format!("{}.{}", struct_def.name, field.name);

        if let Some(offset_expr) = &field.offset {
            if offset_expr != "relative" {
                validate_offset_expression(offset_expr, &defined_fields, &location, errors);
            }
        }

        validate_data_type(
            &field.data_type,
            &defined_fields,
            &location,
            struct_names,
            enum_names,
            errors,
        );

        if let Some(cond) = &field.condition {
            validate_when_expression(&cond.expression, &defined_fields, &location, errors);
        }

        defined_fields.push((field.name.clone(), field.data_type.clone()));
    }
}

fn validate_offset_expression(
    expr: &str,
    defined_fields: &[(String, DataType)],
    location: &str,
    errors: &mut Vec<ValidationError>,
) {
    let identifiers = extract_identifiers(expr);
    for ident in identifiers {
        if ident.chars().next().map_or(false, |c| c.is_ascii_digit()) {
            continue;
        }
        let found = defined_fields.iter().any(|(name, _)| name == &ident);
        if !found {
            errors.push(ValidationError {
                location: location.to_string(),
                reason: format!("offset表达式引用了未定义或尚未定义的字段 '{}'", ident),
            });
        }
    }
}

fn validate_data_type(
    data_type: &DataType,
    defined_fields: &[(String, DataType)],
    location: &str,
    struct_names: &std::collections::HashSet<String>,
    enum_names: &std::collections::HashSet<String>,
    errors: &mut Vec<ValidationError>,
) {
    match data_type {
        DataType::Array { element_type, length } => {
            let length_idents = extract_identifiers(length);
            for ident in length_idents {
                if ident.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                    continue;
                }
                let field_type = defined_fields.iter().find(|(name, _)| name == &ident);
                if let Some((_, ft)) = field_type {
                    if !is_integer_type(ft) {
                        errors.push(ValidationError {
                            location: location.to_string(),
                            reason: format!("数组长度表达式引用的字段 '{}' 不是整数类型", ident),
                        });
                    }
                } else {
                    errors.push(ValidationError {
                        location: location.to_string(),
                        reason: format!("数组长度表达式引用了未定义或尚未定义的字段 '{}'", ident),
                    });
                }
            }
            validate_data_type(element_type, defined_fields, location, struct_names, enum_names, errors);
        }
        DataType::Struct { name } => {
            if !struct_names.contains(name) {
                errors.push(ValidationError {
                    location: location.to_string(),
                    reason: format!("引用的结构体 '{}' 未定义", name),
                });
            }
        }
        DataType::Enum { name, underlying } => {
            if !enum_names.contains(name) {
                errors.push(ValidationError {
                    location: location.to_string(),
                    reason: format!("引用的枚举 '{}' 未定义", name),
                });
            }
            validate_data_type(underlying, defined_fields, location, struct_names, enum_names, errors);
        }
        DataType::Bytes { length } | DataType::String { length, .. } => {
            let length_idents = extract_identifiers(length);
            for ident in length_idents {
                if ident.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                    continue;
                }
                let found = defined_fields.iter().any(|(name, _)| name == &ident);
                if !found {
                    errors.push(ValidationError {
                        location: location.to_string(),
                        reason: format!("长度表达式引用了未定义或尚未定义的字段 '{}'", ident),
                    });
                }
            }
        }
        _ => {}
    }
}

fn validate_when_expression(
    expr: &str,
    defined_fields: &[(String, DataType)],
    location: &str,
    errors: &mut Vec<ValidationError>,
) {
    let trimmed = expr.trim();

    let (op, parts) = if let Some(pos) = trimmed.find("!=") {
        ("!=", trimmed.split_at(pos))
    } else if let Some(pos) = trimmed.find("==") {
        ("==", trimmed.split_at(pos))
    } else {
        errors.push(ValidationError {
            location: location.to_string(),
            reason: "when表达式语法错误: 只允许 == 和 != 两种比较操作符".to_string(),
        });
        return;
    };

    let left = parts.0.trim();
    let right_full = parts.1.trim_start_matches(op).trim();

    let left_parts: Vec<&str> = left.split('.').collect();
    let left_base = left_parts.iter().rev().find(|p| !p.is_empty()).unwrap_or(&"");

    let found = defined_fields.iter().any(|(name, _)| name == *left_base);
    if !found {
        let is_path = left_parts.len() > 1;
        if is_path {
            let root_found = defined_fields.iter().any(|(name, _)| name == left_parts[0]);
            if !root_found {
                errors.push(ValidationError {
                    location: location.to_string(),
                    reason: format!("when表达式左侧 '{}' 不是已定义的字段路径", left),
                });
            }
        } else {
            errors.push(ValidationError {
                location: location.to_string(),
                reason: format!("when表达式左侧 '{}' 不是已定义的字段", left),
            });
        }
    }

    if !is_integer_literal(right_full) {
        errors.push(ValidationError {
            location: location.to_string(),
            reason: format!("when表达式右侧 '{}' 不是整数字面量", right_full),
        });
    }
}

fn is_integer_literal(s: &str) -> bool {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        s[2..].chars().all(|c| c.is_ascii_hexdigit())
    } else if s.starts_with('-') || s.starts_with('+') {
        s[1..].chars().all(|c| c.is_ascii_digit())
    } else {
        s.chars().all(|c| c.is_ascii_digit())
    }
}

fn is_integer_type(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::U8
            | DataType::U16
            | DataType::U32
            | DataType::U64
            | DataType::I8
            | DataType::I16
            | DataType::I32
            | DataType::I64
    )
}

fn extract_identifiers(expr: &str) -> Vec<String> {
    let mut identifiers = Vec::new();
    let mut current = String::new();

    for c in expr.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            current.push(c);
        } else {
            if !current.is_empty() && !current.chars().all(|c| c.is_ascii_digit()) {
                identifiers.push(current.clone());
            }
            current.clear();
        }
    }
    if !current.is_empty() && !current.chars().all(|c| c.is_ascii_digit()) {
        identifiers.push(current);
    }

    identifiers
}

fn validate_enum(enum_def: &EnumDefinition, def: &FormatDefinition, errors: &mut Vec<ValidationError>) {
    let (min_val, max_val) = find_enum_underlying_range(def, &enum_def.name);

    for (name, value) in &enum_def.values {
        if *value < min_val || *value > max_val {
            errors.push(ValidationError {
                location: format!("枚举.{}.{}", enum_def.name, name),
                reason: format!(
                    "枚举值 {} 超出underlying类型范围({}~{})",
                    value, min_val, max_val
                ),
            });
        }
    }
}

fn find_enum_underlying_range(def: &FormatDefinition, enum_name: &str) -> (i64, i64) {
    for s in &def.structs {
        for f in &s.fields {
            if let DataType::Enum { name, underlying } = &f.data_type {
                if name == enum_name {
                    return type_range(underlying);
                }
            }
            if let DataType::Array { element_type, .. } = &f.data_type {
                if let DataType::Enum { name, underlying } = &**element_type {
                    if name == enum_name {
                        return type_range(underlying);
                    }
                }
            }
        }
    }
    for f in &def.root.fields {
        if let DataType::Enum { name, underlying } = &f.data_type {
            if name == enum_name {
                return type_range(underlying);
            }
        }
        if let DataType::Array { element_type, .. } = &f.data_type {
            if let DataType::Enum { name, underlying } = &**element_type {
                if name == enum_name {
                    return type_range(underlying);
                }
            }
        }
    }
    (0, 255)
}

fn type_range(data_type: &DataType) -> (i64, i64) {
    match data_type {
        DataType::U8 => (0, 255),
        DataType::U16 => (0, 65535),
        DataType::U32 => (0, 4294967295),
        DataType::U64 => (0, i64::MAX),
        DataType::I8 => (-128, 127),
        DataType::I16 => (-32768, 32767),
        DataType::I32 => (-2147483648, 2147483647),
        DataType::I64 => (i64::MIN, i64::MAX),
        _ => (0, 255),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_format() {
        let yaml = r#"
name: test
root:
  name: root
  fields:
    - name: magic
      type: u32
      offset: "0"
    - name: length
      type: u16
      offset: relative
"#;
        let def = FormatDefinition::from_yaml_unvalidated(yaml).unwrap();
        let errors = validate_format_definition(&def);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_validate_undefined_field_in_offset() {
        let yaml = r#"
name: test
root:
  name: root
  fields:
    - name: magic
      type: u32
      offset: "0"
    - name: data
      type: u16
      offset: "undefined_field + 4"
"#;
        let def = FormatDefinition::from_yaml_unvalidated(yaml).unwrap();
        let errors = validate_format_definition(&def);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.reason.contains("未定义")));
    }

    #[test]
    fn test_validate_undefined_struct() {
        let yaml = r#"
name: test
root:
  name: root
  fields:
    - name: header
      type:
        struct: NonExistent
      offset: "0"
"#;
        let def = FormatDefinition::from_yaml_unvalidated(yaml).unwrap();
        let errors = validate_format_definition(&def);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.reason.contains("未定义")));
    }

    #[test]
    fn test_validate_enum_value_out_of_range() {
        let yaml = r#"
name: test
enums:
  - name: TestEnum
    values:
      BIG_VAL: 300
root:
  name: root
  fields:
    - name: val
      type:
        enum:
          name: TestEnum
          underlying: u8
      offset: "0"
"#;
        let def = FormatDefinition::from_yaml_unvalidated(yaml).unwrap();
        let errors = validate_format_definition(&def);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.reason.contains("underlying类型范围")));
    }

    #[test]
    fn test_validate_invalid_when_expression() {
        let yaml = r#"
name: test
root:
  name: root
  fields:
    - name: kind
      type: u8
      offset: "0"
    - name: data
      type: u32
      offset: relative
      condition:
        when: kind > 5
"#;
        let def = FormatDefinition::from_yaml_unvalidated(yaml).unwrap();
        let errors = validate_format_definition(&def);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.reason.contains("==") || e.reason.contains("!=")));
    }

    #[test]
    fn test_validate_array_length_non_integer_ref() {
        let yaml = r#"
name: test
root:
  name: root
  fields:
    - name: name
      type:
        string:
          length: "10"
          encoding: ascii
      offset: "0"
    - name: items
      type:
        array:
          element_type: u8
          length: name
      offset: relative
"#;
        let def = FormatDefinition::from_yaml_unvalidated(yaml).unwrap();
        let errors = validate_format_definition(&def);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.reason.contains("不是整数类型")));
    }

    #[test]
    fn test_validate_enum_u16_range() {
        let yaml = r#"
name: test
enums:
  - name: TestEnum
    values:
      BIG_VAL: 70000
root:
  name: root
  fields:
    - name: val
      type:
        enum:
          name: TestEnum
          underlying: u16
      offset: "0"
"#;
        let def = FormatDefinition::from_yaml_unvalidated(yaml).unwrap();
        let errors = validate_format_definition(&def);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.reason.contains("underlying类型范围")));
    }
}
