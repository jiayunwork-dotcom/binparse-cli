use serde::{de::Deserializer, Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DslError {
    #[error("YAML parse error: {0}")]
    YamlError(#[from] serde_yaml::Error),
    #[error("Circular reference detected: {0}")]
    CircularReference(String),
    #[error("Invalid expression: {0}")]
    InvalidExpression(String),
    #[error("Unknown struct: {0}")]
    UnknownStruct(String),
    #[error("Unknown enum: {0}")]
    UnknownEnum(String),
    #[error("Invalid data type: {0}")]
    InvalidDataType(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Endian {
    #[serde(rename = "little")]
    Little,
    #[serde(rename = "big")]
    Big,
}

impl Default for Endian {
    fn default() -> Self {
        Endian::Little
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayFormat {
    #[serde(rename = "hex")]
    Hex,
    #[serde(rename = "dec")]
    Dec,
    #[serde(rename = "bin")]
    Bin,
    #[serde(rename = "ascii")]
    Ascii,
    #[serde(rename = "utf8")]
    Utf8,
}

impl Default for DisplayFormat {
    fn default() -> Self {
        DisplayFormat::Hex
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum DataType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    Bytes { length: String },
    String { length: String, encoding: Option<String> },
    BitField { bit_start: u8, bit_length: u8 },
    Struct { name: String },
    Array {
        element_type: Box<DataType>,
        length: String,
    },
    Enum { name: String, underlying: Box<DataType> },
}

impl<'de> Deserialize<'de> for DataType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        DataType::from_yaml_value(&value).map_err(serde::de::Error::custom)
    }
}

impl DataType {
    fn from_yaml_value(value: &serde_yaml::Value) -> Result<Self, String> {
        match value {
            serde_yaml::Value::String(s) => match s.as_str() {
                "u8" => Ok(DataType::U8),
                "u16" => Ok(DataType::U16),
                "u32" => Ok(DataType::U32),
                "u64" => Ok(DataType::U64),
                "i8" => Ok(DataType::I8),
                "i16" => Ok(DataType::I16),
                "i32" => Ok(DataType::I32),
                "i64" => Ok(DataType::I64),
                "f32" => Ok(DataType::F32),
                "f64" => Ok(DataType::F64),
                other => Err(format!("Unknown data type: {}", other)),
            },
            serde_yaml::Value::Mapping(map) => {
                if let Some(bytes_val) = map.get(&serde_yaml::Value::String("bytes".to_string())) {
                    let inner = bytes_val.as_mapping().ok_or("bytes must be a mapping")?;
                    let length = inner.get(&serde_yaml::Value::String("length".to_string()))
                        .and_then(|v| v.as_str())
                        .ok_or("Missing 'length' for bytes type")?.to_string();
                    return Ok(DataType::Bytes { length });
                }
                if let Some(string_val) = map.get(&serde_yaml::Value::String("string".to_string())) {
                    let inner = string_val.as_mapping().ok_or("string must be a mapping")?;
                    let length = inner.get(&serde_yaml::Value::String("length".to_string()))
                        .and_then(|v| v.as_str())
                        .ok_or("Missing 'length' for string type")?.to_string();
                    let encoding = inner.get(&serde_yaml::Value::String("encoding".to_string()))
                        .and_then(|v| v.as_str()).map(|s| s.to_string());
                    return Ok(DataType::String { length, encoding });
                }
                if let Some(bit_field_val) = map.get(&serde_yaml::Value::String("bit_field".to_string())) {
                    let inner = bit_field_val.as_mapping().ok_or("bit_field must be a mapping")?;
                    let bit_start = inner.get(&serde_yaml::Value::String("bit_start".to_string()))
                        .and_then(|v| v.as_u64())
                        .ok_or("Missing 'bit_start' for bit_field type")? as u8;
                    let bit_length = inner.get(&serde_yaml::Value::String("bit_length".to_string()))
                        .and_then(|v| v.as_u64())
                        .ok_or("Missing 'bit_length' for bit_field type")? as u8;
                    return Ok(DataType::BitField { bit_start, bit_length });
                }
                if let Some(struct_val) = map.get(&serde_yaml::Value::String("struct".to_string())) {
                    let name = if let Some(s) = struct_val.as_str() {
                        s.to_string()
                    } else if let Some(inner) = struct_val.as_mapping() {
                        inner.get(&serde_yaml::Value::String("name".to_string()))
                            .and_then(|v| v.as_str())
                            .ok_or("Missing 'name' for struct type")?.to_string()
                    } else {
                        return Err("struct value must be a string or mapping".to_string());
                    };
                    return Ok(DataType::Struct { name });
                }
                if let Some(array_val) = map.get(&serde_yaml::Value::String("array".to_string())) {
                    let inner = array_val.as_mapping().ok_or("array must be a mapping")?;
                    let element_type_val = inner.get(&serde_yaml::Value::String("element_type".to_string()))
                        .ok_or("Missing 'element_type' for array type")?;
                    let element_type = Box::new(DataType::from_yaml_value(element_type_val)?);
                    let length = inner.get(&serde_yaml::Value::String("length".to_string()))
                        .and_then(|v| v.as_str())
                        .ok_or("Missing 'length' for array type")?.to_string();
                    return Ok(DataType::Array { element_type, length });
                }
                if let Some(enum_val) = map.get(&serde_yaml::Value::String("enum".to_string())) {
                    if let Some(inner) = enum_val.as_mapping() {
                        let name = inner.get(&serde_yaml::Value::String("name".to_string()))
                            .and_then(|v| v.as_str())
                            .ok_or("Missing 'name' for enum type")?.to_string();
                        let underlying_val = inner.get(&serde_yaml::Value::String("underlying".to_string()))
                            .ok_or("Missing 'underlying' for enum type")?;
                        let underlying = Box::new(DataType::from_yaml_value(underlying_val)?);
                        return Ok(DataType::Enum { name, underlying });
                    }
                    return Err("enum value must be a mapping".to_string());
                }
                Err("Invalid data type format: expected type name or bytes/string/bit_field/struct/array/enum object".to_string())
            }
            _ => Err("Invalid data type format: expected string or object".to_string()),
        }
    }
    pub fn size(&self) -> Option<usize> {
        match self {
            DataType::U8 | DataType::I8 => Some(1),
            DataType::U16 | DataType::I16 => Some(2),
            DataType::U32 | DataType::I32 | DataType::F32 => Some(4),
            DataType::U64 | DataType::I64 | DataType::F64 => Some(8),
            DataType::BitField { .. } => Some(1),
            DataType::Bytes { length } => length.parse::<usize>().ok(),
            DataType::String { length, .. } => length.parse::<usize>().ok(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    #[serde(rename = "when")]
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecksumField {
    pub algorithm: String,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    #[serde(default)]
    pub offset: Option<String>,
    #[serde(rename = "type")]
    pub data_type: DataType,
    #[serde(default)]
    pub endian: Endian,
    #[serde(default, rename = "format")]
    pub display_format: DisplayFormat,
    #[serde(default)]
    pub condition: Option<Condition>,
    #[serde(default)]
    pub checksum: Option<ChecksumField>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumDefinition {
    pub name: String,
    pub values: HashMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructDefinition {
    pub name: String,
    #[serde(default)]
    pub magic: Option<Vec<u8>>,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatDefinition {
    pub name: String,
    #[serde(default)]
    pub magic: Option<Vec<u8>>,
    #[serde(default)]
    pub enums: Vec<EnumDefinition>,
    #[serde(default)]
    pub structs: Vec<StructDefinition>,
    pub root: StructDefinition,
}

impl FormatDefinition {
    pub fn from_yaml(yaml_str: &str) -> Result<Self, DslError> {
        let def: FormatDefinition = serde_yaml::from_str(yaml_str)?;
        def.validate()?;
        Ok(def)
    }

    pub fn from_yaml_unvalidated(yaml_str: &str) -> Result<Self, DslError> {
        let def: FormatDefinition = serde_yaml::from_str(yaml_str)?;
        Ok(def)
    }

    pub fn validate(&self) -> Result<(), DslError> {
        self.check_circular_references()?;
        self.validate_struct_references()?;
        Ok(())
    }

    fn check_circular_references(&self) -> Result<(), DslError> {
        let mut all_structs = self.structs.clone();
        all_structs.push(self.root.clone());
        
        for struct_def in &all_structs {
            let mut visited = std::collections::HashSet::new();
            let mut path = vec![struct_def.name.clone()];
            self.dfs_circular_check(struct_def, &mut visited, &all_structs, &mut path)?;
        }
        Ok(())
    }

    fn dfs_circular_check(
        &self,
        struct_def: &StructDefinition,
        visited: &mut std::collections::HashSet<String>,
        all_structs: &[StructDefinition],
        path: &mut Vec<String>,
    ) -> Result<(), DslError> {
        visited.insert(struct_def.name.clone());

        for field in &struct_def.fields {
            if let DataType::Struct { name } = &field.data_type {
                if path.contains(name) {
                    path.push(name.clone());
                    return Err(DslError::CircularReference(path.join(" -> ")));
                }
                if !visited.contains(name) {
                    if let Some(s) = all_structs.iter().find(|s| s.name == *name) {
                        path.push(name.clone());
                        self.dfs_circular_check(s, visited, all_structs, path)?;
                        path.pop();
                    }
                }
            }
            if let DataType::Array { element_type, .. } = &field.data_type {
                if let DataType::Struct { name } = &**element_type {
                    if path.contains(name) {
                        path.push(name.clone());
                        return Err(DslError::CircularReference(path.join(" -> ")));
                    }
                    if !visited.contains(name) {
                        if let Some(s) = all_structs.iter().find(|s| s.name == *name) {
                            path.push(name.clone());
                            self.dfs_circular_check(s, visited, all_structs, path)?;
                            path.pop();
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_struct_references(&self) -> Result<(), DslError> {
        let mut struct_names: std::collections::HashSet<String> = 
            self.structs.iter().map(|s| s.name.clone()).collect();
        struct_names.insert(self.root.name.clone());

        let all_structs = &self.structs;
        self.validate_fields(&self.root.fields, &struct_names, all_structs)?;
        for s in &self.structs {
            self.validate_fields(&s.fields, &struct_names, all_structs)?;
        }
        Ok(())
    }

    fn validate_fields(
        &self,
        fields: &[Field],
        struct_names: &std::collections::HashSet<String>,
        all_structs: &[StructDefinition],
    ) -> Result<(), DslError> {
        for field in fields {
            match &field.data_type {
                DataType::Struct { name } => {
                    if !struct_names.contains(name) {
                        return Err(DslError::UnknownStruct(name.clone()));
                    }
                    if let Some(s) = all_structs.iter().find(|s| s.name == *name) {
                        self.validate_fields(&s.fields, struct_names, all_structs)?;
                    }
                }
                DataType::Array { element_type, .. } => {
                    if let DataType::Struct { name } = &**element_type {
                        if !struct_names.contains(name) {
                            return Err(DslError::UnknownStruct(name.clone()));
                        }
                    }
                }
                DataType::Enum { name, .. } => {
                    if !self.enums.iter().any(|e| e.name == *name) {
                        return Err(DslError::UnknownEnum(name.clone()));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn get_struct(&self, name: &str) -> Option<&StructDefinition> {
        if self.root.name == name {
            Some(&self.root)
        } else {
            self.structs.iter().find(|s| s.name == name)
        }
    }

    pub fn get_enum(&self, name: &str) -> Option<&EnumDefinition> {
        self.enums.iter().find(|e| e.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_format() {
        let yaml = r#"
name: test
root:
  name: root
  fields:
    - name: magic
      type: u32
      offset: "0"
      endian: big
    - name: length
      type: u16
      offset: relative
"#;
        let def = FormatDefinition::from_yaml(yaml).unwrap();
        assert_eq!(def.name, "test");
        assert_eq!(def.root.fields.len(), 2);
        assert_eq!(def.root.fields[0].name, "magic");
        assert_eq!(def.root.fields[0].data_type, DataType::U32);
        assert_eq!(def.root.fields[0].endian, Endian::Big);
    }

    #[test]
    fn test_circular_reference_detection() {
        let yaml = r#"
name: test
structs:
  - name: A
    fields:
      - name: b
        type:
          struct: B
  - name: B
    fields:
      - name: a
        type:
          struct: A
root:
  name: root
  fields:
    - name: a
      type:
        struct: A
"#;
        let result = FormatDefinition::from_yaml(yaml);
        assert!(matches!(result, Err(DslError::CircularReference(_))));
    }
}
