use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "binparse-cli", version = "0.1.0", about = "通用二进制文件格式解析与结构可视化工具")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(short, long, help = "二进制文件路径")]
    pub file: Option<PathBuf>,

    #[arg(short = 'F', long, help = "格式定义文件路径 (.yaml/.yml/.bfmt)")]
    pub format: Option<PathBuf>,

    #[arg(short = 't', long, help = "使用内置格式 (png/bmp/wav/zip/elf/pe)")]
    pub builtin_format: Option<String>,

    #[arg(long, value_enum, help = "输出格式")]
    pub output_format: Option<OutputFormat>,

    #[arg(long, help = "输出纯JSON到stdout")]
    pub json: bool,

    #[arg(short, long, help = "差异对比模式，指定第二个文件路径")]
    pub diff: Option<PathBuf>,

    #[arg(short, long, help = "禁用TUI，直接输出结果")]
    pub no_tui: bool,

    #[arg(long, help = "输出文件路径")]
    pub output: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "解析二进制文件")]
    Parse {
        #[arg(help = "二进制文件路径")]
        file: PathBuf,

        #[arg(short = 'F', long, help = "格式定义文件路径 (.yaml/.yml/.bfmt)")]
        format: Option<PathBuf>,

        #[arg(short = 't', long, help = "使用内置格式")]
        builtin_format: Option<String>,

        #[arg(long, value_enum, help = "输出格式")]
        output_format: Option<OutputFormat>,

        #[arg(long, help = "输出纯JSON")]
        json: bool,

        #[arg(short, long, help = "禁用TUI")]
        no_tui: bool,

        #[arg(long, help = "输出文件路径")]
        output: Option<PathBuf>,

        #[arg(long, help = "字段路径glob过滤模式 (如 *.width, PNGFile.ihdr.*)")]
        filter: Option<String>,

        #[arg(long, help = "输出解析统计摘要到stderr")]
        stats: bool,
    },
    #[command(about = "差异对比两个二进制文件")]
    Diff {
        #[arg(help = "第一个文件路径")]
        file1: PathBuf,

        #[arg(help = "第二个文件路径")]
        file2: PathBuf,

        #[arg(short = 'F', long, help = "格式定义文件路径 (.yaml/.yml/.bfmt)")]
        format: Option<PathBuf>,

        #[arg(short = 't', long, help = "使用内置格式")]
        builtin_format: Option<String>,

        #[arg(long, value_enum, help = "输出格式")]
        output_format: Option<OutputFormat>,

        #[arg(long, help = "输出文件路径")]
        output: Option<PathBuf>,

        #[arg(long, help = "字段路径glob过滤模式")]
        filter: Option<String>,
    },
    #[command(about = "验证格式定义文件的正确性")]
    Validate {
        #[arg(help = "格式定义文件路径 (.yaml/.yml/.bfmt)")]
        format: PathBuf,
    },
    #[command(about = "列出内置格式")]
    ListFormats,
    #[command(about = "导出自定义格式定义示例")]
    ExportExample {
        #[arg(help = "内置格式名称")]
        format_name: String,

        #[arg(short, long, help = "输出文件路径")]
        output: Option<PathBuf>,
    },
    #[command(about = "修补二进制文件中的指定字段")]
    Patch {
        #[arg(help = "源二进制文件路径")]
        input: PathBuf,

        #[arg(help = "输出文件路径")]
        output: PathBuf,

        #[arg(short = 'F', long, help = "YAML格式定义文件路径")]
        format: Option<PathBuf>,

        #[arg(short = 't', long, help = "使用内置格式 (png/bmp/wav/zip/elf/pe)")]
        builtin_format: Option<String>,

        #[arg(long = "set", help = "设置字段值，格式为\"字段路径=新值[@条件表达式]\"，可多次指定")]
        sets: Vec<String>,

        #[arg(long = "patch-file", help = "批量修改脚本文件路径，每行一条\"字段路径=新值[@条件表达式]\"指令，支持${变量名}模板占位符")]
        patch_file: Option<PathBuf>,

        #[arg(long = "var", help = "模板变量值，格式为\"name=value\"，用于替换patch文件中的${变量名}")]
        vars: Vec<String>,

        #[arg(long, help = "试运行模式，不实际写文件，只输出修改计划")]
        dry_run: bool,

        #[arg(long, help = "撤销最近一次patch操作，从.binpatch_history文件恢复")]
        undo: bool,
    },
    #[command(about = "编译YAML格式定义为二进制.bfmt文件")]
    Compile {
        #[arg(help = "YAML格式定义文件路径")]
        input: PathBuf,

        #[arg(help = "输出.bfmt文件路径")]
        output: PathBuf,

        #[arg(long, help = "包含调试信息（字段在YAML中的行号）")]
        debug: bool,

        #[arg(long, help = "目标格式版本号，默认为当前最新版本")]
        target_version: Option<u16>,

        #[arg(long, help = "启用编译缓存，未变更时跳过编译")]
        cache: bool,

        #[arg(long, help = "优化字符串表，按引用频次降序排列")]
        optimize: bool,
    },
    #[command(about = "反编译.bfmt二进制文件为YAML格式定义")]
    Decompile {
        #[arg(help = ".bfmt文件路径")]
        input: PathBuf,

        #[arg(short, long, help = "输出文件路径，默认输出到stdout")]
        output: Option<PathBuf>,
    },
    #[command(about = "对比两个格式定义文件的结构差异（支持.yaml和.bfmt）")]
    DiffFormat {
        #[arg(help = "第一个格式定义文件路径 (.yaml/.bfmt)")]
        file1: PathBuf,

        #[arg(help = "第二个格式定义文件路径 (.yaml/.bfmt)")]
        file2: PathBuf,
    },
}

#[derive(clap::ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Csv,
    Md,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    ChecksumFailure = 1,
    FormatError = 2,
    NoMatch = 3,
    ValidationError = 4,
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        code as i32
    }
}
