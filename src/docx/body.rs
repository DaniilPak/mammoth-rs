//! Converts WordprocessingML body XML (`w:p`, `w:r`, `w:tbl`, …) into the
//! document model. A port of mammoth.js's `docx/body-reader.js`.

use std::borrow::Cow;

use super::FileSource;
use super::dingbats;
use super::parts::{ContentTypes, Numbering, Relationships, Styles};
use crate::document::{
    BreakType, Element, Hyperlink, Image, NoteReference, NoteType, NumberingLevel, Paragraph,
    ParagraphIndent, Run, Table, TableCell, TableRow, VerticalAlignment,
};
use crate::error::Message;
use crate::xml::{EMPTY, XmlElement, XmlNode};

pub(crate) struct BodyContext<'a> {
    pub relationships: &'a Relationships,
    pub content_types: &'a ContentTypes,
    pub files: &'a (dyn FileSource + 'a),
    pub numbering: &'a Numbering,
    pub styles: &'a Styles,
}

/// The result of reading some XML: the elements read, "extra" elements that
/// should be placed after the enclosing paragraph (such as text boxes), and
/// any messages.
#[derive(Default)]
pub(crate) struct ReadResult {
    pub elements: Vec<Element>,
    pub extra: Vec<Element>,
    pub messages: Vec<Message>,
}

impl ReadResult {
    fn element(element: Element) -> Self {
        ReadResult {
            elements: vec![element],
            ..Default::default()
        }
    }

    fn warning(message: impl Into<String>) -> Self {
        ReadResult {
            messages: vec![Message::warning(message)],
            ..Default::default()
        }
    }

    fn append(&mut self, other: ReadResult) {
        self.elements.extend(other.elements);
        self.extra.extend(other.extra);
        self.messages.extend(other.messages);
    }
}

#[derive(Clone)]
struct HyperlinkTarget {
    href: Option<String>,
    anchor: Option<String>,
}

enum ComplexField {
    Begin(XmlElement),
    Hyperlink(HyperlinkTarget),
    Checkbox(bool),
    Unknown,
}

pub(crate) struct BodyReader<'a> {
    context: &'a BodyContext<'a>,
    complex_field_stack: Vec<ComplexField>,
    current_instr_text: Vec<String>,
    deleted_paragraph_contents: Vec<XmlNode>,
}

const IGNORED_ELEMENTS: &[&str] = &[
    "office-word:wrap",
    "v:shadow",
    "v:shapetype",
    "w:annotationRef",
    "w:bookmarkEnd",
    "w:sectPr",
    "w:proofErr",
    "w:lastRenderedPageBreak",
    "w:commentRangeStart",
    "w:commentRangeEnd",
    "w:del",
    "w:footnoteRef",
    "w:endnoteRef",
    "w:moveFrom",
    "w:pPr",
    "w:rPr",
    "w:tblPr",
    "w:tblGrid",
    "w:trPr",
    "w:tcPr",
];

const PASS_THROUGH_ELEMENTS: &[&str] = &[
    "w:customXml",
    "w:ins",
    "w:moveFromRangeEnd",
    "w:moveFromRangeStart",
    "w:moveTo",
    "w:moveToRangeEnd",
    "w:moveToRangeStart",
    "w:object",
    "w:smartTag",
    "w:drawing",
    "v:roundrect",
    "v:shape",
    "v:textbox",
    "w:txbxContent",
    "v:group",
    "v:rect",
];

const SUPPORTED_IMAGE_TYPES: &[&str] = &[
    "image/png",
    "image/gif",
    "image/jpeg",
    "image/svg+xml",
    "image/tiff",
];

fn read_boolean_element(element: Option<&XmlElement>) -> bool {
    match element {
        Some(element) => read_boolean_attribute_value(element.attr("w:val")),
        None => false,
    }
}

fn read_boolean_attribute_value(value: Option<&str>) -> bool {
    !matches!(value, Some("false") | Some("0"))
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value.filter(|v| !v.is_empty()).map(str::to_string)
}

fn is_blank(value: Option<&str>) -> bool {
    value.is_none_or(|v| v.trim().is_empty())
}

impl<'a> BodyReader<'a> {
    pub fn new(context: &'a BodyContext<'a>) -> Self {
        BodyReader {
            context,
            complex_field_stack: Vec::new(),
            current_instr_text: Vec::new(),
            deleted_paragraph_contents: Vec::new(),
        }
    }

    pub fn read_xml_elements(&mut self, nodes: &[XmlNode]) -> ReadResult {
        let mut result = ReadResult::default();
        for node in nodes {
            if let XmlNode::Element(element) = node {
                result.append(self.read_xml_element(element));
            }
        }
        result
    }

    fn read_child_elements(&mut self, element: &XmlElement) -> ReadResult {
        self.read_xml_elements(&element.children)
    }

    fn read_xml_element(&mut self, element: &XmlElement) -> ReadResult {
        let name = element.name.as_str();
        match name {
            "w:p" => self.read_paragraph(element),
            "w:r" => self.read_run(element),
            "w:fldChar" => self.read_fld_char(element),
            "w:instrText" => {
                self.current_instr_text.push(element.text());
                ReadResult::default()
            }
            "w:t" => ReadResult::element(Element::Text(element.text())),
            "w:tab" => ReadResult::element(Element::Tab),
            "w:noBreakHyphen" => ReadResult::element(Element::Text("\u{2011}".to_string())),
            "w:softHyphen" => ReadResult::element(Element::Text("\u{00AD}".to_string())),
            "w:sym" => read_symbol(element),
            "w:hyperlink" => self.read_hyperlink(element),
            "w:tbl" => self.read_table(element),
            "w:tr" => self.read_table_row(element),
            "w:tc" => self.read_table_cell(element),
            "w:footnoteReference" => note_reference(element, NoteType::Footnote),
            "w:endnoteReference" => note_reference(element, NoteType::Endnote),
            "w:commentReference" => ReadResult::element(Element::CommentReference {
                comment_id: element.attr("w:id").unwrap_or_default().to_string(),
            }),
            "w:br" => match element.attr("w:type") {
                None | Some("textWrapping") => ReadResult::element(Element::Break(BreakType::Line)),
                Some("page") => ReadResult::element(Element::Break(BreakType::Page)),
                Some("column") => ReadResult::element(Element::Break(BreakType::Column)),
                Some(other) => ReadResult::warning(format!("Unsupported break type: {other}")),
            },
            "w:bookmarkStart" => {
                let name = element.attr("w:name").unwrap_or_default();
                if name == "_GoBack" {
                    ReadResult::default()
                } else {
                    ReadResult::element(Element::BookmarkStart {
                        name: name.to_string(),
                    })
                }
            }
            "mc:AlternateContent" => {
                self.read_child_elements(element.first_or_empty("mc:Fallback"))
            }
            "w:sdt" => self.read_sdt(element),
            "w:pict" => {
                let result = self.read_child_elements(element);
                let mut extra = result.extra;
                extra.extend(result.elements);
                ReadResult {
                    elements: Vec::new(),
                    extra,
                    messages: result.messages,
                }
            }
            "wp:inline" | "wp:anchor" => self.read_drawing_element(element),
            "v:imagedata" => self.read_image_data(element),
            _ if PASS_THROUGH_ELEMENTS.contains(&name) => self.read_child_elements(element),
            _ if IGNORED_ELEMENTS.contains(&name) => ReadResult::default(),
            _ => ReadResult::warning(format!("An unrecognised element was ignored: {name}")),
        }
    }

    // -- Paragraphs and runs ------------------------------------------------

    fn read_paragraph(&mut self, element: &XmlElement) -> ReadResult {
        let properties_element = element.first_or_empty("w:pPr");
        let is_deleted = properties_element
            .first_or_empty("w:rPr")
            .first("w:del")
            .is_some();

        if is_deleted {
            // A deleted paragraph mark merges its contents into the next paragraph.
            self.deleted_paragraph_contents
                .extend(element.children.iter().cloned());
            return ReadResult::default();
        }

        let children_xml: Cow<'_, [XmlNode]> = if self.deleted_paragraph_contents.is_empty() {
            Cow::Borrowed(&element.children)
        } else {
            let mut nodes = std::mem::take(&mut self.deleted_paragraph_contents);
            nodes.extend(element.children.iter().cloned());
            Cow::Owned(nodes)
        };

        let (mut paragraph, mut messages) = self.read_paragraph_properties(properties_element);
        let children = self.read_xml_elements(&children_xml);
        messages.extend(children.messages);
        paragraph.children = children.elements;

        let mut elements = vec![Element::Paragraph(paragraph)];
        elements.extend(children.extra);
        ReadResult {
            elements,
            extra: Vec::new(),
            messages,
        }
    }

    fn read_paragraph_properties(&self, element: &XmlElement) -> (Paragraph, Vec<Message>) {
        let mut messages = Vec::new();
        let (style_id, style_name) = self.read_style(
            element,
            "w:pStyle",
            "Paragraph",
            |id| self.context.styles.find_paragraph_style(id),
            &mut messages,
        );
        let indent = element.first_or_empty("w:ind");
        let first_of =
            |a: &str, b: &str| non_empty(indent.attr(a)).or_else(|| non_empty(indent.attr(b)));
        let paragraph = Paragraph {
            children: Vec::new(),
            numbering: read_numbering_properties(
                style_id.as_deref(),
                element.first_or_empty("w:numPr"),
                self.context.numbering,
            ),
            style_id,
            style_name,
            alignment: non_empty(element.first_or_empty("w:jc").attr("w:val")),
            indent: ParagraphIndent {
                start: first_of("w:start", "w:left"),
                end: first_of("w:end", "w:right"),
                first_line: non_empty(indent.attr("w:firstLine")),
                hanging: non_empty(indent.attr("w:hanging")),
            },
        };
        (paragraph, messages)
    }

    fn read_run(&mut self, element: &XmlElement) -> ReadResult {
        let (mut run, mut messages) = self.read_run_properties(element.first_or_empty("w:rPr"));
        let children = self.read_xml_elements(&element.children);
        messages.extend(children.messages);
        let mut child_elements = children.elements;
        if let Some(target) = self.current_hyperlink_target() {
            child_elements = vec![Element::Hyperlink(Hyperlink {
                children: child_elements,
                href: target.href,
                anchor: target.anchor,
                target_frame: None,
            })];
        }
        run.children = child_elements;
        ReadResult {
            elements: vec![Element::Run(run)],
            extra: children.extra,
            messages,
        }
    }

    fn read_run_properties(&self, element: &XmlElement) -> (Run, Vec<Message>) {
        let mut messages = Vec::new();
        let (style_id, style_name) = self.read_style(
            element,
            "w:rStyle",
            "Run",
            |id| self.context.styles.find_character_style(id),
            &mut messages,
        );
        let font_size = element
            .first_or_empty("w:sz")
            .attr("w:val")
            .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|half_points| half_points / 2.0);
        let vertical_alignment = match element.first_or_empty("w:vertAlign").attr("w:val") {
            Some("superscript") => VerticalAlignment::Superscript,
            Some("subscript") => VerticalAlignment::Subscript,
            _ => VerticalAlignment::Baseline,
        };
        let run = Run {
            children: Vec::new(),
            style_id,
            style_name,
            is_bold: read_boolean_element(element.first("w:b")),
            is_underline: read_underline(element.first("w:u")),
            is_italic: read_boolean_element(element.first("w:i")),
            is_strikethrough: read_boolean_element(element.first("w:strike")),
            is_all_caps: read_boolean_element(element.first("w:caps")),
            is_small_caps: read_boolean_element(element.first("w:smallCaps")),
            vertical_alignment,
            font: non_empty(element.first_or_empty("w:rFonts").attr("w:ascii")),
            font_size,
            highlight: element
                .first_or_empty("w:highlight")
                .attr("w:val")
                .filter(|v| !v.is_empty() && *v != "none")
                .map(str::to_string),
        };
        (run, messages)
    }

    fn read_style<'s>(
        &self,
        element: &XmlElement,
        tag_name: &str,
        style_type: &str,
        find: impl Fn(&str) -> Option<&'s super::parts::Style>,
        messages: &mut Vec<Message>,
    ) -> (Option<String>, Option<String>) {
        let Some(style_element) = element.first(tag_name) else {
            return (None, None);
        };
        let Some(style_id) = non_empty(style_element.attr("w:val")) else {
            return (None, None);
        };
        match find(&style_id) {
            Some(style) => {
                let name = style.name.clone();
                (Some(style_id), name)
            }
            None => {
                messages.push(Message::warning(format!(
                    "{style_type} style with ID {style_id} was referenced but not defined in the document"
                )));
                (Some(style_id), None)
            }
        }
    }

    // -- Complex fields (hyperlinks and checkboxes in field codes) ----------

    fn read_fld_char(&mut self, element: &XmlElement) -> ReadResult {
        match element.attr("w:fldCharType") {
            Some("begin") => {
                self.complex_field_stack
                    .push(ComplexField::Begin(element.clone()));
                self.current_instr_text.clear();
                ReadResult::default()
            }
            Some("end") => {
                let Some(field) = self.complex_field_stack.pop() else {
                    return ReadResult::warning(
                        "Ignoring complex field end character without corresponding start character",
                    );
                };
                let field = match field {
                    ComplexField::Begin(fld_char) => self.parse_current_instr_text(&fld_char),
                    other => other,
                };
                match field {
                    ComplexField::Checkbox(checked) => {
                        ReadResult::element(Element::Checkbox { checked })
                    }
                    _ => ReadResult::default(),
                }
            }
            Some("separate") => {
                let Some(field) = self.complex_field_stack.pop() else {
                    return ReadResult::warning(
                        "Ignoring complex field separator character without corresponding start character",
                    );
                };
                let parsed = match &field {
                    ComplexField::Begin(fld_char) => self.parse_current_instr_text(fld_char),
                    _ => self.parse_current_instr_text(&EMPTY),
                };
                self.complex_field_stack.push(parsed);
                ReadResult::default()
            }
            _ => ReadResult::default(),
        }
    }

    fn current_hyperlink_target(&self) -> Option<HyperlinkTarget> {
        self.complex_field_stack
            .iter()
            .rev()
            .find_map(|field| match field {
                ComplexField::Hyperlink(target) => Some(target.clone()),
                _ => None,
            })
    }

    fn parse_current_instr_text(&self, fld_char: &XmlElement) -> ComplexField {
        parse_instr_text(&self.current_instr_text.concat(), fld_char)
    }

    // -- Hyperlinks --------------------------------------------------------

    fn read_hyperlink(&mut self, element: &XmlElement) -> ReadResult {
        let relationship_id = non_empty(element.attr("r:id"));
        let anchor = non_empty(element.attr("w:anchor"));
        let target_frame = non_empty(element.attr("w:tgtFrame"));
        let mut result = self.read_child_elements(element);

        let link = match (relationship_id, anchor) {
            (Some(id), anchor) => self
                .context
                .relationships
                .find_target_by_id(&id)
                .map(|href| {
                    let href = match &anchor {
                        Some(anchor) => replace_fragment(href, anchor),
                        None => href.to_string(),
                    };
                    (Some(href), None)
                }),
            (None, Some(anchor)) => Some((None, Some(anchor))),
            (None, None) => None,
        };

        if let Some((href, anchor)) = link {
            let children = std::mem::take(&mut result.elements);
            result.elements = vec![Element::Hyperlink(Hyperlink {
                children,
                href,
                anchor,
                target_frame,
            })];
        }
        result
    }

    // -- Structured document tags -----------------------------------------

    fn read_sdt(&mut self, element: &XmlElement) -> ReadResult {
        let mut result = self.read_child_elements(element.first_or_empty("w:sdtContent"));
        let Some(checkbox) = element.first_or_empty("w:sdtPr").first("wordml:checkbox") else {
            return result;
        };
        let checked = checkbox
            .first("wordml:checked")
            .is_some_and(|checked| read_boolean_attribute_value(checked.attr("wordml:val")));

        let mut has_checkbox = false;
        replace_first_text_with_checkbox(&mut result.elements, checked, &mut has_checkbox);
        if !has_checkbox {
            result.elements = vec![Element::Checkbox { checked }];
        }
        result
    }

    // -- Tables -------------------------------------------------------------

    fn read_table(&mut self, element: &XmlElement) -> ReadResult {
        let mut style_messages = Vec::new();
        let (style_id, style_name) = self.read_style(
            element.first_or_empty("w:tblPr"),
            "w:tblStyle",
            "Table",
            |id| self.context.styles.find_table_style(id),
            &mut style_messages,
        );
        let mut result = self.read_child_elements(element);
        let rows = std::mem::take(&mut result.elements);
        let (rows, span_messages) = calculate_row_spans(rows);
        result.messages.extend(span_messages);
        result.messages.extend(style_messages);
        result.elements = vec![Element::Table(Table {
            children: rows,
            style_id,
            style_name,
        })];
        result
    }

    fn read_table_row(&mut self, element: &XmlElement) -> ReadResult {
        let properties = element.first_or_empty("w:trPr");
        // A deleted table row (tracked change) is dropped entirely.
        if properties.first("w:del").is_some() {
            return ReadResult::default();
        }
        let is_header = properties.first("w:tblHeader").is_some();
        let mut result = self.read_child_elements(element);
        let children = std::mem::take(&mut result.elements);
        result.elements = vec![Element::TableRow(TableRow {
            children,
            is_header,
        })];
        result
    }

    fn read_table_cell(&mut self, element: &XmlElement) -> ReadResult {
        let mut result = self.read_child_elements(element);
        let properties = element.first_or_empty("w:tcPr");
        let col_span = properties
            .first_or_empty("w:gridSpan")
            .attr("w:val")
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|span| *span > 0)
            .unwrap_or(1);
        let v_merge = properties
            .first("w:vMerge")
            .map(|v_merge| matches!(v_merge.attr("w:val"), None | Some("continue")));
        let children = std::mem::take(&mut result.elements);
        let mut cell = TableCell::new(children).with_col_span(col_span);
        cell.v_merge = v_merge;
        result.elements = vec![Element::TableCell(cell)];
        result
    }

    // -- Images -------------------------------------------------------------

    fn read_drawing_element(&mut self, element: &XmlElement) -> ReadResult {
        let mut result = ReadResult::default();
        for graphic in element.elements_named("a:graphic") {
            for graphic_data in graphic.elements_named("a:graphicData") {
                for pic in graphic_data.elements_named("pic:pic") {
                    for blip_fill in pic.elements_named("pic:blipFill") {
                        for blip in blip_fill.elements_named("a:blip") {
                            result.append(self.read_blip(element, blip));
                        }
                    }
                }
            }
        }
        result
    }

    fn read_blip(&mut self, element: &XmlElement, blip: &XmlElement) -> ReadResult {
        let properties = element.first_or_empty("wp:docPr");
        let descr = properties.attr("descr");
        let alt_text = if is_blank(descr) {
            properties.attr("title")
        } else {
            descr
        }
        .map(str::to_string);

        let image_file = if let Some(embed) = non_empty(blip.attr("r:embed")) {
            self.find_embedded_image_file(&embed)
        } else if let Some(link) = non_empty(blip.attr("r:link")) {
            self.context
                .relationships
                .find_target_by_id(&link)
                .map(|path| ImageFile::External(path.to_string()))
        } else {
            None
        };
        let Some(image_file) = image_file else {
            return ReadResult::warning("Could not find image file for a:blip element");
        };

        let mut result = self.read_image(image_file, alt_text);
        let relationship_id = properties.first_or_empty("a:hlinkClick").attr("r:id");
        if let Some(href) =
            relationship_id.and_then(|id| self.context.relationships.find_target_by_id(id))
        {
            let children = std::mem::take(&mut result.elements);
            result.elements = vec![Element::Hyperlink(Hyperlink {
                children,
                href: Some(href.to_string()),
                anchor: None,
                target_frame: None,
            })];
        }
        result
    }

    fn read_image_data(&mut self, element: &XmlElement) -> ReadResult {
        match non_empty(element.attr("r:id")) {
            Some(id) => match self.find_embedded_image_file(&id) {
                Some(file) => self.read_image(file, element.attr("o:title").map(str::to_string)),
                None => ReadResult::warning("Could not find image file for v:imagedata element"),
            },
            None => {
                ReadResult::warning("A v:imagedata element without a relationship ID was ignored")
            }
        }
    }

    fn find_embedded_image_file(&self, relationship_id: &str) -> Option<ImageFile> {
        self.context
            .relationships
            .find_target_by_id(relationship_id)
            .map(|target| ImageFile::Embedded(uri_to_zip_entry_name("word", target)))
    }

    fn read_image(&mut self, file: ImageFile, alt_text: Option<String>) -> ReadResult {
        let (path, data) = match &file {
            ImageFile::Embedded(path) => (path.as_str(), self.context.files.read_zip_entry(path)),
            ImageFile::External(uri) => (uri.as_str(), self.context.files.read_external(uri)),
        };
        let content_type = self.context.content_types.find_content_type(path);
        let mut result = ReadResult::default();
        let supported = content_type
            .as_deref()
            .is_some_and(|ct| SUPPORTED_IMAGE_TYPES.contains(&ct));
        if !supported {
            result.messages.push(Message::warning(format!(
                "Image of type {} is unlikely to display in web browsers",
                content_type.as_deref().unwrap_or("null")
            )));
        }
        result.elements.push(Element::Image(Image::from_result(
            alt_text,
            content_type,
            data,
        )));
        result
    }
}

enum ImageFile {
    Embedded(String),
    External(String),
}

fn note_reference(element: &XmlElement, note_type: NoteType) -> ReadResult {
    ReadResult::element(Element::NoteReference(NoteReference {
        note_type,
        note_id: element.attr("w:id").unwrap_or_default().to_string(),
    }))
}

fn read_underline(element: Option<&XmlElement>) -> bool {
    match element.and_then(|e| e.attr("w:val")) {
        Some(value) => !matches!(value, "false" | "0" | "none"),
        None => false,
    }
}

fn read_symbol(element: &XmlElement) -> ReadResult {
    // See 17.3.3.30 sym (Symbol Character) of ECMA-376 4th edition Part 1
    let font = element.attr("w:font").unwrap_or_default();
    let ch = element.attr("w:char").unwrap_or_default();
    let mut unicode = dingbats::lookup_hex(font, ch);
    if unicode.is_none() && ch.len() == 4 && ch.starts_with("F0") {
        unicode = dingbats::lookup_hex(font, &ch[2..]);
    }
    match unicode {
        Some(c) => ReadResult::element(Element::Text(c.to_string())),
        None => ReadResult::warning(format!(
            "A w:sym element with an unsupported character was ignored: char {ch} in font {font}"
        )),
    }
}

fn replace_first_text_with_checkbox(elements: &mut [Element], checked: bool, done: &mut bool) {
    for element in elements.iter_mut() {
        if *done {
            return;
        }
        if let Some(children) = element.children_mut() {
            replace_first_text_with_checkbox(children, checked, done);
        }
        if let Element::Text(text) = element
            && !text.is_empty()
            && !*done
        {
            *done = true;
            *element = Element::Checkbox { checked };
        }
    }
}

fn parse_instr_text(instr_text: &str, fld_char: &XmlElement) -> ComplexField {
    if let Some(target) = parse_hyperlink_instr(instr_text) {
        return ComplexField::Hyperlink(target);
    }
    if instr_text.contains("FORMCHECKBOX") {
        let checkbox = fld_char
            .first_or_empty("w:ffData")
            .first_or_empty("w:checkBox");
        let checked = match checkbox.first("w:checked") {
            Some(checked) => read_boolean_element(Some(checked)),
            None => read_boolean_element(checkbox.first("w:default")),
        };
        return ComplexField::Checkbox(checked);
    }
    ComplexField::Unknown
}

/// Parses `HYPERLINK "url"` and `HYPERLINK \l "anchor"` field instructions.
fn parse_hyperlink_instr(instr_text: &str) -> Option<HyperlinkTarget> {
    let rest = instr_text.trim_start().strip_prefix("HYPERLINK")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let mut rest = rest.trim_start();
    let mut is_anchor = false;
    if let Some(after) = rest.strip_prefix("\\l")
        && after.starts_with(char::is_whitespace)
    {
        is_anchor = true;
        rest = after.trim_start();
    }
    let location = if let Some(quoted) = rest.strip_prefix('"') {
        let line = quoted.split('\n').next().unwrap_or("");
        match line.rfind('"') {
            Some(end) => line[..end].to_string(),
            None => rest
                .split(char::is_whitespace)
                .next()
                .unwrap_or("")
                .to_string(),
        }
    } else if rest.starts_with('\\') || rest.is_empty() {
        return None;
    } else {
        rest.split(char::is_whitespace)
            .next()
            .unwrap_or("")
            .to_string()
    };
    Some(if is_anchor {
        HyperlinkTarget {
            href: None,
            anchor: Some(location),
        }
    } else {
        HyperlinkTarget {
            href: Some(location),
            anchor: None,
        }
    })
}

fn read_numbering_properties(
    style_id: Option<&str>,
    element: &XmlElement,
    numbering: &Numbering,
) -> Option<NumberingLevel> {
    let level = element.first_or_empty("w:ilvl").attr("w:val");
    let num_id = element.first_or_empty("w:numId").attr("w:val");
    if let (Some(level), Some(num_id)) = (level, num_id) {
        return numbering.find_level(num_id, level);
    }
    if let Some(style_id) = style_id
        && let Some(level) = numbering.find_level_by_paragraph_style_id(style_id)
    {
        return Some(level);
    }
    // Some malformed documents omit w:ilvl: assume level 0.
    if let Some(num_id) = num_id {
        return numbering.find_level(num_id, "0");
    }
    None
}

fn calculate_row_spans(mut rows: Vec<Element>) -> (Vec<Element>, Vec<Message>) {
    if rows.iter().any(|row| !matches!(row, Element::TableRow(_))) {
        return (
            rows,
            vec![Message::warning(
                "unexpected non-row element in table, cell merging may be incorrect",
            )],
        );
    }
    let has_non_cells = rows.iter().any(|row| match row {
        Element::TableRow(row) => row
            .children
            .iter()
            .any(|cell| !matches!(cell, Element::TableCell(_))),
        _ => false,
    });
    if has_non_cells {
        return (
            rows,
            vec![Message::warning(
                "unexpected non-cell element in table row, cell merging may be incorrect",
            )],
        );
    }

    // column index → (row index, cell index) of the cell currently spanning it
    let mut columns: std::collections::HashMap<u32, (usize, usize)> = Default::default();
    let mut increments: Vec<(usize, usize)> = Vec::new();
    for (row_index, row) in rows.iter_mut().enumerate() {
        let Element::TableRow(row) = row else {
            continue;
        };
        let mut column = 0u32;
        for (cell_index, cell) in row.children.iter_mut().enumerate() {
            let Element::TableCell(cell) = cell else {
                continue;
            };
            if cell.v_merge == Some(true) && columns.contains_key(&column) {
                increments.push(columns[&column]);
            } else {
                columns.insert(column, (row_index, cell_index));
                cell.v_merge = Some(false);
            }
            column += cell.col_span;
        }
    }
    for (row_index, cell_index) in increments {
        if let Element::TableRow(row) = &mut rows[row_index]
            && let Element::TableCell(cell) = &mut row.children[cell_index]
        {
            cell.row_span += 1;
        }
    }
    for row in rows.iter_mut() {
        if let Element::TableRow(row) = row {
            row.children
                .retain(|cell| !matches!(cell, Element::TableCell(c) if c.v_merge == Some(true)));
        }
    }
    (rows, Vec::new())
}

pub(crate) fn replace_fragment(uri: &str, fragment: &str) -> String {
    let base = uri.split('#').next().unwrap_or(uri);
    format!("{base}#{fragment}")
}

/// Resolves a relationship target relative to `base` into a zip entry name.
pub(crate) fn uri_to_zip_entry_name(base: &str, uri: &str) -> String {
    let joined = match uri.strip_prefix('/') {
        Some(absolute) => absolute.to_string(),
        None => format!("{base}/{uri}"),
    };
    // Normalise `.` and `..` segments.
    let mut segments: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyperlink_instructions() {
        let t = parse_hyperlink_instr(" HYPERLINK \"http://example.com\" ").unwrap();
        assert_eq!(t.href.as_deref(), Some("http://example.com"));
        let t = parse_hyperlink_instr("HYPERLINK \\l \"_Toc1\"").unwrap();
        assert_eq!(t.anchor.as_deref(), Some("_Toc1"));
        let t = parse_hyperlink_instr("HYPERLINK http://example.com \\o tip").unwrap();
        assert_eq!(t.href.as_deref(), Some("http://example.com"));
        assert!(parse_hyperlink_instr("PAGE").is_none());
    }

    #[test]
    fn zip_entry_names() {
        assert_eq!(
            uri_to_zip_entry_name("word", "media/a.png"),
            "word/media/a.png"
        );
        assert_eq!(uri_to_zip_entry_name("word", "/media/a.png"), "media/a.png");
        assert_eq!(
            uri_to_zip_entry_name("word", "../media/a.png"),
            "media/a.png"
        );
    }
}
