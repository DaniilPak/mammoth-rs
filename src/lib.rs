//! # mammoth-rs
//!
//! Convert `.docx` documents (Microsoft Word, Google Docs, LibreOffice) into
//! clean, semantic HTML or Markdown. A pure-Rust port of
//! [mammoth.js](https://github.com/mwilliamson/mammoth.js).
//!
//! Like mammoth, the conversion is driven by the *styles* used in the
//! document rather than its visual formatting: a paragraph styled
//! `Heading 1` becomes `<h1>`, a custom `Section Title` style can be mapped to
//! whatever HTML you like, and the output stays free of inline CSS.
//!
//! ```no_run
//! use mammoth_rs::{convert_to_html, Options};
//!
//! let options = Options::new()
//!     .style_map("p[style-name='Section Title'] => h1:fresh\n\
//!                 r[style-name='Code'] => code");
//! let result = convert_to_html("document.docx", &options)?;
//! println!("{}", result.value);
//! for message in result.messages {
//!     eprintln!("{message}");
//! }
//! # Ok::<(), mammoth_rs::Error>(())
//! ```
//!
//! See the [README](https://github.com/DaniilPak/mammoth-rs) for the style
//! map syntax and a feature comparison with mammoth.js.

#![warn(missing_docs)]

mod convert;
pub mod document;
mod docx;
mod error;
mod html;
mod images;
mod style_map;
pub mod transforms;
mod xml;

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use document::{Document, Element, Image};
pub use error::{ConversionResult, Error, Message, MessageKind, Result};
pub use images::{BoxError, ImageConverter, image_filename_extension};
pub use style_map::{DEFAULT_STYLE_MAP, read_style_map_lines, validate_style_mapping};

use convert::{Conversion, ConvertSettings};
use docx::PackageFiles;
use html::{MarkdownWriter, PrettyHtmlWriter, SimpleHtmlWriter, Writer};
use style_map::{StyleMapping, parse_style_mapping};

/// The `.docx` document to convert: a path on disk or the file's bytes.
///
/// Usually created implicitly via `From`, e.g. `convert_to_html("a.docx", …)`
/// or `convert_to_html(&bytes[..], …)`.
#[derive(Debug, Clone)]
pub enum Input<'a> {
    /// Read the document from this path. External images (when enabled) are
    /// resolved relative to the document's directory.
    Path(Cow<'a, Path>),
    /// Read the document from these bytes.
    Bytes(Cow<'a, [u8]>),
}

impl<'a> From<&'a Path> for Input<'a> {
    fn from(path: &'a Path) -> Self {
        Input::Path(Cow::Borrowed(path))
    }
}

impl<'a> From<&'a PathBuf> for Input<'a> {
    fn from(path: &'a PathBuf) -> Self {
        Input::Path(Cow::Borrowed(path.as_path()))
    }
}

impl From<PathBuf> for Input<'static> {
    fn from(path: PathBuf) -> Self {
        Input::Path(Cow::Owned(path))
    }
}

impl<'a> From<&'a str> for Input<'a> {
    fn from(path: &'a str) -> Self {
        Input::Path(Cow::Borrowed(Path::new(path)))
    }
}

impl<'a> From<&'a String> for Input<'a> {
    fn from(path: &'a String) -> Self {
        Input::Path(Cow::Borrowed(Path::new(path.as_str())))
    }
}

impl<'a> From<&'a [u8]> for Input<'a> {
    fn from(bytes: &'a [u8]) -> Self {
        Input::Bytes(Cow::Borrowed(bytes))
    }
}

impl<'a, const N: usize> From<&'a [u8; N]> for Input<'a> {
    fn from(bytes: &'a [u8; N]) -> Self {
        Input::Bytes(Cow::Borrowed(&bytes[..]))
    }
}

impl<'a> From<&'a Vec<u8>> for Input<'a> {
    fn from(bytes: &'a Vec<u8>) -> Self {
        Input::Bytes(Cow::Borrowed(bytes.as_slice()))
    }
}

impl From<Vec<u8>> for Input<'static> {
    fn from(bytes: Vec<u8>) -> Self {
        Input::Bytes(Cow::Owned(bytes))
    }
}

impl Input<'_> {
    fn load(&self) -> Result<(Cow<'_, [u8]>, Option<&Path>)> {
        match self {
            Input::Path(path) => Ok((Cow::Owned(std::fs::read(path)?), Some(path.as_ref()))),
            Input::Bytes(bytes) => Ok((Cow::Borrowed(bytes.as_ref()), None)),
        }
    }
}

type TransformFn = dyn Fn(Document) -> Document + Send + Sync;

/// Conversion options. Construct with [`Options::new`] (or `Default`) and
/// the builder methods.
#[derive(Clone)]
pub struct Options {
    style_map: Vec<String>,
    include_default_style_map: bool,
    include_embedded_style_map: bool,
    ignore_empty_paragraphs: bool,
    id_prefix: String,
    pretty_print: bool,
    external_file_access: bool,
    image_converter: ImageConverter,
    transform_document: Option<Arc<TransformFn>>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            style_map: Vec::new(),
            include_default_style_map: true,
            include_embedded_style_map: true,
            ignore_empty_paragraphs: true,
            id_prefix: String::new(),
            pretty_print: false,
            external_file_access: false,
            image_converter: ImageConverter::data_uri(),
            transform_document: None,
        }
    }
}

impl fmt::Debug for Options {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Options")
            .field("style_map", &self.style_map)
            .field("include_default_style_map", &self.include_default_style_map)
            .field(
                "include_embedded_style_map",
                &self.include_embedded_style_map,
            )
            .field("ignore_empty_paragraphs", &self.ignore_empty_paragraphs)
            .field("id_prefix", &self.id_prefix)
            .field("pretty_print", &self.pretty_print)
            .field("external_file_access", &self.external_file_access)
            .field("transform_document", &self.transform_document.is_some())
            .finish_non_exhaustive()
    }
}

impl Options {
    /// Default options, equivalent to mammoth.js's defaults.
    pub fn new() -> Self {
        Options::default()
    }

    /// Adds style mappings, one per line. Blank lines and lines starting
    /// with `#` are ignored. May be called repeatedly; earlier mappings take
    /// precedence over later ones, and all of them over the embedded and
    /// default style maps.
    pub fn style_map(mut self, style_map: &str) -> Self {
        self.style_map.extend(read_style_map_lines(style_map));
        self
    }

    /// Adds a single style mapping, e.g. `p[style-name='Aside'] => aside:fresh`.
    pub fn style_mapping(mut self, mapping: impl Into<String>) -> Self {
        self.style_map.push(mapping.into());
        self
    }

    /// Whether to append mammoth's [default style map](DEFAULT_STYLE_MAP)
    /// (default: `true`).
    pub fn include_default_style_map(mut self, include: bool) -> Self {
        self.include_default_style_map = include;
        self
    }

    /// Whether to use a style map embedded in the document with
    /// [`embed_style_map`] (default: `true`).
    pub fn include_embedded_style_map(mut self, include: bool) -> Self {
        self.include_embedded_style_map = include;
        self
    }

    /// Whether to drop empty paragraphs (default: `true`).
    pub fn ignore_empty_paragraphs(mut self, ignore: bool) -> Self {
        self.ignore_empty_paragraphs = ignore;
        self
    }

    /// Prefix for generated IDs (bookmarks, footnotes, comments), useful when
    /// several documents end up on one page.
    pub fn id_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.id_prefix = prefix.into();
        self
    }

    /// Indent the generated HTML (default: `false`). Ignored for Markdown.
    pub fn pretty_print(mut self, pretty_print: bool) -> Self {
        self.pretty_print = pretty_print;
        self
    }

    /// Allow reading images linked from (rather than embedded in) the
    /// document from the file system (default: `false`).
    ///
    /// Only enable this for trusted documents: a document could otherwise
    /// make the converter read arbitrary files.
    pub fn external_file_access(mut self, allow: bool) -> Self {
        self.external_file_access = allow;
        self
    }

    /// Sets how images are converted. Defaults to [`ImageConverter::data_uri`].
    pub fn image_converter(mut self, converter: ImageConverter) -> Self {
        self.image_converter = converter;
        self
    }

    /// Transforms the document model before it is converted, for instance
    /// with [`transforms::paragraph`].
    pub fn transform_document<F>(mut self, transform: F) -> Self
    where
        F: Fn(Document) -> Document + Send + Sync + 'static,
    {
        self.transform_document = Some(Arc::new(transform));
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Html,
    Markdown,
}

/// Converts a `.docx` document to HTML.
///
/// ```no_run
/// let result = mammoth_rs::convert_to_html("report.docx", &mammoth_rs::Options::new())?;
/// std::fs::write("report.html", result.value)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn convert_to_html<'a>(
    input: impl Into<Input<'a>>,
    options: &Options,
) -> Result<ConversionResult> {
    convert(input.into(), options, OutputFormat::Html)
}

/// Converts a `.docx` document to Markdown.
///
/// As with mammoth.js, Markdown support is more limited than HTML support:
/// formatting without a Markdown equivalent is dropped.
pub fn convert_to_markdown<'a>(
    input: impl Into<Input<'a>>,
    options: &Options,
) -> Result<ConversionResult> {
    convert(input.into(), options, OutputFormat::Markdown)
}

/// Extracts the raw text of a document, ignoring all formatting. Each
/// paragraph is followed by two newlines.
pub fn extract_raw_text<'a>(input: impl Into<Input<'a>>) -> Result<ConversionResult> {
    let input = input.into();
    let (bytes, path) = input.load()?;
    let files = PackageFiles::open(&bytes, false, path)?;
    let (document, messages) = docx::read(&files)?;
    let mut value = String::new();
    for element in &document.children {
        transforms::raw_text(element, &mut value);
    }
    Ok(ConversionResult {
        value,
        messages: error::dedupe_messages(messages),
    })
}

/// Reads the style map embedded in a document by [`embed_style_map`], if any.
pub fn read_embedded_style_map<'a>(input: impl Into<Input<'a>>) -> Result<Option<String>> {
    let input = input.into();
    let (bytes, path) = input.load()?;
    let files = PackageFiles::open(&bytes, false, path)?;
    docx::read_embedded_style_map(&files)
}

/// Returns a copy of the document with `style_map` embedded in it. The
/// embedded style map is used by later conversions unless disabled with
/// [`Options::include_embedded_style_map`].
pub fn embed_style_map<'a>(input: impl Into<Input<'a>>, style_map: &str) -> Result<Vec<u8>> {
    let input = input.into();
    let (bytes, _) = input.load()?;
    docx::embed_style_map(&bytes, style_map)
}

/// Reads a document into the [`Document`] model without converting it.
pub fn read_document<'a>(
    input: impl Into<Input<'a>>,
    options: &Options,
) -> Result<(Document, Vec<Message>)> {
    let input = input.into();
    let (bytes, path) = input.load()?;
    let files = PackageFiles::open(&bytes, options.external_file_access, path)?;
    let (document, messages) = docx::read(&files)?;
    Ok((document, error::dedupe_messages(messages)))
}

fn convert(input: Input<'_>, options: &Options, format: OutputFormat) -> Result<ConversionResult> {
    let (bytes, path) = input.load()?;
    let files = PackageFiles::open(&bytes, options.external_file_access, path)?;
    let embedded_style_map = if options.include_embedded_style_map {
        docx::read_embedded_style_map(&files)?
    } else {
        None
    };
    let (document, mut messages) = docx::read(&files)?;
    let document = match &options.transform_document {
        Some(transform) => transform(document),
        None => document,
    };

    let mut lines: Vec<Cow<'_, str>> = options
        .style_map
        .iter()
        .map(|s| Cow::Borrowed(s.as_str()))
        .collect();
    if let Some(embedded) = &embedded_style_map {
        lines.extend(read_style_map_lines(embedded).into_iter().map(Cow::Owned));
    }
    if options.include_default_style_map {
        lines.extend(DEFAULT_STYLE_MAP.iter().map(|s| Cow::Borrowed(*s)));
    }
    let mut style_map: Vec<StyleMapping> = Vec::with_capacity(lines.len());
    for line in &lines {
        match parse_style_mapping(line) {
            Ok(mapping) => style_map.push(mapping),
            Err(warning) => messages.push(Message::warning(warning)),
        }
    }

    let value = convert_document(&document, &style_map, options, format, &mut messages);
    Ok(ConversionResult {
        value,
        messages: error::dedupe_messages(messages),
    })
}

fn convert_document(
    document: &Document,
    style_map: &[StyleMapping],
    options: &Options,
    format: OutputFormat,
    messages: &mut Vec<Message>,
) -> String {
    let settings = ConvertSettings {
        style_map,
        id_prefix: &options.id_prefix,
        ignore_empty_paragraphs: options.ignore_empty_paragraphs,
        image_converter: &options.image_converter,
    };
    let (nodes, conversion_messages) =
        Conversion::new(settings, document).convert_document(document);
    messages.extend(conversion_messages);
    let nodes = html::simplify(nodes);
    let mut writer: Box<dyn Writer> = match format {
        OutputFormat::Markdown => Box::<MarkdownWriter>::default(),
        OutputFormat::Html if options.pretty_print => Box::<PrettyHtmlWriter>::default(),
        OutputFormat::Html => Box::<SimpleHtmlWriter>::default(),
    };
    html::write(writer.as_mut(), &nodes);
    writer.finish()
}
