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
            ExitCode::FormatError
        }
    };
    
    process::exit(exit_code.into());
}

fn run(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.command {
        Some(Commands::ListFormats) => {
            list_formats();
            return Ok(ExitCode::Success);
        }
        Some(Commands::ExportExample { format_name, output }) => {
            return export_example(&format_name, output.as_deref());
        }
        _ => {}
    }

    let (file_path, diff_path, format_path, builtin_format, output_format, json_output, no_tui, output_path) = 
        if let Some(Commands::Parse { file, format, builtin_format, output_format, json, no_tui, output }) = &cli.command {
            (Some(file.clone()), None, format.clone(), builtin_format.clone(), output_format.clone(), *json, *no_tui, output.clone())
        } else if let Some(Commands::Diff { file1, file2, format, builtin_format, output_format, output }) = &cli.command {
            (Some(file1.clone()), Some(file2.clone()), format.clone(), builtin_format.clone(), output_format.clone(), false, true, output.clone())
        } else {
            (cli.file.clone(), cli.diff.clone(), cli.format.clone(), cli.builtin_format.clone(), cli.output_format.clone(), cli.json, cli.no_tui, cli.output.clone())
        };

    if let Some(diff_file) = diff_path {
        let file1 = file_path.ok_or("请指定第一个文件路径")?;
        return run_diff(&file1, &diff_file, format_path.as_deref(), builtin_format.as_deref(), output_format, output_path.as_deref());
    }

    let file_path = file_path.ok_or("请指定二进制文件路径")?;
    let data = read_data(&file_path)?;

    let format_def = if let Some(format_path) = format_path {
        let yaml = fs::read_to_string(&format_path)?;
        FormatDefinition::from_yaml(&yaml).map_err(|e| format!("格式定义错误: {}", e))?
    } else if let Some(builtin) = builtin_format {
        parse_builtin_format(&builtin).map_err(|e| format!("内置格式错误: {}", e))?
    } else {
        match detect_format(&data) {
            Some(def) => def,
            None => {
                eprintln!("无法自动检测文件格式，请使用 --format 或 --builtin-format 指定格式定义");
                return Ok(ExitCode::NoMatch);
            }
        }
    };

    let (parsed, has_checksum_failure) = parser::parse(&data, &format_def)?;

    let output_format = if json_output {
        Some(OutputFormat::Json)
    } else {
        output_format
    };

    if let Some(fmt) = output_format {
        let output = match fmt {
            OutputFormat::Json => export::to_json(&parsed),
            OutputFormat::Csv => export::to_csv(&parsed)?,
            OutputFormat::Md => export::to_markdown(&parsed, &format_def.name),
            OutputFormat::Terminal => export::to_terminal_summary(&parsed, &format_def.name, atty::is(atty::Stream::Stdout)),
        };

        if let Some(output_path) = output_path {
            fs::write(&output_path, &output)?;
            println!("结果已写入: {}", output_path.display());
        } else {
            print!("{}", output);
        }
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
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let data1 = read_data(file1)?;
    let data2 = read_data(file2)?;

    let format_def = if let Some(format_path) = format_path {
        let yaml = fs::read_to_string(format_path)?;
        FormatDefinition::from_yaml(&yaml).map_err(|e| format!("格式定义错误: {}", e))?
    } else if let Some(builtin) = builtin_format {
        parse_builtin_format(builtin).map_err(|e| format!("内置格式错误: {}", e))?
    } else {
        match detect_format(&data1) {
            Some(def) => def,
            None => {
                eprintln!("无法自动检测文件格式，请使用 --format 或 --builtin-format 指定格式定义");
                return Ok(ExitCode::NoMatch);
            }
        }
    };

    let (parsed1, _) = parser::parse(&data1, &format_def)?;
    let (parsed2, _) = parser::parse(&data2, &format_def)?;

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
