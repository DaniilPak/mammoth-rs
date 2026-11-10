//! Command-line interface, mirroring mammoth.js's `mammoth` command.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use mammoth_rs::{ImageConverter, Options, image_filename_extension};

const USAGE: &str = "\
Usage: mammoth-rs <input.docx> [output] [options]

Converts a .docx file to HTML (default) or Markdown.
If no output path is given, the result is written to stdout.

Options:
  --output-dir=DIR        Write the output to DIR (named after the input) and
                          save images there as separate files
  --output-format=FORMAT  html (default) or markdown
  --style-map=FILE        Read additional style mappings from FILE
  --pretty                Indent the HTML output
  --id-prefix=PREFIX      Prefix for generated IDs
  --preserve-empty-paragraphs
                          Keep empty paragraphs instead of dropping them
  --external-file-access  Allow reading images linked from outside the document
  --raw-text              Output the raw text only
  -h, --help              Show this help
  -V, --version           Show the version
";

struct Args {
    input: PathBuf,
    output: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    markdown: bool,
    style_map: Option<PathBuf>,
    pretty: bool,
    id_prefix: Option<String>,
    preserve_empty_paragraphs: bool,
    external_file_access: bool,
    raw_text: bool,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut positional = Vec::new();
    let mut args = Args {
        input: PathBuf::new(),
        output: None,
        output_dir: None,
        markdown: false,
        style_map: None,
        pretty: false,
        id_prefix: None,
        preserve_empty_paragraphs: false,
        external_file_access: false,
        raw_text: false,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let (name, inline_value) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => {
                (name.to_string(), Some(value.to_string()))
            }
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            match &inline_value {
                Some(value) => Ok(value.clone()),
                None => iter
                    .next()
                    .ok_or_else(|| format!("{name} requires a value")),
            }
        };
        match name.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("mammoth-rs {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--output-dir" => args.output_dir = Some(PathBuf::from(value(&name)?)),
            "--output-format" => match value(&name)?.as_str() {
                "html" => args.markdown = false,
                "markdown" | "md" => args.markdown = true,
                other => return Err(format!("unknown output format: {other}")),
            },
            "--style-map" => args.style_map = Some(PathBuf::from(value(&name)?)),
            "--pretty" => args.pretty = true,
            "--id-prefix" => args.id_prefix = Some(value(&name)?),
            "--preserve-empty-paragraphs" => args.preserve_empty_paragraphs = true,
            "--external-file-access" => args.external_file_access = true,
            "--raw-text" => args.raw_text = true,
            _ if name.starts_with('-') && name != "-" => {
                return Err(format!("unknown option: {name}"));
            }
            _ => positional.push(PathBuf::from(arg)),
        }
    }
    let mut positional = positional.into_iter();
    args.input = positional
        .next()
        .ok_or_else(|| "missing input file".to_string())?;
    args.output = positional.next();
    if positional.next().is_some() {
        return Err("too many arguments".to_string());
    }
    if args.output.is_some() && args.output_dir.is_some() {
        return Err("use either an output path or --output-dir, not both".to_string());
    }
    Ok(Some(args))
}

fn image_saver(dir: &Path) -> ImageConverter {
    let dir = dir.to_path_buf();
    let counter = Arc::new(AtomicUsize::new(0));
    ImageConverter::img_element(move |image| {
        let number = counter.fetch_add(1, Ordering::SeqCst) + 1;
        let extension = image_filename_extension(image).unwrap_or("bin");
        let filename = format!("{number}.{extension}");
        std::fs::write(dir.join(&filename), image.read()?)?;
        Ok(vec![("src".to_string(), filename)])
    })
}

fn run(args: Args) -> Result<(), String> {
    let mut options = Options::new()
        .pretty_print(args.pretty)
        .ignore_empty_paragraphs(!args.preserve_empty_paragraphs)
        .external_file_access(args.external_file_access);
    if let Some(prefix) = &args.id_prefix {
        options = options.id_prefix(prefix.clone());
    }
    if let Some(path) = &args.style_map {
        let style_map = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read style map {}: {e}", path.display()))?;
        options = options.style_map(&style_map);
    }

    let mut output_path = args.output.clone();
    if let Some(dir) = &args.output_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        let extension = if args.raw_text {
            "txt"
        } else if args.markdown {
            "md"
        } else {
            "html"
        };
        let stem = args
            .input
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output".to_string());
        output_path = Some(dir.join(format!("{stem}.{extension}")));
        options = options.image_converter(image_saver(dir));
    }

    let result = if args.raw_text {
        mammoth_rs::extract_raw_text(&args.input)
    } else if args.markdown {
        mammoth_rs::convert_to_markdown(&args.input, &options)
    } else {
        mammoth_rs::convert_to_html(&args.input, &options)
    }
    .map_err(|e| format!("{}: {e}", args.input.display()))?;

    for message in &result.messages {
        eprintln!("{message}");
    }
    match output_path {
        Some(path) => std::fs::write(&path, result.value)
            .map_err(|e| format!("could not write {}: {e}", path.display())),
        None => std::io::stdout()
            .write_all(result.value.as_bytes())
            .map_err(|e| e.to_string()),
    }
}

fn main() -> ExitCode {
    match parse_args() {
        Ok(None) => ExitCode::SUCCESS,
        Ok(Some(args)) => match run(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("mammoth-rs: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("mammoth-rs: {error}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
