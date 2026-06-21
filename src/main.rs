use binparse_cli::*;
use clap::Parser;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process;

fn main() {
    let cli = Cli::parse();

    let exit_code = match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::FormatError as i32
        }
    };

    process::exit(exit_code);
}

fn run(cli: Cli) -> Result<i32, Box<dyn std::error::Error>> {
    match &cli.command {
        Some(Commands::ListFormats) => {
            list_formats();
            return Ok(ExitCode::Success as i32);
        }
        Some(Commands::ExportExample { format_name, output }) => {
            return Ok(export_example(format_name, output.as_deref())? as i32);
        }
        Some(Commands::Validate { format: format_path }) => {
            return Ok(run_validate(format_path)? as i32);
        }
        Some(Commands::Patch {
            input,
            output,
            format,
            builtin_format,
            sets,
            patch_file,
            vars,
            dry_run,
            undo,
        }) => {
            return run_patch_cmd(input, output, format.as_deref(), builtin_format.as_deref(), sets, patch_file.as_deref(), vars, *dry_run, *undo);
        }
        Some(Commands::Compile { input, output, debug }) => {
            return Ok(run_compile(input, output, *debug)? as i32);
        }
        Some(Commands::Decompile { input, output }) => {
            return Ok(run_decompile(input, output.as_deref())? as i32);
        }
        _ => {}
    }

    if let Some(Commands::Parse {
        file,
        format,
        builtin_format,
        output_format,
        json,
        no_tui,
        output,
        filter,
        stats,
    }) = &cli.command
    {
        return Ok(run_parse(file, format.as_deref(), builtin_format.as_deref(), output_format.clone(), *json, *no_tui, output.as_deref(), filter.as_deref(), *stats)? as i32);
    }

    if let Some(Commands::Diff {
        file1,
        file2,
        format,
        builtin_format,
        output_format,
        output,
        filter,
    }) = &cli.command
    {
        return Ok(run_diff(file1, file2, format.as_deref(), builtin_format.as_deref(), output_format.clone(), output.as_deref(), filter.as_deref())? as i32);
    }

    let diff_path = cli.diff.clone();
    let format_path = cli.format.clone();
    let builtin_format = cli.builtin_format.clone();
    let output_format = cli.output_format.clone();
    let json_output = cli.json;
    let no_tui = cli.no_tui;
    let output_path = cli.output.clone();

    if let Some(diff_file) = diff_path {
        let file1 = cli.file.ok_or("请指定第一个文件路径")?;
        return Ok(run_diff(&file1, &diff_file, format_path.as_deref(), builtin_format.as_deref(), output_format, output_path.as_deref(), None)? as i32);
    }

    let file_path = cli.file.ok_or("请指定二进制文件路径")?;
    let data = read_data(&file_path)?;

    let format_def = load_format_def(format_path.as_deref(), builtin_format.as_deref(), &data)?;

    let (parsed, has_checksum_failure) = parser::parse(&data, &format_def)?;

    let output_format = if json_output {
        Some(OutputFormat::Json)
    } else {
        output_format
    };

    if let Some(fmt) = output_format {
        let output = format_output(&parsed, &format_def.name, fmt);
        write_output(&output, output_path.as_deref())?;
    } else if no_tui || !atty::is(atty::Stream::Stdout) {
        let output = export::to_terminal_summary(&parsed, &format_def.name, false);
        print!("{}", output);
    } else {
        println!("正在启动TUI界面... (按 q 退出)");
        tui::run_tui(data, parsed, format_def.name.clone())?;
    }

    Ok(if has_checksum_failure {
        ExitCode::ChecksumFailure as i32
    } else {
        ExitCode::Success as i32
    })
}

fn run_parse(
    file: &Path,
    format_path: Option<&Path>,
    builtin_format: Option<&str>,
    output_format: Option<OutputFormat>,
    json_output: bool,
    no_tui: bool,
    output_path: Option<&Path>,
    filter_pattern: Option<&str>,
    show_stats: bool,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let data = read_data(file)?;
    let format_def = load_format_def(format_path, builtin_format, &data)?;

    let (parsed, has_checksum_failure) = parser::parse(&data, &format_def)?;

    let parsed = if let Some(pattern) = filter_pattern {
        if !filter::has_matching_fields(&parsed, pattern) {
            println!("无匹配字段");
            return Ok(ExitCode::Success);
        }
        filter::filter_parsed_field(&parsed, pattern).unwrap_or(parsed)
    } else {
        parsed
    };

    if show_stats {
        let stats = stats::ParseStats::from_parsed_field(&parsed, data.len());
        eprintln!("{}", stats.format_to_stderr());
    }

    let output_format = if json_output {
        Some(OutputFormat::Json)
    } else {
        output_format
    };

    if let Some(fmt) = output_format {
        let output = format_output(&parsed, &format_def.name, fmt);
        write_output(&output, output_path)?;
    } else if no_tui || !atty::is(atty::Stream::Stdout) {
        let output = export::to_terminal_summary(&parsed, &format_def.name, false);
        print!("{}", output);
    } else {
        println!("正在启动TUI界面... (按 q 退出)");
        tui::run_tui(data, parsed, format_def.name.clone())?;
    }

    Ok(if has_checksum_failure {
        ExitCode::ChecksumFailure
    } else {
        ExitCode::Success
    })
}

fn run_diff(
    file1: &Path,
    file2: &Path,
    format_path: Option<&Path>,
    builtin_format: Option<&str>,
    output_format: Option<OutputFormat>,
    output_path: Option<&Path>,
    filter_pattern: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let data1 = read_data(file1)?;
    let data2 = read_data(file2)?;

    let format_def = load_format_def(format_path, builtin_format, &data1)?;

    let (parsed1, _) = parser::parse(&data1, &format_def)?;
    let (parsed2, _) = parser::parse(&data2, &format_def)?;

    let parsed1 = if let Some(pattern) = filter_pattern {
        if !filter::has_matching_fields(&parsed1, pattern) {
            println!("无匹配字段");
            return Ok(ExitCode::Success);
        }
        filter::filter_parsed_field(&parsed1, pattern).unwrap_or(parsed1)
    } else {
        parsed1
    };
    let parsed2 = if let Some(pattern) = filter_pattern {
        if !filter::has_matching_fields(&parsed2, pattern) {
            println!("无匹配字段");
            return Ok(ExitCode::Success);
        }
        filter::filter_parsed_field(&parsed2, pattern).unwrap_or(parsed2)
    } else {
        parsed2
    };

    let diff_result = diff::diff(&parsed1, &parsed2);

    let output = match output_format.unwrap_or(OutputFormat::Md) {
        OutputFormat::Json => serde_json::to_string_pretty(&diff_result)?,
        OutputFormat::Csv => {
            let mut wtr = csv::Writer::from_writer(Vec::new());
            wtr.write_record(&["path", "offset", "length", "file1_value", "file2_value", "is_different", "truncated", "undecidable", "skipped"])?;
            write_diff_csv_rows(&diff_result.fields, &mut wtr)?;
            wtr.flush()?;
            String::from_utf8_lossy(&wtr.into_inner()?).to_string()
        }
        OutputFormat::Md | OutputFormat::Terminal => {
            let mut md = String::new();
            md.push_str(&format!("# 差异对比报告\n\n"));
            md.push_str(&format!("**文件1:** {}\n\n", file1.display()));
            md.push_str(&format!("**文件2:** {}\n\n", file2.display()));
            md.push_str(&format!("**格式:** {}\n\n", format_def.name));
            md.push_str(&format!("## 统计信息\n\n"));
            md.push_str(&format!("| 指标 | 数值 |\n"));
            md.push_str(&format!("|------|------|\n"));
            md.push_str(&format!("| 总字段数 | {} |\n", diff_result.total_fields));
            md.push_str(&format!("| 差异字段数 | {} |\n", diff_result.different_fields));
            md.push_str(&format!("| 差异率 | {:.2}% |\n\n", diff_result.diff_rate * 100.0));

            if diff_result.different_fields > 0 {
                md.push_str("## 差异详情\n\n");
                md.push_str("| 字段路径 | 偏移 | 长度 | 文件1值 | 文件2值 | 状态 |\n");
                md.push_str("|----------|------|------|---------|---------|------|\n");
                write_diff_markdown_rows(&diff_result.fields, &mut md);
            } else {
                md.push_str("## 结果\n\n");
                md.push_str("两个文件完全相同，没有发现差异。\n");
            }

            md
        }
    };

    if let Some(output_path) = output_path {
        fs::write(output_path, &output)?;
        println!("差异报告已写入: {}", output_path.display());
    } else {
        print!("{}", output);
    }

    println!("\n=== 差异统计 ===");
    println!("总字段数: {}", diff_result.total_fields);
    println!("差异字段数: {}", diff_result.different_fields);
    println!("差异率: {:.2}%", diff_result.diff_rate * 100.0);

    Ok(ExitCode::Success)
}

fn run_validate(format_path: &Path) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let def = if let Some(ext) = format_path.extension().and_then(|e| e.to_str()) {
        if ext == "bfmt" {
            let mut file = fs::File::open(format_path)?;
            match load_from_bfmt(&mut file) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("二进制格式定义加载错误: {}", e);
                    return Ok(ExitCode::FormatError);
                }
            }
        } else {
            let yaml = fs::read_to_string(format_path)
                .map_err(|e| format!("无法读取格式定义文件: {}", e))?;
            match FormatDefinition::from_yaml_unvalidated(&yaml) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("格式定义错误: {}", e);
                    return Ok(ExitCode::FormatError);
                }
            }
        }
    } else {
        let yaml = fs::read_to_string(format_path)
            .map_err(|e| format!("无法读取格式定义文件: {}", e))?;
        match FormatDefinition::from_yaml_unvalidated(&yaml) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("格式定义错误: {}", e);
                return Ok(ExitCode::FormatError);
            }
        }
    };

    let errors = validate::validate_format_definition(&def);

    if errors.is_empty() {
        println!("格式定义有效");
        Ok(ExitCode::Success)
    } else {
        for err in &errors {
            eprintln!("错误 [{}]: {}", err.location, err.reason);
        }
        Ok(ExitCode::FormatError)
    }
}

fn load_format_def(
    format_path: Option<&Path>,
    builtin_format: Option<&str>,
    data: &[u8],
) -> Result<FormatDefinition, Box<dyn std::error::Error>> {
    if let Some(fp) = format_path {
        if let Some(ext) = fp.extension().and_then(|e| e.to_str()) {
            if ext == "bfmt" {
                let mut file = fs::File::open(fp)?;
                return Ok(load_from_bfmt(&mut file)
                    .map_err(|e| format!("二进制格式定义加载错误: {}", e))?);
            }
        }
        let yaml = fs::read_to_string(fp)?;
        Ok(FormatDefinition::from_yaml(&yaml).map_err(|e| format!("格式定义错误: {}", e))?)
    } else if let Some(builtin) = builtin_format {
        Ok(parse_builtin_format(builtin).map_err(|e| format!("内置格式错误: {}", e))?)
    } else {
        match detect_format(data) {
            Some(def) => Ok(def),
            None => {
                eprintln!("无法自动检测文件格式，请使用 --format 或 --builtin-format 指定格式定义");
                Err("无法自动检测文件格式".into())
            }
        }
    }
}

fn run_compile(
    input: &Path,
    output: &Path,
    include_debug: bool,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let yaml = match fs::read_to_string(input) {
        Ok(y) => y,
        Err(e) => {
            eprintln!("读取输入文件失败: {}", e);
            return Ok(ExitCode::FormatError);
        }
    };
    let def = match FormatDefinition::from_yaml(&yaml) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("YAML解析错误: {}", e);
            return Ok(ExitCode::FormatError);
        }
    };
    let mut file = fs::File::create(output)?;
    let size = compile_to_bfmt(&def, &mut file, include_debug)?;
    let struct_count = def.structs.len() + 1;
    let enum_count = def.enums.len();
    let field_count = count_fields(&def);
    println!("编译成功!");
    println!("  输出文件: {}", output.display());
    println!("  文件大小: {} 字节", size);
    println!("  结构体: {} 个", struct_count);
    println!("  枚举: {} 个", enum_count);
    println!("  字段: {} 个", field_count);
    Ok(ExitCode::Success)
}

fn run_decompile(
    input: &Path,
    output: Option<&Path>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut file = fs::File::open(input)?;
    let def = load_from_bfmt(&mut file)
        .map_err(|e| format!("二进制格式定义加载错误: {}", e))?;
    let yaml = decompile_to_yaml(&def)?;
    if let Some(output_path) = output {
        fs::write(output_path, &yaml)?;
        println!("反编译成功，输出文件: {}", output_path.display());
    } else {
        print!("{}", yaml);
    }
    Ok(ExitCode::Success)
}

fn format_output(parsed: &parser::ParsedField, format_name: &str, fmt: OutputFormat) -> String {
    match fmt {
        OutputFormat::Json => export::to_json(parsed),
        OutputFormat::Csv => export::to_csv(parsed).unwrap_or_default(),
        OutputFormat::Md => export::to_markdown(parsed, format_name),
        OutputFormat::Terminal => export::to_terminal_summary(parsed, format_name, atty::is(atty::Stream::Stdout)),
    }
}

fn write_output(output: &str, output_path: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = output_path {
        fs::write(path, output)?;
        println!("结果已写入: {}", path.display());
    } else {
        print!("{}", output);
    }
    Ok(())
}

fn write_diff_csv_rows(
    fields: &[diff::DiffField],
    wtr: &mut csv::Writer<Vec<u8>>,
) -> Result<(), Box<dyn std::error::Error>> {
    for field in fields {
        wtr.write_record(&[
            field.path.clone(),
            format!("0x{:08X}", field.offset),
            field.length.to_string(),
            field.value1.clone(),
            field.value2.clone(),
            field.is_different.to_string(),
            field.truncated.to_string(),
            field.undecidable.to_string(),
            field.skipped.to_string(),
        ])?;
        write_diff_csv_rows(&field.children, wtr)?;
    }
    Ok(())
}

fn write_diff_markdown_rows(fields: &[diff::DiffField], md: &mut String) {
    for field in fields {
        if field.is_different {
            let status = if field.truncated {
                "截断"
            } else if field.undecidable {
                "不可判定"
            } else if field.skipped {
                "跳过"
            } else {
                "不同"
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
        write_diff_markdown_rows(&field.children, md);
    }
}

fn read_data(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if path.to_str() == Some("-") {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        Ok(buf)
    } else {
        Ok(fs::read(path)?)
    }
}

fn list_formats() {
    println!("可用的内置格式:");
    let formats = get_builtin_formats();
    let mut names: Vec<&str> = formats.keys().cloned().collect();
    names.sort();
    for name in names {
        println!("  - {}", name);
    }
}

fn export_example(format_name: &str, output: Option<&Path>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let yaml = get_builtin_format(format_name)
        .ok_or_else(|| format!("未知的内置格式: {}", format_name))?;

    if let Some(output_path) = output {
        fs::write(output_path, yaml)?;
        println!("格式定义已导出到: {}", output_path.display());
    } else {
        print!("{}", yaml);
    }

    Ok(ExitCode::Success)
}

fn run_patch_cmd(
    input: &Path,
    output: &Path,
    format_path: Option<&Path>,
    builtin_format: Option<&str>,
    sets: &[String],
    patch_file: Option<&Path>,
    vars: &[String],
    dry_run: bool,
    undo: bool,
) -> Result<i32, Box<dyn std::error::Error>> {
    let input_data = read_data(input)?;

    let format_def = match load_format_def(format_path, builtin_format, &input_data) {
        Ok(def) => def,
        Err(e) => {
            eprintln!("Error: {}", e);
            return Ok(patch::PatchError::FORMAT_ERROR);
        }
    };

    if undo {
        return match patch::undo_last_patch(output, &format_def) {
            Ok((changes, exit_code)) => {
                println!("=== 撤销成功 ===");
                println!("\n已恢复以下字段 (共{}项):", changes.len());
                for change in &changes {
                    println!("\n  偏移: 0x{:08X} ({}字节)", change.offset, change.length);
                    println!("  恢复前: {}", change.original_value_display);
                    println!("  恢复后: {}", change.new_value_display);
                }
                println!("\n输出文件: {}", output.display());
                Ok(exit_code)
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                Ok(patch::PatchError::FORMAT_ERROR)
            }
        };
    }

    if sets.is_empty() && patch_file.is_none() {
        return Err("请至少指定一个--set参数或--patch-file参数".into());
    }

    let instructions = match patch::parse_patch_instructions(sets, patch_file, vars) {
        Ok(inst) => inst,
        Err(e) => {
            let err_str = e.to_string();
            eprintln!("Error: {}", e);
            if err_str.contains("未定义的模板变量") {
                return Ok(patch::PatchError::UNDEFINED_VARIABLE);
            }
            return Ok(patch::PatchError::FORMAT_ERROR);
        }
    };

    if instructions.is_empty() {
        return Err("没有有效的修改指令".into());
    }

    let (result, exit_code) = match patch::run_patch(&input_data, output, &format_def, &instructions, dry_run) {
        Ok(r) => r,
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("字段路径不存在") {
                eprintln!("Error: {}", e);
                return Ok(patch::PatchError::FIELD_NOT_FOUND);
            } else if err_str.contains("编码失败") || err_str.contains("值编码") || err_str.contains("超出范围") || err_str.contains("解析失败") || err_str.contains("未定义的模板变量") {
                eprintln!("Error: {}", e);
                if err_str.contains("未定义的模板变量") {
                    return Ok(patch::PatchError::UNDEFINED_VARIABLE);
                }
                return Ok(patch::PatchError::VALUE_ENCODING_ERROR);
            } else {
                eprintln!("Error: {}", e);
                return Ok(patch::PatchError::FORMAT_ERROR);
            }
        }
    };

    if dry_run {
        println!("=== DRY-RUN: 计划修改 (未实际写入) ===");
    } else {
        println!("=== 修改摘要 ===");
    }

    println!("\n字段修改 (共{}项):", result.changes.len());
    for change in &result.changes {
        println!("\n  字段: {}", change.field_path);
        println!("  偏移: 0x{:08X} ({}字节)", change.offset, change.length);
        println!("  原值: {}", change.original_value_display);
        println!("  新值: {}", change.new_value_display);
        let orig_hex: String = change.original_bytes.iter().map(|b| format!("{:02X}", b)).collect();
        let new_hex: String = change.new_bytes.iter().map(|b| format!("{:02X}", b)).collect();
        println!("  原始字节: {}", orig_hex);
        println!("  新字节:   {}", new_hex);
    }

    if !result.skipped.is_empty() {
        println!("\n跳过的修改 (共{}项):", result.skipped.len());
        for skip in &result.skipped {
            println!("  - {}: {}", skip.field_path, skip.reason);
        }
    }

    if !result.offset_warnings.is_empty() {
        eprintln!("\n=== 偏移量警告 ===");
        for warning in &result.offset_warnings {
            eprintln!("  注意: 字段{}的偏移表达式引用了被修改的字段{}，实际偏移可能需要手动调整",
                warning.dependent_field, warning.modified_field);
        }
    }

    if !result.checksum_recalcs.is_empty() {
        println!("\n校验和重算 (共{}项):", result.checksum_recalcs.len());
        for cs in &result.checksum_recalcs {
            println!("\n  字段: {}", cs.field_path);
            println!("  算法: {}", cs.algorithm);
            println!("  覆盖范围: 0x{:08X} - 0x{:08X}", cs.start, cs.end);
            println!("  原校验值: 0x{:08X}", cs.original_value);
            println!("  新校验值: 0x{:08X}", cs.new_value);
        }
    } else if dry_run {
        println!("\n校验和重算: 无需要重算的校验和");
    }

    if !dry_run {
        if !result.changes.is_empty() {
            if let Err(e) = patch::write_patch_history(output, &result.changes) {
                eprintln!("\n警告: 无法写入patch历史文件: {}", e);
            }
        }

        if !result.validation_failures.is_empty() {
            eprintln!("\n=== 警告: 验证失败 ===");
            for (path, expected, actual) in &result.validation_failures {
                eprintln!("  字段 {}: 期望值={}, 实际解析值={}", path, expected, actual);
            }
        } else if !result.changes.is_empty() {
            println!("\n验证通过: 所有修改字段已正确写入并重新解析确认。");
        }

        println!("\n输出文件: {}", output.display());
    }

    Ok(exit_code)
}
