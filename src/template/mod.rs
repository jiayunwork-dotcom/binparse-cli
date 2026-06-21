use crate::dsl::*;
use evalexpr::ContextWithMutableVariables;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TemplateError {
    #[error("参数类型错误: 参数 '{name}' 期望类型为 {expected}, 实际值为 '{value}'")]
    ParameterTypeError {
        name: String,
        expected: String,
        value: String,
    },
    #[error("缺少必填参数: '{0}'")]
    MissingParameter(String),
    #[error("模板定义错误: {0}")]
    TemplateDefinitionError(String),
    #[error("YAML解析错误: {0}")]
    YamlError(#[from] serde_yaml::Error),
    #[error("IO错误: {0}")]
    IoError(#[from] std::io::Error),
    #[error("条件表达式求值错误: {0}")]
    ExpressionError(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    Int,
    String,
    Bool,
}

impl std::fmt::Display for ParameterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParameterType::Int => write!(f, "int"),
            ParameterType::String => write!(f, "string"),
            ParameterType::Bool => write!(f, "bool"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: ParameterType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_yaml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Vec<TemplateParameter>>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magic: Option<Vec<u8>>,
    #[serde(default)]
    pub enums: Vec<EnumDefinition>,
    #[serde(default)]
    pub structs: Vec<TemplateStructDefinition>,
    pub root: TemplateStructDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateStructDefinition {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magic: Option<Vec<u8>>,
    pub fields: Vec<TemplateField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateField {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
    #[serde(rename = "type")]
    pub data_type: serde_yaml::Value,
    #[serde(default)]
    pub endian: Endian,
    #[serde(default, rename = "format")]
    pub display_format: DisplayFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<Condition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<ChecksumField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_repeat: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub template_path: String,
    pub parameters: HashMap<String, serde_yaml::Value>,
}

impl TemplateDefinition {
    pub fn from_yaml(yaml_str: &str) -> Result<Self, TemplateError> {
        let def: TemplateDefinition = serde_yaml::from_str(yaml_str)?;
        Ok(def)
    }

    pub fn from_file(path: &Path) -> Result<Self, TemplateError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml(&content)
    }

    pub fn get_parameters(&self) -> &[TemplateParameter] {
        self.parameters.as_ref().map(|p| p.as_slice()).unwrap_or(&[])
    }

    fn resolve_parameters(
        &self,
        instance_params: &HashMap<String, serde_yaml::Value>,
    ) -> Result<HashMap<String, serde_yaml::Value>, TemplateError> {
        let mut resolved = HashMap::new();

        for param in self.get_parameters() {
            let value = if let Some(v) = instance_params.get(&param.name) {
                validate_param_type(&param.name, &param.param_type, v)?;
                v.clone()
            } else if let Some(default) = &param.default {
                validate_param_type(&param.name, &param.param_type, default)?;
                default.clone()
            } else {
                return Err(TemplateError::MissingParameter(param.name.clone()));
            };
            resolved.insert(param.name.clone(), value);
        }

        Ok(resolved)
    }

    pub fn instantiate(
        &self,
        instance_params: &HashMap<String, serde_yaml::Value>,
    ) -> Result<FormatDefinition, TemplateError> {
        let resolved = self.resolve_parameters(instance_params)?;

        let root = self.instantiate_struct(&self.root, &resolved)?;
        let mut structs = Vec::new();
        for s in &self.structs {
            structs.push(self.instantiate_struct(s, &resolved)?);
        }

        Ok(FormatDefinition {
            name: substitute_string(&self.name, &resolved),
            magic: self.magic.clone(),
            enums: self.enums.clone(),
            structs,
            root,
        })
    }

    fn instantiate_struct(
        &self,
        template_struct: &TemplateStructDefinition,
        params: &HashMap<String, serde_yaml::Value>,
    ) -> Result<StructDefinition, TemplateError> {
        let mut fields = Vec::new();

        for template_field in &template_struct.fields {
            if let Some(when_expr) = &template_field.template_when {
                let result = evaluate_template_when(when_expr, params)?;
                if !result {
                    continue;
                }
            }

            if let Some(repeat_expr) = &template_field.template_repeat {
                let count = resolve_repeat_count(repeat_expr, params)?;
                for i in 0..count {
                    let mut expanded = template_field.clone();
                    expanded.name = format!("{}_{}", template_field.name, i);
                    expanded.template_when = None;
                    expanded.template_repeat = None;
                    let field = self.instantiate_field(&expanded, params)?;
                    fields.push(field);
                }
            } else {
                let mut field_template = template_field.clone();
                field_template.template_when = None;
                field_template.template_repeat = None;
                let field = self.instantiate_field(&field_template, params)?;
                fields.push(field);
            }
        }

        Ok(StructDefinition {
            name: substitute_string(&template_struct.name, params),
            magic: template_struct.magic.clone(),
            fields,
        })
    }

    fn instantiate_field(
        &self,
        template_field: &TemplateField,
        params: &HashMap<String, serde_yaml::Value>,
    ) -> Result<Field, TemplateError> {
        let data_type = instantiate_data_type(&template_field.data_type, params)?;

        Ok(Field {
            name: substitute_string(&template_field.name, params),
            offset: template_field.offset.as_ref().map(|o| substitute_string(o, params)),
            data_type,
            endian: template_field.endian,
            display_format: template_field.display_format,
            condition: template_field.condition.clone(),
            checksum: template_field.checksum.clone(),
            description: template_field.description.as_ref().map(|d| substitute_string(d, params)),
        })
    }

    pub fn validate_template(&self) -> Vec<TemplateValidationError> {
        let mut errors = Vec::new();
        let param_names: HashMap<&str, &TemplateParameter> = self
            .get_parameters()
            .iter()
            .map(|p| (p.name.as_str(), p))
            .collect();

        let mut seen_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for param in self.get_parameters() {
            if seen_names.contains(param.name.as_str()) {
                errors.push(TemplateValidationError {
                    location: format!("parameters.{}", param.name),
                    reason: format!("参数名 '{}' 重复", param.name),
                });
            }
            seen_names.insert(param.name.as_str());

            if let Some(default) = &param.default {
                if !validate_param_type_silent(&param.param_type, default) {
                    errors.push(TemplateValidationError {
                        location: format!("parameters.{}", param.name),
                        reason: format!(
                            "参数 '{}' 的default值类型与声明类型不匹配 (期望: {})",
                            param.name, param.param_type
                        ),
                    });
                }
            }
        }

        self.validate_struct_template(&self.root, &param_names, &mut errors);
        for s in &self.structs {
            self.validate_struct_template(s, &param_names, &mut errors);
        }

        errors
    }

    fn validate_struct_template(
        &self,
        template_struct: &TemplateStructDefinition,
        param_names: &HashMap<&str, &TemplateParameter>,
        errors: &mut Vec<TemplateValidationError>,
    ) {
        for field in &template_struct.fields {
            let location = format!("{}.{}", template_struct.name, field.name);

            if let Some(when_expr) = &field.template_when {
                let mut referenced = extract_param_refs_from_expr(when_expr);
                let bare_refs = extract_bare_identifiers(when_expr);
                for r in bare_refs {
                    if !referenced.iter().any(|e| e == &r) {
                        referenced.push(r);
                    }
                }
                for param_ref in referenced {
                    if !param_names.contains_key(param_ref.as_str()) {
                        errors.push(TemplateValidationError {
                            location: location.clone(),
                            reason: format!(
                                "template_when表达式引用了未声明的参数 '{}'",
                                param_ref
                            ),
                        });
                    }
                }
            }

            validate_yaml_value_params(&field.data_type, param_names, &location, errors);

            if let Some(offset) = &field.offset {
                validate_param_refs_in_string(offset, param_names, &location, "offset", errors);
            }

            if let Some(repeat_expr) = &field.template_repeat {
                let mut referenced = extract_param_refs_from_expr(repeat_expr);
                let bare_refs = extract_bare_identifiers(repeat_expr);
                for r in bare_refs {
                    if !referenced.iter().any(|e| e == &r) {
                        referenced.push(r);
                    }
                }
                for param_ref in referenced {
                    match param_names.get(param_ref.as_str()) {
                        Some(param) => {
                            if param.param_type != ParameterType::Int {
                                errors.push(TemplateValidationError {
                                    location: location.clone(),
                                    reason: format!(
                                        "template_repeat引用的参数 '{}' 不是int类型 (实际: {})",
                                        param_ref, param.param_type
                                    ),
                                });
                            }
                        }
                        None => {
                            errors.push(TemplateValidationError {
                                location: location.clone(),
                                reason: format!(
                                    "template_repeat引用了未声明的参数 '{}'",
                                    param_ref
                                ),
                            });
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct TemplateValidationError {
    pub location: String,
    pub reason: String,
}

impl InstanceConfig {
    pub fn from_yaml(yaml_str: &str) -> Result<Self, TemplateError> {
        let config: InstanceConfig = serde_yaml::from_str(yaml_str)?;
        Ok(config)
    }

    pub fn from_file(path: &Path) -> Result<Self, TemplateError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml(&content)
    }
}

fn validate_param_type(
    name: &str,
    expected_type: &ParameterType,
    value: &serde_yaml::Value,
) -> Result<(), TemplateError> {
    let type_ok = match expected_type {
        ParameterType::Int => value.is_number() || (value.is_string() && value.as_str().map_or(false, |s| s.parse::<i64>().is_ok())),
        ParameterType::Bool => {
            value.is_bool()
                || (value.is_string()
                    && value
                        .as_str()
                        .map_or(false, |s| s == "true" || s == "false"))
        }
        ParameterType::String => true,
    };

    if !type_ok {
        return Err(TemplateError::ParameterTypeError {
            name: name.to_string(),
            expected: expected_type.to_string(),
            value: format!("{:?}", value),
        });
    }

    Ok(())
}

fn validate_param_type_silent(param_type: &ParameterType, value: &serde_yaml::Value) -> bool {
    match param_type {
        ParameterType::Int => value.is_number() || (value.is_string() && value.as_str().map_or(false, |s| s.parse::<i64>().is_ok())),
        ParameterType::Bool => {
            value.is_bool()
                || (value.is_string()
                    && value
                        .as_str()
                        .map_or(false, |s| s == "true" || s == "false"))
        }
        ParameterType::String => true,
    }
}

fn substitute_string(s: &str, params: &HashMap<String, serde_yaml::Value>) -> String {
    let mut result = s.to_string();
    for (key, value) in params {
        let placeholder = format!("${{{}}}", key);
        if result.contains(&placeholder) {
            let replacement = match value {
                serde_yaml::Value::Number(n) => n.to_string(),
                serde_yaml::Value::Bool(b) => b.to_string(),
                serde_yaml::Value::String(s) => s.clone(),
                _ => format!("{:?}", value),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }
    result
}

fn evaluate_template_when(
    expr: &str,
    params: &HashMap<String, serde_yaml::Value>,
) -> Result<bool, TemplateError> {
    let mut eval_expr = expr.to_string();

    for (key, value) in params {
        let placeholder = format!("${{{}}}", key);
        let replacement = match value {
            serde_yaml::Value::Number(n) => n.to_string(),
            serde_yaml::Value::Bool(b) => b.to_string(),
            serde_yaml::Value::String(s) => format!("\"{}\"", s),
            _ => format!("{:?}", value),
        };
        eval_expr = eval_expr.replace(&placeholder, &replacement);
    }

    let mut context = evalexpr::HashMapContext::new();
    for (key, value) in params {
        match value {
            serde_yaml::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    let _ = context.set_value(key.clone(), evalexpr::Value::Int(i));
                } else if let Some(f) = n.as_f64() {
                    let _ = context.set_value(key.clone(), evalexpr::Value::Float(f));
                }
            }
            serde_yaml::Value::Bool(b) => {
                let _ = context.set_value(key.clone(), evalexpr::Value::Boolean(*b));
            }
            serde_yaml::Value::String(s) => {
                let _ = context.set_value(
                    key.clone(),
                    evalexpr::Value::String(s.clone()),
                );
            }
            _ => {}
        }
    }

    eval_expr = eval_expr.replace(" and ", " && ");
    eval_expr = eval_expr.replace(" or ", " || ");
    eval_expr = eval_expr.replace("not ", "!");

    match evalexpr::eval_with_context(&eval_expr, &context) {
        Ok(evalexpr::Value::Boolean(b)) => Ok(b),
        Ok(other) => Err(TemplateError::ExpressionError(format!(
            "表达式求值结果不是布尔类型: {:?}",
            other
        ))),
        Err(e) => Err(TemplateError::ExpressionError(format!(
            "表达式求值失败 '{}': {}",
            expr, e
        ))),
    }
}

fn resolve_repeat_count(
    expr: &str,
    params: &HashMap<String, serde_yaml::Value>,
) -> Result<usize, TemplateError> {
    let mut eval_expr = expr.to_string();

    for (key, value) in params {
        let placeholder = format!("${{{}}}", key);
        if eval_expr.contains(&placeholder) {
            let replacement = match value {
                serde_yaml::Value::Number(n) => n.to_string(),
                _ => format!("{:?}", value),
            };
            eval_expr = eval_expr.replace(&placeholder, &replacement);
        }
    }

    let mut context = evalexpr::HashMapContext::new();
    for (key, value) in params {
        if let serde_yaml::Value::Number(n) = value {
            if let Some(i) = n.as_i64() {
                let _ = context.set_value(key.clone(), evalexpr::Value::Int(i));
            }
        }
    }

    match evalexpr::eval_with_context(&eval_expr, &context) {
        Ok(evalexpr::Value::Int(n)) => {
            if n < 0 {
                return Err(TemplateError::ExpressionError(format!(
                    "template_repeat求值结果为负数: {}",
                    n
                )));
            }
            Ok(n as usize)
        }
        Ok(other) => Err(TemplateError::ExpressionError(format!(
            "template_repeat求值结果不是整数: {:?}",
            other
        ))),
        Err(e) => Err(TemplateError::ExpressionError(format!(
            "template_repeat表达式求值失败 '{}': {}",
            expr, e
        ))),
    }
}

fn instantiate_data_type(
    yaml_value: &serde_yaml::Value,
    params: &HashMap<String, serde_yaml::Value>,
) -> Result<DataType, TemplateError> {
    let substituted = substitute_yaml_value(yaml_value, params);
    DataType::from_yaml_value(&substituted)
        .map_err(|e| TemplateError::TemplateDefinitionError(format!("数据类型实例化失败: {}", e)))
}

fn substitute_yaml_value(
    value: &serde_yaml::Value,
    params: &HashMap<String, serde_yaml::Value>,
) -> serde_yaml::Value {
    match value {
        serde_yaml::Value::String(s) => {
            let substituted = substitute_string(s, params);
            serde_yaml::Value::String(substituted)
        }
        serde_yaml::Value::Mapping(map) => {
            let mut new_map = serde_yaml::Mapping::new();
            for (k, v) in map {
                let new_k = substitute_yaml_value(k, params);
                let new_v = substitute_yaml_value(v, params);
                new_map.insert(new_k, new_v);
            }
            serde_yaml::Value::Mapping(new_map)
        }
        serde_yaml::Value::Sequence(seq) => {
            let new_seq: Vec<serde_yaml::Value> =
                seq.iter().map(|v| substitute_yaml_value(v, params)).collect();
            serde_yaml::Value::Sequence(new_seq)
        }
        other => other.clone(),
    }
}

fn extract_param_refs_from_expr(expr: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut start = 0;
    while let Some(pos) = expr[start..].find("${") {
        let abs_pos = start + pos;
        if let Some(end) = expr[abs_pos..].find('}') {
            let param_name = &expr[abs_pos + 2..abs_pos + end];
            if !param_name.is_empty() {
                refs.push(param_name.to_string());
            }
            start = abs_pos + end + 1;
        } else {
            break;
        }
    }
    refs
}

fn extract_bare_identifiers(expr: &str) -> Vec<String> {
    let mut identifiers = Vec::new();
    let mut current = String::new();
    for c in expr.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            current.push(c);
        } else {
            if !current.is_empty()
                && !current.chars().next().map_or(false, |c| c.is_ascii_digit())
                && current != "true"
                && current != "false"
            {
                identifiers.push(current.clone());
            }
            current.clear();
        }
    }
    if !current.is_empty()
        && !current.chars().next().map_or(false, |c| c.is_ascii_digit())
        && current != "true"
        && current != "false"
    {
        identifiers.push(current);
    }
    identifiers
}

fn validate_param_refs_in_string(
    s: &str,
    param_names: &HashMap<&str, &TemplateParameter>,
    location: &str,
    field_name: &str,
    errors: &mut Vec<TemplateValidationError>,
) {
    let refs = extract_param_refs_from_expr(s);
    for param_ref in refs {
        if !param_names.contains_key(param_ref.as_str()) {
            errors.push(TemplateValidationError {
                location: location.to_string(),
                reason: format!(
                    "{}中${{{}}}占位符引用了未声明的参数 '{}'",
                    field_name, param_ref, param_ref
                ),
            });
        }
    }
}

fn validate_yaml_value_params(
    value: &serde_yaml::Value,
    param_names: &HashMap<&str, &TemplateParameter>,
    location: &str,
    errors: &mut Vec<TemplateValidationError>,
) {
    match value {
        serde_yaml::Value::String(s) => {
            validate_param_refs_in_string(s, param_names, location, "type", errors);
        }
        serde_yaml::Value::Mapping(map) => {
            for (k, v) in map {
                if let serde_yaml::Value::String(key) = k {
                    validate_param_refs_in_string(key, param_names, location, "type", errors);
                }
                match v {
                    serde_yaml::Value::String(s) => {
                        validate_param_refs_in_string(s, param_names, location, "type", errors);
                    }
                    serde_yaml::Value::Mapping(inner_map) => {
                        for (ik, iv) in inner_map {
                            if let serde_yaml::Value::String(s) = ik {
                                validate_param_refs_in_string(
                                    s,
                                    param_names,
                                    location,
                                    "type",
                                    errors,
                                );
                            }
                            if let serde_yaml::Value::String(s) = iv {
                                validate_param_refs_in_string(
                                    s,
                                    param_names,
                                    location,
                                    "type",
                                    errors,
                                );
                            }
                        }
                    }
                    serde_yaml::Value::Sequence(seq) => {
                        for item in seq {
                            validate_yaml_value_params(item, param_names, location, errors);
                        }
                    }
                    _ => {}
                }
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                validate_yaml_value_params(item, param_names, location, errors);
            }
        }
        _ => {}
    }
}

pub fn format_params_table(params: &[TemplateParameter]) -> String {
    let mut table = String::new();
    table.push_str(&format!(
        "{:<20} {:<10} {:<15} {:<10}\n",
        "名称", "类型", "默认值", "必填"
    ));
    table.push_str(&format!(
        "{:<20} {:<10} {:<15} {:<10}\n",
        "----", "----", "------", "----"
    ));
    for param in params {
        let default_str = match &param.default {
            Some(v) => match v {
                serde_yaml::Value::Number(n) => n.to_string(),
                serde_yaml::Value::Bool(b) => b.to_string(),
                serde_yaml::Value::String(s) => s.clone(),
                _ => format!("{:?}", v),
            },
            None => "-".to_string(),
        };
        let required = if param.default.is_some() {
            "否"
        } else {
            "是"
        };
        table.push_str(&format!(
            "{:<20} {:<10} {:<15} {:<10}\n",
            param.name, param.param_type, default_str, required
        ));
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_param_refs() {
        let expr = "${header_size} + ${extra_offset}";
        let refs = extract_param_refs_from_expr(expr);
        assert_eq!(refs, vec!["header_size", "extra_offset"]);
    }

    #[test]
    fn test_substitute_string() {
        let mut params = HashMap::new();
        params.insert("payload_size".to_string(), serde_yaml::Value::Number(64.into()));
        params.insert("name".to_string(), serde_yaml::Value::String("test".to_string()));
        let result = substitute_string("${payload_size}", &params);
        assert_eq!(result, "64");
        let result2 = substitute_string("prefix_${name}_suffix", &params);
        assert_eq!(result2, "prefix_test_suffix");
    }

    #[test]
    fn test_evaluate_template_when() {
        let mut params = HashMap::new();
        params.insert("has_checksum".to_string(), serde_yaml::Value::Bool(true));
        params.insert("version".to_string(), serde_yaml::Value::Number(2.into()));
        assert!(evaluate_template_when("has_checksum == true", &params).unwrap());
        assert!(evaluate_template_when("version == 2", &params).unwrap());
        assert!(!evaluate_template_when("version == 1", &params).unwrap());
        assert!(evaluate_template_when("has_checksum == true and version == 2", &params).unwrap());
        assert!(!evaluate_template_when("has_checksum == true and version == 1", &params).unwrap());
        assert!(evaluate_template_when("has_checksum == true or version == 1", &params).unwrap());
        assert!(evaluate_template_when("not has_checksum == false", &params).unwrap());
    }

    #[test]
    fn test_resolve_repeat_count() {
        let mut params = HashMap::new();
        params.insert("channel_count".to_string(), serde_yaml::Value::Number(4.into()));
        assert_eq!(resolve_repeat_count("channel_count", &params).unwrap(), 4);
        assert_eq!(resolve_repeat_count("${channel_count}", &params).unwrap(), 4);
    }

    #[test]
    fn test_validate_param_type() {
        assert!(validate_param_type("test", &ParameterType::Int, &serde_yaml::Value::Number(42.into())).is_ok());
        assert!(validate_param_type("test", &ParameterType::Int, &serde_yaml::Value::String("not_int".into())).is_err());
        assert!(validate_param_type("test", &ParameterType::Bool, &serde_yaml::Value::Bool(true)).is_ok());
        assert!(validate_param_type("test", &ParameterType::String, &serde_yaml::Value::String("hello".into())).is_ok());
    }

    #[test]
    fn test_template_instantiate() {
        let yaml = r#"
parameters:
  - name: header_size
    type: int
    default: 16
  - name: has_checksum
    type: bool
    default: true
  - name: channel_count
    type: int
name: DeviceFirmware
root:
  name: Header
  fields:
    - name: magic
      type:
        bytes:
          length: "4"
      offset: "0"
      format: hex
    - name: header_data
      type:
        bytes:
          length: "${header_size}"
      offset: relative
    - name: checksum
      type: u32
      offset: relative
      template_when: "has_checksum == true"
    - name: channel
      type: u8
      offset: relative
      template_repeat: "channel_count"
"#;
        let template = TemplateDefinition::from_yaml(yaml).unwrap();
        let mut params = HashMap::new();
        params.insert("channel_count".to_string(), serde_yaml::Value::Number(3.into()));
        let result = template.instantiate(&params).unwrap();
        assert_eq!(result.name, "DeviceFirmware");
        assert_eq!(result.root.fields.len(), 6);
        
        // 验证 bytes 类型的 length 参数替换
        let header_data_field = &result.root.fields[1];
        assert_eq!(header_data_field.name, "header_data");
        match &header_data_field.data_type {
            DataType::Bytes { length } => {
                assert_eq!(length, "16"); // 默认值 16
            }
            _ => panic!("Expected Bytes type"),
        }
        
        // 验证 offset 字段的参数替换
        // offset 是 "relative"，不包含占位符
    }

    #[test]
    fn test_template_validate() {
        let yaml = r#"
parameters:
  - name: header_size
    type: int
    default: 16
  - name: has_checksum
    type: bool
name: TestTemplate
root:
  name: Header
  fields:
    - name: magic
      type:
        bytes:
          length: "${header_size}"
      offset: "0"
    - name: checksum
      type: u32
      offset: relative
      template_when: "has_checksum == true"
    - name: bad_field
      type: u32
      offset: relative
      template_when: "${undeclared_param} == true"
"#;
        let template = TemplateDefinition::from_yaml(yaml).unwrap();
        let errors = template.validate_template();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.reason.contains("未声明的参数")));
    }

    #[test]
    fn test_template_validate_repeat_not_int() {
        let yaml = r#"
parameters:
  - name: channel_count
    type: string
  - name: count_val
    type: int
name: TestTemplate
root:
  name: Header
  fields:
    - name: channel
      type: u8
      offset: "0"
      template_repeat: "channel_count"
    - name: other
      type: u8
      offset: relative
      template_repeat: "count_val"
"#;
        let template = TemplateDefinition::from_yaml(yaml).unwrap();
        let errors = template.validate_template();
        assert!(errors.iter().any(|e| e.reason.contains("不是int类型")));
    }

    #[test]
    fn test_missing_required_param() {
        let yaml = r#"
parameters:
  - name: header_size
    type: int
  - name: has_checksum
    type: bool
    default: true
name: Test
root:
  name: Header
  fields:
    - name: magic
      type: u32
      offset: "0"
"#;
        let template = TemplateDefinition::from_yaml(yaml).unwrap();
        let params = HashMap::new();
        let result = template.instantiate(&params);
        assert!(matches!(result, Err(TemplateError::MissingParameter(_))));
    }

    #[test]
    fn test_param_type_mismatch() {
        let yaml = r#"
parameters:
  - name: header_size
    type: int
name: Test
root:
  name: Header
  fields:
    - name: magic
      type: u32
      offset: "0"
"#;
        let template = TemplateDefinition::from_yaml(yaml).unwrap();
        let mut params = HashMap::new();
        params.insert("header_size".to_string(), serde_yaml::Value::String("not_a_number".into()));
        let result = template.instantiate(&params);
        assert!(matches!(result, Err(TemplateError::ParameterTypeError { .. })));
    }

    #[test]
    fn test_substitute_yaml_value_nested() {
        let mut params = HashMap::new();
        params.insert("header_size".to_string(), serde_yaml::Value::Number(32.into()));

        let yaml = serde_yaml::Value::Mapping({
            let mut outer = serde_yaml::Mapping::new();
            let mut inner = serde_yaml::Mapping::new();
            inner.insert(
                serde_yaml::Value::String("length".to_string()),
                serde_yaml::Value::String("${header_size}".to_string()),
            );
            outer.insert(
                serde_yaml::Value::String("bytes".to_string()),
                serde_yaml::Value::Mapping(inner),
            );
            outer
        });

        let result = substitute_yaml_value(&yaml, &params);

        match &result {
            serde_yaml::Value::Mapping(outer_map) => {
                let bytes_val = outer_map.get(&serde_yaml::Value::String("bytes".to_string())).unwrap();
                match bytes_val {
                    serde_yaml::Value::Mapping(inner_map) => {
                        let length_val = inner_map.get(&serde_yaml::Value::String("length".to_string())).unwrap();
                        match length_val {
                            serde_yaml::Value::String(s) => {
                                assert_eq!(s, "32");
                            }
                            _ => panic!("length should be a string"),
                        }
                    }
                    _ => panic!("bytes value should be a mapping"),
                }
            }
            _ => panic!("result should be a mapping"),
        }

        // 同时验证 DataType::from_yaml_value 能正确解析
        let data_type = DataType::from_yaml_value(&result).unwrap();
        match data_type {
            DataType::Bytes { length } => {
                assert_eq!(length, "32");
            }
            _ => panic!("Expected Bytes type"),
        }
    }

    #[test]
    fn test_duplicate_param_name() {
        let yaml = r#"
parameters:
  - name: size
    type: int
  - name: size
    type: string
name: Test
root:
  name: Header
  fields:
    - name: magic
      type: u32
      offset: "0"
"#;
        let template = TemplateDefinition::from_yaml(yaml).unwrap();
        let errors = template.validate_template();
        assert!(errors.iter().any(|e| e.reason.contains("重复")));
    }

    #[test]
    fn test_default_type_mismatch() {
        let yaml = r#"
parameters:
  - name: size
    type: int
    default: "not_int"
name: Test
root:
  name: Header
  fields:
    - name: magic
      type: u32
      offset: "0"
"#;
        let template = TemplateDefinition::from_yaml(yaml).unwrap();
        let errors = template.validate_template();
        assert!(errors.iter().any(|e| e.reason.contains("default值类型与声明类型不匹配")));
    }
}
