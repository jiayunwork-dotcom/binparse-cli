use crate::parser::ParsedField;

pub fn matches_glob(pattern: &str, path: &str) -> bool {
    let pat_parts: Vec<&str> = pattern.split('.').collect();
    let path_parts: Vec<&str> = path.split('.').collect();
    glob_match_recursive(&pat_parts, &path_parts)
}

fn glob_match_recursive(pat_parts: &[&str], path_parts: &[&str]) -> bool {
    if pat_parts.is_empty() && path_parts.is_empty() {
        return true;
    }
    if pat_parts.is_empty() {
        return false;
    }
    let pat_first = pat_parts[0];
    if pat_first == "*" {
        if path_parts.is_empty() {
            return false;
        }
        if glob_match_recursive(&pat_parts[1..], &path_parts[1..]) {
            return true;
        }
        for skip in 1..=path_parts.len() {
            if glob_match_recursive(pat_parts, &path_parts[skip..]) {
                return true;
            }
        }
        false
    } else if pat_first == "**" {
        if glob_match_recursive(&pat_parts[1..], path_parts) {
            return true;
        }
        for skip in 1..=path_parts.len() {
            if glob_match_recursive(pat_parts, &path_parts[skip..]) {
                return true;
            }
        }
        false
    } else {
        if path_parts.is_empty() {
            return false;
        }
        if simple_glob_match(pat_first, path_parts[0]) {
            glob_match_recursive(&pat_parts[1..], &path_parts[1..])
        } else {
            false
        }
    }
}

fn simple_glob_match(pattern: &str, text: &str) -> bool {
    let p_chars: Vec<char> = pattern.chars().collect();
    let t_chars: Vec<char> = text.chars().collect();
    let mut dp = vec![vec![false; t_chars.len() + 1]; p_chars.len() + 1];
    dp[0][0] = true;

    for i in 1..=p_chars.len() {
        if p_chars[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }

    for i in 1..=p_chars.len() {
        for j in 1..=t_chars.len() {
            if p_chars[i - 1] == '*' {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1] || dp[i - 1][j - 1];
            } else if p_chars[i - 1] == '?' || p_chars[i - 1] == t_chars[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }

    dp[p_chars.len()][t_chars.len()]
}

pub fn filter_parsed_field(root: &ParsedField, pattern: &str) -> Option<ParsedField> {
    let filtered = filter_recursive(root, pattern);
    if filtered.children.is_empty() && !matches_glob(pattern, &root.path) {
        return None;
    }
    Some(filtered)
}

fn filter_recursive(field: &ParsedField, pattern: &str) -> ParsedField {
    let matching_children: Vec<ParsedField> = field
        .children
        .iter()
        .map(|child| filter_recursive(child, pattern))
        .filter(|child| {
            matches_glob(pattern, &child.path) || !child.children.is_empty()
        })
        .collect();

    ParsedField {
        name: field.name.clone(),
        path: field.path.clone(),
        offset: field.offset,
        length: field.length,
        value: field.value.clone(),
        display_format: field.display_format,
        truncated: field.truncated,
        undecidable: field.undecidable,
        skipped: field.skipped,
        checksum_result: field.checksum_result.clone(),
        children: matching_children,
    }
}

pub fn has_matching_fields(root: &ParsedField, pattern: &str) -> bool {
    if matches_glob(pattern, &root.path) {
        return true;
    }
    for child in &root.children {
        if has_matching_fields(child, pattern) {
            return true;
        }
    }
    false
}

pub fn collect_leaf_fields(field: &ParsedField) -> Vec<&ParsedField> {
    let mut result = Vec::new();
    collect_leaves_recursive(field, &mut result);
    result
}

fn collect_leaves_recursive<'a>(field: &'a ParsedField, result: &mut Vec<&'a ParsedField>) {
    if field.children.is_empty() {
        result.push(field);
    } else {
        for child in &field.children {
            collect_leaves_recursive(child, result);
        }
    }
}

pub fn collect_leaf_fields_owned(field: &ParsedField) -> Vec<ParsedField> {
    let mut result = Vec::new();
    collect_leaves_owned_recursive(field, &mut result);
    result
}

fn collect_leaves_owned_recursive(field: &ParsedField, result: &mut Vec<ParsedField>) {
    if field.children.is_empty() {
        result.push(field.clone());
    } else {
        for child in &field.children {
            collect_leaves_owned_recursive(child, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedValue;
    use crate::dsl::DisplayFormat;

    #[test]
    fn test_glob_match_exact() {
        assert!(matches_glob("PNGFile.ihdr.width", "PNGFile.ihdr.width"));
        assert!(!matches_glob("PNGFile.ihdr.width", "PNGFile.ihdr.height"));
    }

    #[test]
    fn test_glob_match_wildcard_name() {
        assert!(matches_glob("*.width", "PNGFile.ihdr.width"));
        assert!(matches_glob("*.width", "root.width"));
        assert!(!matches_glob("*.width", "root.height"));
    }

    #[test]
    fn test_glob_match_prefix() {
        assert!(matches_glob("PNGFile.ihdr.*", "PNGFile.ihdr.width"));
        assert!(matches_glob("PNGFile.ihdr.*", "PNGFile.ihdr.height"));
        assert!(!matches_glob("PNGFile.ihdr.*", "PNGFile.signature"));
    }

    #[test]
    fn test_simple_glob_star() {
        assert!(simple_glob_match("*", "anything"));
        assert!(simple_glob_match("w*", "width"));
        assert!(simple_glob_match("*th", "width"));
    }

    #[test]
    fn test_has_matching_fields() {
        let root = ParsedField {
            name: "root".to_string(),
            path: "root".to_string(),
            offset: 0,
            length: 10,
            value: ParsedValue::U8(0),
            display_format: DisplayFormat::Hex,
            truncated: false,
            undecidable: false,
            skipped: false,
            checksum_result: None,
            children: vec![
                ParsedField {
                    name: "width".to_string(),
                    path: "root.width".to_string(),
                    offset: 0,
                    length: 4,
                    value: ParsedValue::U32(100),
                    display_format: DisplayFormat::Dec,
                    truncated: false,
                    undecidable: false,
                    skipped: false,
                    checksum_result: None,
                    children: Vec::new(),
                },
                ParsedField {
                    name: "height".to_string(),
                    path: "root.height".to_string(),
                    offset: 4,
                    length: 4,
                    value: ParsedValue::U32(200),
                    display_format: DisplayFormat::Dec,
                    truncated: false,
                    undecidable: false,
                    skipped: false,
                    checksum_result: None,
                    children: Vec::new(),
                },
            ],
        };
        assert!(has_matching_fields(&root, "*.width"));
        assert!(!has_matching_fields(&root, "*.depth"));
    }
}
