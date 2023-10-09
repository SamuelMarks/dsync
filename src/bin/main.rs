use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use dsync::{error::IOErrorToError, GenerationConfig, TableOptions};
use dsync::{FileChangeStatus, StringType};
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

#[derive(Debug, Parser, Clone, PartialEq)]
#[command(author, version, about, long_about = None)]
#[command(bin_name("dsync"))]
#[command(disable_help_subcommand(true))] // Disable subcommand "help", only "-h" or "--help" should be used
#[command(subcommand_negates_reqs(true))]
#[command(infer_subcommands(true))]
pub struct CliDerive {
    // use extra struct, otherwise clap subcommands require all options
    #[clap(flatten)]
    pub args: Option<MainOptions>,

    #[command(subcommand)]
    pub subcommands: Option<SubCommands>,
}

#[derive(Debug, Subcommand, Clone, PartialEq)]
pub enum SubCommands {
    /// Generate shell completions
    Completions(CommandCompletions),
}

#[derive(Debug, Parser, Clone, PartialEq)]
pub struct CommandCompletions {
    /// Set which shell completions should be generated
    /// Supported are: Bash, Elvish, Fish, PowerShell, Zsh
    #[arg(short = 's', long = "shell", value_enum)]
    pub shell: Shell,
    /// Output path where to output the completions to
    /// Not specifying this will print to STDOUT
    #[arg(short = 'o', long = "out")]
    pub output_file_path: Option<PathBuf>,
}

#[derive(Debug, Parser, Clone, PartialEq)]
pub struct MainOptions {
    /// Input diesel schema file
    #[arg(short = 'i', long = "input")]
    pub input: PathBuf,

    /// Output file, stdout if not present
    #[arg(short = 'o', long = "output")]
    pub output: PathBuf,

    /// adds the #[tsync] attribute to all structs; see https://github.com/Wulf/tsync
    #[arg(long = "tsync")]
    #[cfg(feature = "tsync")]
    pub tsync: bool,

    /// uses diesel_async for generated functions; see https://github.com/weiznich/diesel_async
    #[arg(long = "async")]
    #[cfg(feature = "async")]
    pub use_async: bool,

    /// List of columns which are automatically generated but are not primary keys (for example: "created_at", "updated_at", etc.)
    #[arg(short = 'g', long = "autogenerated-columns")]
    pub autogenerated_columns: Option<Vec<String>>,

    /// rust type which describes a connection, for example: "diesel::r2d2::PooledConnection<diesel::r2d2::ConnectionManager<diesel::PgConnection>>"
    #[arg(short = 'c', long = "connection-type")]
    pub connection_type: String,

    /// Disable generating serde implementations
    #[arg(long = "no-serde")]
    pub no_serde: bool,

    /// Set custom schema use path
    #[arg(long = "schema-path", default_value = "crate::schema::")]
    pub schema_path: String,

    /// Set custom model use path
    #[arg(long = "model-path", default_value = "crate::models::")]
    pub model_path: String,

    /// Do not generate the CRUD (impl) functions for generated models
    #[arg(long = "no-crud")]
    pub no_crud: bool,

    /// Set which string type to use for Create* structs
    #[arg(long = "create-str", default_value = "string")]
    pub create_str: StringTypeCli,

    /// Set which string type to use for Update* structs
    #[arg(long = "update-str", default_value = "string")]
    pub update_str: StringTypeCli,

    /// Only Generate a single model file instead of a directory with "mod.rs" and "generated.rs"
    #[arg(long = "single-model-file")]
    pub single_model_file: bool,

    /// Generate common structs only once in a "common.rs" file
    #[arg(long = "once-common-structs")]
    pub once_common_structs: bool,

    /// Generate the "ConnectionType" type only once in a "common.rs" file
    #[arg(long = "once-connection-type")]
    pub once_connection_type: bool,

    /// A Prefix to treat a table matching this as readonly (only generate the Read struct)
    #[arg(long = "readonly-prefix")]
    pub readonly_prefixes: Vec<String>,

    /// A Suffix to treat a table matching this as readonly (only generate the Read struct)
    #[arg(long = "readonly-suffix")]
    pub readonly_suffixes: Vec<String>,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Default)]
pub enum StringTypeCli {
    /// Use "String"
    #[default]
    String,
    /// Use "&str"
    Str,
    /// Use "Cow<str>"
    Cow,
}

impl From<StringTypeCli> for StringType {
    fn from(value: StringTypeCli) -> Self {
        match value {
            StringTypeCli::String => StringType::String,
            StringTypeCli::Str => StringType::Str,
            StringTypeCli::Cow => StringType::Cow,
        }
    }
}

fn main() {
    let res = actual_main();

    if let Err(err) = res {
        eprintln!("Error:\n{err}");
        #[cfg(feature = "backtrace")]
        {
            let backtrace = err.backtrace().to_string();

            if backtrace == "disabled backtrace" {
                eprintln!(
                    "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace"
                );
            } else {
                eprintln!("{}", backtrace);
            }
        }
        #[cfg(not(feature = "backtrace"))]
        {
            eprintln!("backtrace support is disabled, enable feature \"backtrace\"");
        }

        std::process::exit(1);
    }
}

fn actual_main() -> dsync::Result<()> {
    let cli = CliDerive::parse();

    if let Some(subcommand) = cli.subcommands {
        return match subcommand {
            SubCommands::Completions(subcommand) => command_completions(&subcommand),
        };
    }

    let args = cli
        .args
        .expect("cli.args should be defined if no subcommand is given");

    let cols = args.autogenerated_columns.unwrap_or_default();
    let mut default_table_options = TableOptions::default()
        .autogenerated_columns(cols.iter().map(|t| t.as_str()).collect::<Vec<&str>>())
        .create_str_type(args.create_str.into())
        .update_str_type(args.update_str.into());

    #[cfg(feature = "tsync")]
    if args.tsync {
        default_table_options = default_table_options.tsync();
    }

    #[cfg(feature = "async")]
    if args.use_async {
        default_table_options = default_table_options.use_async();
    }

    if args.no_serde {
        default_table_options = default_table_options.disable_serde();
    }

    if args.no_crud {
        default_table_options = default_table_options.disable_fns();
    }

    if args.single_model_file {
        default_table_options = default_table_options.single_model_file();
    }

    let changes = dsync::generate_files(
        &args.input,
        &args.output,
        GenerationConfig {
            default_table_options,
            table_options: HashMap::from([]),
            connection_type: args.connection_type,
            schema_path: args.schema_path,
            model_path: args.model_path,
            once_common_structs: args.once_common_structs,
            once_connection_type: args.once_connection_type,
            readonly_prefixes: args.readonly_prefixes,
            readonly_suffixes: args.readonly_suffixes,
        },
    )?;

    let mut modified: usize = 0;

    for change in changes {
        println!("{} {}", change.status, change.file.to_string_lossy());
        if change.status != FileChangeStatus::Unchanged {
            modified += 1;
        }
    }

    println!("Modified {} files", modified);

    Ok(())
}

/// Handler function for the "completions" subcommand
/// This function is mainly to keep the code structured and sorted
#[inline]
pub fn command_completions(sub_args: &CommandCompletions) -> dsync::Result<()> {
    // if there is a output file path, use that path, otherwise use stdout
    let mut writer: BufWriter<Box<dyn Write>> = match &sub_args.output_file_path {
        Some(v) => {
            if v.exists() {
                return Err(dsync::Error::other("Output file already exists"));
            }
            let v_parent = v
                .parent()
                .expect("Expected input filename to have a parent");
            std::fs::create_dir_all(v_parent).attach_path_err(v_parent)?;
            BufWriter::new(Box::from(std::fs::File::create(v).attach_path_err(v)?))
        }
        None => BufWriter::new(Box::from(std::io::stdout())),
    };
    let mut parsed = CliDerive::command();
    let bin_name = parsed
        .get_bin_name()
        .expect("Expected binary to have a binary name")
        .to_string();
    generate(sub_args.shell, &mut parsed, bin_name, &mut writer);

    Ok(())
}
