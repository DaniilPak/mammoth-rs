//! The intermediate document model.
//!
//! A `.docx` file is first read into a [`Document`], which is then converted
//! to HTML. The model mirrors the one used by mammoth.js, and can be
//! inspected or modified with [`Options::transform_document`](crate::Options::transform_document)
//! before conversion.

use std::sync::Arc;

/// A whole document: its body plus footnotes, endnotes and comments.
#[derive(Debug, Clone, Default)]
pub struct Document {
    /// Top-level elements of the document body.
    pub children: Vec<Element>,
    /// Footnotes and endnotes, referenced from [`Element::NoteReference`].
    pub notes: Vec<Note>,
    /// Comments, referenced from [`Element::CommentReference`].
    pub comments: Vec<Comment>,
}

impl Document {
    /// Finds the note referenced by `note_type` and `note_id`.
    pub fn find_note(&self, note_type: NoteType, note_id: &str) -> Option<&Note> {
        self.notes
            .iter()
            .find(|note| note.note_type == note_type && note.note_id == note_id)
    }

    /// Finds the comment with the given ID.
    pub fn find_comment(&self, comment_id: &str) -> Option<&Comment> {
        self.comments
            .iter()
            .find(|comment| comment.comment_id == comment_id)
    }
}

/// A node in the document tree.
#[derive(Debug, Clone)]
pub enum Element {
    /// A paragraph (`w:p`).
    Paragraph(Paragraph),
    /// A run of text sharing the same formatting (`w:r`).
    Run(Run),
    /// Literal text.
    Text(String),
    /// A tab character.
    Tab,
    /// A checkbox (form field or content control).
    Checkbox {
        /// Whether the checkbox is ticked.
        checked: bool,
    },
    /// A hyperlink, either external or to a bookmark in the document.
    Hyperlink(Hyperlink),
    /// A reference to a footnote or endnote.
    NoteReference(NoteReference),
    /// A reference to a comment.
    CommentReference {
        /// ID of the referenced comment.
        comment_id: String,
    },
    /// An image.
    Image(Image),
    /// A table.
    Table(Table),
    /// A table row.
    TableRow(TableRow),
    /// A table cell.
    TableCell(TableCell),
    /// A line, page or column break.
    Break(BreakType),
    /// The start of a bookmark, used as a target for internal hyperlinks.
    BookmarkStart {
        /// Name of the bookmark.
        name: String,
    },
}

impl Element {
    /// Child elements, if this element can have children.
    pub fn children(&self) -> Option<&Vec<Element>> {
        match self {
            Element::Paragraph(p) => Some(&p.children),
            Element::Run(r) => Some(&r.children),
            Element::Hyperlink(h) => Some(&h.children),
            Element::Table(t) => Some(&t.children),
            Element::TableRow(r) => Some(&r.children),
            Element::TableCell(c) => Some(&c.children),
            _ => None,
        }
    }

    /// Mutable access to child elements, if this element can have children.
    pub fn children_mut(&mut self) -> Option<&mut Vec<Element>> {
        match self {
            Element::Paragraph(p) => Some(&mut p.children),
            Element::Run(r) => Some(&mut r.children),
            Element::Hyperlink(h) => Some(&mut h.children),
            Element::Table(t) => Some(&mut t.children),
            Element::TableRow(r) => Some(&mut r.children),
            Element::TableCell(c) => Some(&mut c.children),
            _ => None,
        }
    }
}

/// A paragraph.
#[derive(Debug, Clone, Default)]
pub struct Paragraph {
    /// Contents of the paragraph.
    pub children: Vec<Element>,
    /// ID of the paragraph style (`w:pStyle`), e.g. `Heading1`.
    pub style_id: Option<String>,
    /// Human readable name of the paragraph style, e.g. `Heading 1`.
    pub style_name: Option<String>,
    /// List numbering, if the paragraph is part of a list.
    pub numbering: Option<NumberingLevel>,
    /// Paragraph alignment (`w:jc`), e.g. `center`.
    pub alignment: Option<String>,
    /// Paragraph indentation.
    pub indent: ParagraphIndent,
}

/// Paragraph indentation, as raw values from the document (twentieths of a point).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParagraphIndent {
    /// Start (left in left-to-right text) indentation.
    pub start: Option<String>,
    /// End (right in left-to-right text) indentation.
    pub end: Option<String>,
    /// First line indentation.
    pub first_line: Option<String>,
    /// Hanging indentation.
    pub hanging: Option<String>,
}

/// The list level of a numbered or bulleted paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberingLevel {
    /// Zero-based nesting level (`w:ilvl`).
    pub level: String,
    /// `true` for numbered lists, `false` for bulleted lists.
    pub is_ordered: bool,
}

impl NumberingLevel {
    /// The nesting level as a number, if it is a valid integer.
    pub fn level_index(&self) -> Option<u32> {
        self.level.trim().parse().ok()
    }
}

/// A run of text with shared formatting.
#[derive(Debug, Clone, Default)]
pub struct Run {
    /// Contents of the run.
    pub children: Vec<Element>,
    /// ID of the character style (`w:rStyle`).
    pub style_id: Option<String>,
    /// Human readable name of the character style.
    pub style_name: Option<String>,
    /// Bold (`w:b`).
    pub is_bold: bool,
    /// Underlined (`w:u`).
    pub is_underline: bool,
    /// Italic (`w:i`).
    pub is_italic: bool,
    /// Struck through (`w:strike`).
    pub is_strikethrough: bool,
    /// All capitals (`w:caps`).
    pub is_all_caps: bool,
    /// Small capitals (`w:smallCaps`).
    pub is_small_caps: bool,
    /// Superscript, subscript or baseline.
    pub vertical_alignment: VerticalAlignment,
    /// Font name (`w:rFonts/@w:ascii`).
    pub font: Option<String>,
    /// Font size in points.
    pub font_size: Option<f64>,
    /// Highlight colour, e.g. `yellow`.
    pub highlight: Option<String>,
}

/// Vertical alignment of a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerticalAlignment {
    /// Normal text.
    #[default]
    Baseline,
    /// Superscript.
    Superscript,
    /// Subscript.
    Subscript,
}

/// A hyperlink.
#[derive(Debug, Clone, Default)]
pub struct Hyperlink {
    /// Contents of the link.
    pub children: Vec<Element>,
    /// External target.
    pub href: Option<String>,
    /// Internal target (bookmark name).
    pub anchor: Option<String>,
    /// Target frame, e.g. `_blank`.
    pub target_frame: Option<String>,
}

/// Whether a note is a footnote or an endnote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteType {
    /// A footnote.
    Footnote,
    /// An endnote.
    Endnote,
}

impl NoteType {
    /// `"footnote"` or `"endnote"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            NoteType::Footnote => "footnote",
            NoteType::Endnote => "endnote",
        }
    }
}

/// A reference to a note from the document body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteReference {
    /// Footnote or endnote.
    pub note_type: NoteType,
    /// ID of the note.
    pub note_id: String,
}

/// A footnote or endnote.
#[derive(Debug, Clone)]
pub struct Note {
    /// Footnote or endnote.
    pub note_type: NoteType,
    /// ID of the note.
    pub note_id: String,
    /// Contents of the note.
    pub body: Vec<Element>,
}

/// A comment.
#[derive(Debug, Clone)]
pub struct Comment {
    /// ID of the comment.
    pub comment_id: String,
    /// Contents of the comment.
    pub body: Vec<Element>,
    /// Author name, if present.
    pub author_name: Option<String>,
    /// Author initials, if present.
    pub author_initials: Option<String>,
}

/// An image embedded in (or linked from) the document.
#[derive(Debug, Clone)]
pub struct Image {
    /// Alternative text (`descr`, falling back to `title`).
    pub alt_text: Option<String>,
    /// MIME type, e.g. `image/png`, if known.
    pub content_type: Option<String>,
    data: Arc<std::result::Result<Vec<u8>, String>>,
}

impl Image {
    /// Creates an image from its bytes.
    pub fn new(alt_text: Option<String>, content_type: Option<String>, data: Vec<u8>) -> Self {
        Image {
            alt_text,
            content_type,
            data: Arc::new(Ok(data)),
        }
    }

    pub(crate) fn from_result(
        alt_text: Option<String>,
        content_type: Option<String>,
        data: std::result::Result<Vec<u8>, String>,
    ) -> Self {
        Image {
            alt_text,
            content_type,
            data: Arc::new(data),
        }
    }

    /// The raw bytes of the image.
    ///
    /// Fails if the image could not be read, for instance because it is
    /// stored outside the document and external file access is disabled.
    pub fn read(&self) -> std::result::Result<&[u8], ImageReadError> {
        match self.data.as_ref() {
            Ok(data) => Ok(data),
            Err(message) => Err(ImageReadError(message.clone())),
        }
    }

    /// The image bytes encoded as standard base64.
    pub fn read_as_base64(&self) -> std::result::Result<String, ImageReadError> {
        use base64::Engine;
        self.read()
            .map(|data| base64::engine::general_purpose::STANDARD.encode(data))
    }
}

/// Error returned by [`Image::read`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageReadError(pub String);

impl std::fmt::Display for ImageReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImageReadError {}

/// A table.
#[derive(Debug, Clone, Default)]
pub struct Table {
    /// Rows of the table (normally [`Element::TableRow`]).
    pub children: Vec<Element>,
    /// ID of the table style.
    pub style_id: Option<String>,
    /// Human readable name of the table style.
    pub style_name: Option<String>,
}

/// A table row.
#[derive(Debug, Clone, Default)]
pub struct TableRow {
    /// Cells of the row (normally [`Element::TableCell`]).
    pub children: Vec<Element>,
    /// Whether the row repeats as a header row (`w:tblHeader`).
    pub is_header: bool,
}

/// A table cell.
#[derive(Debug, Clone)]
pub struct TableCell {
    /// Contents of the cell.
    pub children: Vec<Element>,
    /// Number of columns spanned.
    pub col_span: u32,
    /// Number of rows spanned.
    pub row_span: u32,
    pub(crate) v_merge: Option<bool>,
}

impl TableCell {
    /// Creates a cell spanning a single row and column.
    pub fn new(children: Vec<Element>) -> Self {
        TableCell {
            children,
            col_span: 1,
            row_span: 1,
            v_merge: None,
        }
    }

    /// Sets the column span.
    pub fn with_col_span(mut self, col_span: u32) -> Self {
        self.col_span = col_span;
        self
    }

    /// Sets the row span.
    pub fn with_row_span(mut self, row_span: u32) -> Self {
        self.row_span = row_span;
        self
    }
}

impl Default for TableCell {
    fn default() -> Self {
        TableCell::new(Vec::new())
    }
}

/// The kind of a [`Element::Break`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakType {
    /// A line break (`<w:br/>`).
    Line,
    /// A page break.
    Page,
    /// A column break.
    Column,
}
