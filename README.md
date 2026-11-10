# mammoth-rs

[![CI](https://github.com/DaniilPak/mammoth-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/DaniilPak/mammoth-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/mammoth-rs.svg)](https://crates.io/crates/mammoth-rs)
[![docs.rs](https://docs.rs/mammoth-rs/badge.svg)](https://docs.rs/mammoth-rs)

Convert `.docx` documents (Microsoft Word, Google Docs, LibreOffice, Apple Pages)
into **clean, semantic HTML** or Markdown. A pure-Rust port of
[mammoth.js](https://github.com/mwilliamson/mammoth.js) by Michael Williamson.

mammoth-rs looks at the *styles* in a document, not its visual formatting. A
paragraph styled `Heading 1` becomes `<h1>`, a `Section Title` style can be
mapped to whatever you like, and the output has no inline CSS or font soup. It
is meant for turning documents into content for a website or CMS, not for
reproducing their exact look.

- No C dependencies, no LibreOffice, no network: just `zip` + `quick-xml`.
- Same style-map syntax, default style map, and warning messages as mammoth.js.
- Library API plus an optional `mammoth-rs` command-line tool.

## Installation

```toml
[dependencies]
mammoth-rs = "0.1"
```

For the command-line tool:

```bash
cargo install mammoth-rs --features cli
```

## Usage

```rust
use mammoth_rs::{convert_to_html, Options};

fn main() -> Result<(), mammoth_rs::Error> {
    let result = convert_to_html("document.docx", &Options::new())?;
    println!("{}", result.value); // the generated HTML
    for message in &result.messages {
        eprintln!("{message}"); // e.g. warning: Unrecognised paragraph style: ...
    }
    Ok(())
}
```

The input can be a path (`&str`, `&Path`, `PathBuf`) or the file's bytes
(`&[u8]`, `Vec<u8>`):

```rust
let bytes = std::fs::read("document.docx")?;
let result = mammoth_rs::convert_to_html(&bytes, &mammoth_rs::Options::new())?;
```

Other entry points:

| Function | Does |
| --- | --- |
| `convert_to_html(input, &options)` | `.docx` → HTML |
| `convert_to_markdown(input, &options)` | `.docx` → Markdown |
| `extract_raw_text(input)` | Plain text, paragraphs separated by blank lines |
| `read_document(input, &options)` | The parsed [`Document`] model, without converting it |
| `embed_style_map(input, style_map)` | A copy of the `.docx` with a style map stored inside it |
| `read_embedded_style_map(input)` | The style map stored by `embed_style_map`, if any |
| `validate_style_mapping(line)` | Checks one style mapping, returning mammoth's error text |

All conversions return a `ConversionResult { value, messages }`. `messages`
holds warnings (unrecognised styles, unsupported elements, …) and errors (for
example an image that could not be read). A file that cannot be opened at all
returns `Err(mammoth_rs::Error)`.

## Style maps

A style map is a list of rules `document matcher => html path`, one per line.
Your rules are checked first, then any style map embedded in the document,
then the [default style map](#default-style-map).

```rust
use mammoth_rs::{convert_to_html, Options};

let options = Options::new().style_map(
    "p[style-name='Section Title'] => h1:fresh
     p[style-name='Subsection Title'] => h2:fresh
     p[style-name='Aside'] => aside > p:fresh
     r[style-name='Code'] => code
     table[style-name='Pricing'] => table.pricing:fresh
     u => em
     # comments and blank lines are ignored
     p[style-name='Draft Note'] => !",
);
let result = convert_to_html("document.docx", &options)?;
```

**Document matchers**

| Matcher | Matches |
| --- | --- |
| `p`, `r`, `table` | Any paragraph, run or table |
| `p.Heading1` | Style ID `Heading1` |
| `p[style-name='Heading 1']` | Style name (case-insensitive) |
| `p[style-name^='Heading']` | Style name prefix |
| `p:ordered-list(1)`, `p:unordered-list(2)` | List paragraphs at a nesting level |
| `b`, `i`, `u`, `strike`, `all-caps`, `small-caps` | Run formatting |
| `highlight`, `highlight[color='yellow']` | Highlighted runs |
| `br[type='line']`, `br[type='page']`, `br[type='column']` | Breaks |
| `comment-reference` | Comment references (comments are dropped unless mapped) |

**HTML paths**

| Path | Produces |
| --- | --- |
| `h1` | `<h1>`; consecutive matches are merged into one element |
| `h1:fresh` | A new `<h1>` for each match |
| `ul > li:fresh` | Nested elements |
| `ul\|ol > li` | Reuse an existing `ul` *or* `ol`, otherwise create a `ul` |
| `p.warning.big` | Classes |
| `a[target='_blank']` | Attributes |
| `pre:separator('\n')` | When merging consecutive matches, join them with the separator |
| `!` | Drop the element and its contents |
| (empty) | Keep the contents, without a wrapping element |

### Default style map

The default style map (`mammoth_rs::DEFAULT_STYLE_MAP`) is mammoth.js's:
`Heading 1`–`Heading 6` (by ID or name) → `h1`–`h6`, Apple Pages `Heading` and
`Body` styles, nested `ul`/`ol` lists five levels deep, `Strong` →
`strong`, footnote/endnote/comment text styles → `p`, and their reference styles
dropped. Bold, italic, strikethrough, superscript and subscript map to
`strong`, `em`, `s`, `sup` and `sub`; underline, caps and highlights are ignored
unless mapped. Disable it with `Options::include_default_style_map(false)`.

## Options

```rust
use mammoth_rs::{ImageConverter, Options};

let options = Options::new()
    .style_map("p[style-name='Quote'] => blockquote:fresh")
    .id_prefix("doc-1-")             // prefix for footnote/bookmark/comment IDs
    .ignore_empty_paragraphs(false)  // keep <p></p> for empty paragraphs
    .pretty_print(true)              // indent the HTML
    .include_embedded_style_map(false)
    .external_file_access(true)      // allow images linked from outside the .docx
    .image_converter(ImageConverter::img_element(|image| {
        // Store the image somewhere and return the <img> attributes.
        let bytes = image.read()?;
        let name = format!("{:x}.png", bytes.len());
        std::fs::write(&name, bytes)?;
        Ok(vec![("src".into(), name)])
    }))
    .transform_document(mammoth_rs::transforms::paragraph(|mut paragraph| {
        // Treat centred, unstyled paragraphs as headings.
        if paragraph.alignment.as_deref() == Some("center") && paragraph.style_id.is_none() {
            paragraph.style_id = Some("Heading2".into());
        }
        paragraph
    }));
```

By default images are inlined as `data:` URIs. Only enable
`external_file_access` for trusted documents: a document can point at any file.

## Command-line tool

```bash
mammoth-rs document.docx output.html
mammoth-rs document.docx --output-format=markdown > output.md
mammoth-rs document.docx --output-dir=out --style-map=custom-style-map
```

`--output-dir` writes `out/document.html` and saves images as separate files
(`1.png`, `2.jpeg`, …) next to it. Run `mammoth-rs --help` for all options.

## Feature parity with mammoth.js

Compared against mammoth.js 1.13.0.

| Feature | mammoth.js | mammoth-rs |
| --- | :---: | :---: |
| Paragraphs, headings, default style map | ✅ | ✅ |
| Custom style maps (full matcher and HTML path syntax) | ✅ | ✅ |
| Style map warnings with character positions | ✅ | ✅ |
| Embedded style maps (read and write) | ✅ | ✅ |
| Bold, italic, underline, strikethrough, all caps, small caps, highlight | ✅ | ✅ |
| Superscript and subscript | ✅ | ✅ |
| Hyperlinks (external, internal anchors, target frame, `HYPERLINK` fields) | ✅ | ✅ |
| Bookmarks | ✅ | ✅ |
| Numbered and bulleted lists, nesting, list styles, `numStyleLink` | ✅ | ✅ |
| Tables, header rows (`thead`), `colspan`, `rowspan`, table style mapping | ✅ | ✅ |
| Line, page and column breaks (mappable) | ✅ | ✅ |
| Footnotes and endnotes | ✅ | ✅ |
| Comments (opt-in via `comment-reference`) | ✅ | ✅ |
| Images: DrawingML and VML, alt text, `data:` URIs, linked images | ✅ | ✅ |
| Custom image converter (`imgElement`) | ✅ | ✅ |
| Image converter returning arbitrary HTML | ✅ | ❌ (only `<img>` attributes) |
| Text boxes | ✅ | ✅ |
| Checkboxes (form fields and content controls) | ✅ | ✅ |
| Symbols in Symbol/Wingdings/Webdings fonts (`w:sym`) | ✅ | ✅ |
| Tracked changes (insertions kept, deletions dropped) | ✅ | ✅ |
| Strict OOXML documents | ✅ | ✅ |
| `messages` (warnings and errors) | ✅ | ✅ |
| `transformDocument` and `transforms.paragraph`/`run` | ✅ | ✅ |
| `idPrefix`, `ignoreEmptyParagraphs`, `prettyPrint` | ✅ | ✅ |
| Markdown output | ✅ (deprecated) | ✅ |
| `extractRawText` | ✅ | ✅ |
| Command-line tool | ✅ | ✅ (`--features cli`) |
| Async/Promise API | ✅ | — (synchronous; wrap in `spawn_blocking` if needed) |
| Browser bundle | ✅ | — (not tested on `wasm32`) |

Small, intentional differences:

- VML image alt text (`o:title`) is read; mammoth.js maps that namespace incorrectly and never sees it.
- `..` segments in image paths are resolved.
- Whitespace around `=>` and `>` in style mappings is optional (mammoth.js requires it).
- A hyperlink whose relationship is missing is output as plain text instead of `href="undefined"`.
- `pretty_print` indents every line of multi-line text, not only the first.

## Credits and license

mammoth-rs is a port of [mammoth.js](https://github.com/mwilliamson/mammoth.js)
by Michael Williamson. The conversion logic, default style map, style-map
syntax and messages follow mammoth.js closely, and the test documents in
`tests/fixtures/mammoth-js/` are taken from its test suite. The symbol table in
`src/docx/dingbats.rs` is generated from
[dingbat-to-unicode](https://github.com/mwilliamson/dingbat-to-unicode), also
by Michael Williamson. Both are BSD-2-Clause licensed; see
[`LICENSE-BSD-2-Clause-mammoth`](LICENSE-BSD-2-Clause-mammoth).

New code in this repository is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option, with the ported parts also subject to the BSD-2-Clause notice
above.

[`Document`]: https://docs.rs/mammoth-rs/latest/mammoth_rs/document/struct.Document.html
