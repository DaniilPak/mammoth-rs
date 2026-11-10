//! Converts the document model to an HTML tree.
//! A port of mammoth.js's `document-to-html.js`.

use std::borrow::Cow;

use crate::document::{
    BreakType, Comment, Document, Element, Image, Note, NoteReference, Paragraph, Run, Table,
    TableCell, VerticalAlignment,
};
use crate::error::Message;
use crate::html::{Attributes, HtmlNode, HtmlPath};
use crate::images::ImageConverter;
use crate::style_map::{MatchTarget, StyleMapping};

pub(crate) struct ConvertSettings<'a> {
    pub style_map: &'a [StyleMapping],
    pub id_prefix: &'a str,
    pub ignore_empty_paragraphs: bool,
    pub image_converter: &'a ImageConverter,
}

#[derive(Clone, Copy, Default)]
struct Context {
    is_table_header: bool,
}

struct ReferencedComment<'a> {
    label: String,
    comment: &'a Comment,
}

pub(crate) struct Conversion<'a> {
    settings: ConvertSettings<'a>,
    comments: &'a [Comment],
    messages: Vec<Message>,
    note_number: usize,
    note_references: Vec<NoteReference>,
    referenced_comments: Vec<ReferencedComment<'a>>,
}

impl<'a> Conversion<'a> {
    pub fn new(settings: ConvertSettings<'a>, document: &'a Document) -> Self {
        Conversion {
            settings,
            comments: &document.comments,
            messages: Vec::new(),
            note_number: 1,
            note_references: Vec::new(),
            referenced_comments: Vec::new(),
        }
    }

    /// Converts the whole document, returning the (unsimplified) HTML tree
    /// and the messages produced.
    pub fn convert_document(mut self, document: &'a Document) -> (Vec<HtmlNode>, Vec<Message>) {
        let context = Context::default();
        let mut nodes = self.convert_elements(&document.children, context);

        let references = std::mem::take(&mut self.note_references);
        let notes: Vec<&Note> = references
            .iter()
            .filter_map(|reference| document.find_note(reference.note_type, &reference.note_id))
            .collect();
        let mut note_nodes = Vec::new();
        for note in notes {
            note_nodes.push(self.convert_note(note, context));
        }
        nodes.push(HtmlNode::fresh("ol", Attributes::new(), note_nodes));

        let referenced = std::mem::take(&mut self.referenced_comments);
        let mut comment_nodes = Vec::new();
        for referenced_comment in &referenced {
            comment_nodes.extend(self.convert_comment(referenced_comment, context));
        }
        nodes.push(HtmlNode::fresh("dl", Attributes::new(), comment_nodes));

        (nodes, self.messages)
    }

    fn find_style(&self, target: MatchTarget<'_>) -> Option<&'a HtmlPath> {
        let style_map: &'a [StyleMapping] = self.settings.style_map;
        style_map
            .iter()
            .find(|mapping| mapping.from.matches(&target))
            .map(|mapping| &mapping.to)
    }

    fn html_id(&self, suffix: &str) -> String {
        format!("{}{}", self.settings.id_prefix, suffix)
    }

    fn referent_html_id(&self, reference_type: &str, id: &str) -> String {
        self.html_id(&format!("{reference_type}-{id}"))
    }

    fn reference_html_id(&self, reference_type: &str, id: &str) -> String {
        self.html_id(&format!("{reference_type}-ref-{id}"))
    }

    fn convert_elements(&mut self, elements: &'a [Element], context: Context) -> Vec<HtmlNode> {
        let mut nodes = Vec::new();
        for element in elements {
            nodes.extend(self.element_to_html(element, context));
        }
        nodes
    }

    fn element_to_html(&mut self, element: &'a Element, context: Context) -> Vec<HtmlNode> {
        match element {
            Element::Paragraph(paragraph) => self.convert_paragraph(paragraph, context),
            Element::Run(run) => self.convert_run(run, context),
            Element::Text(text) => vec![HtmlNode::text(text.clone())],
            Element::Tab => vec![HtmlNode::text("\t")],
            Element::Hyperlink(hyperlink) => {
                let href = match &hyperlink.anchor {
                    Some(anchor) => Some(format!("#{}", self.html_id(anchor))),
                    None => hyperlink.href.clone(),
                };
                let mut attributes = Attributes::new();
                if let Some(href) = href {
                    attributes.set("href", href);
                }
                if let Some(target) = &hyperlink.target_frame {
                    attributes.set("target", target.clone());
                }
                let children = self.convert_elements(&hyperlink.children, context);
                vec![HtmlNode::non_fresh("a", attributes, children)]
            }
            Element::Checkbox { checked } => {
                let mut attributes = Attributes::new().with("type", "checkbox");
                if *checked {
                    attributes.set("checked", "checked");
                }
                vec![HtmlNode::fresh("input", attributes, Vec::new())]
            }
            Element::BookmarkStart { name } => vec![HtmlNode::fresh(
                "a",
                Attributes::new().with("id", self.html_id(name)),
                vec![HtmlNode::ForceWrite],
            )],
            Element::NoteReference(reference) => {
                self.note_references.push(reference.clone());
                let note_type = reference.note_type.as_str();
                let anchor = HtmlNode::fresh(
                    "a",
                    Attributes::new()
                        .with(
                            "href",
                            format!("#{}", self.referent_html_id(note_type, &reference.note_id)),
                        )
                        .with("id", self.reference_html_id(note_type, &reference.note_id)),
                    vec![HtmlNode::text(format!("[{}]", self.note_number))],
                );
                self.note_number += 1;
                vec![HtmlNode::fresh("sup", Attributes::new(), vec![anchor])]
            }
            Element::CommentReference { comment_id } => {
                let path: Cow<'a, HtmlPath> = match self.find_style(MatchTarget::CommentReference) {
                    Some(path) => Cow::Borrowed(path),
                    None => Cow::Owned(HtmlPath::Ignore),
                };
                path.wrap(|| self.convert_comment_reference(comment_id))
            }
            Element::Image(image) => self.convert_image(image),
            Element::Table(table) => self.convert_table(table),
            Element::TableRow(row) => {
                let mut children = vec![HtmlNode::ForceWrite];
                children.extend(self.convert_elements(&row.children, context));
                vec![HtmlNode::fresh("tr", Attributes::new(), children)]
            }
            Element::TableCell(cell) => self.convert_table_cell(cell, context),
            Element::Break(break_type) => {
                let path: Cow<'a, HtmlPath> = match self.find_style(MatchTarget::Break(*break_type))
                {
                    Some(path) => Cow::Borrowed(path),
                    None if *break_type == BreakType::Line => {
                        Cow::Owned(HtmlPath::top_level_element("br", Attributes::new()))
                    }
                    None => Cow::Owned(HtmlPath::empty()),
                };
                path.wrap(Vec::new)
            }
        }
    }

    fn convert_paragraph(&mut self, paragraph: &'a Paragraph, context: Context) -> Vec<HtmlNode> {
        let path: Cow<'a, HtmlPath> = match self.find_style(MatchTarget::Paragraph(paragraph)) {
            Some(path) => Cow::Borrowed(path),
            None => {
                if let Some(style_id) = &paragraph.style_id {
                    self.messages.push(unrecognised_style_warning(
                        "paragraph",
                        paragraph.style_name.as_deref(),
                        style_id,
                    ));
                }
                Cow::Owned(HtmlPath::top_level_element("p", Attributes::new()))
            }
        };
        let ignore_empty = self.settings.ignore_empty_paragraphs;
        path.wrap(|| {
            let content = self.convert_elements(&paragraph.children, context);
            if ignore_empty {
                content
            } else {
                let mut nodes = vec![HtmlNode::ForceWrite];
                nodes.extend(content);
                nodes
            }
        })
    }

    fn path_for_run_property(
        &self,
        target: MatchTarget<'_>,
        default_tag: Option<&str>,
    ) -> Cow<'a, HtmlPath> {
        match self.find_style(target) {
            Some(path) => Cow::Borrowed(path),
            None => match default_tag {
                Some(tag) => Cow::Owned(HtmlPath::element(tag, false)),
                None => Cow::Owned(HtmlPath::empty()),
            },
        }
    }

    fn convert_run(&mut self, run: &'a Run, context: Context) -> Vec<HtmlNode> {
        // Paths are listed innermost first.
        let mut paths: Vec<Cow<'a, HtmlPath>> = Vec::new();
        if let Some(color) = &run.highlight
            && let Some(path) = self.find_style(MatchTarget::Highlight(color))
        {
            paths.push(Cow::Borrowed(path));
        }
        if run.is_small_caps {
            paths.push(self.path_for_run_property(MatchTarget::SmallCaps, None));
        }
        if run.is_all_caps {
            paths.push(self.path_for_run_property(MatchTarget::AllCaps, None));
        }
        if run.is_strikethrough {
            paths.push(self.path_for_run_property(MatchTarget::Strikethrough, Some("s")));
        }
        if run.is_underline {
            paths.push(self.path_for_run_property(MatchTarget::Underline, None));
        }
        if run.vertical_alignment == VerticalAlignment::Subscript {
            paths.push(Cow::Owned(HtmlPath::element("sub", false)));
        }
        if run.vertical_alignment == VerticalAlignment::Superscript {
            paths.push(Cow::Owned(HtmlPath::element("sup", false)));
        }
        if run.is_italic {
            paths.push(self.path_for_run_property(MatchTarget::Italic, Some("em")));
        }
        if run.is_bold {
            paths.push(self.path_for_run_property(MatchTarget::Bold, Some("strong")));
        }
        let style_path = match self.find_style(MatchTarget::Run(run)) {
            Some(path) => Cow::Borrowed(path),
            None => {
                if let Some(style_id) = &run.style_id {
                    self.messages.push(unrecognised_style_warning(
                        "run",
                        run.style_name.as_deref(),
                        style_id,
                    ));
                }
                Cow::Owned(HtmlPath::empty())
            }
        };
        paths.push(style_path);
        self.wrap_paths(&paths, &run.children, context)
    }

    fn wrap_paths(
        &mut self,
        paths: &[Cow<'a, HtmlPath>],
        children: &'a [Element],
        context: Context,
    ) -> Vec<HtmlNode> {
        match paths.split_last() {
            None => self.convert_elements(children, context),
            Some((outer, rest)) => outer.wrap(|| self.wrap_paths(rest, children, context)),
        }
    }

    fn convert_image(&mut self, image: &Image) -> Vec<HtmlNode> {
        match self.settings.image_converter.convert(image) {
            Ok(result) => {
                let mut attributes = Attributes::new();
                if let Some(alt) = image.alt_text.as_deref().filter(|alt| !alt.is_empty()) {
                    attributes.set("alt", alt);
                }
                for (key, value) in result {
                    attributes.set(&key, value);
                }
                vec![HtmlNode::fresh("img", attributes, Vec::new())]
            }
            Err(error) => {
                self.messages.push(Message::error(error.to_string()));
                Vec::new()
            }
        }
    }

    fn convert_table(&mut self, table: &'a Table) -> Vec<HtmlNode> {
        let path: Cow<'a, HtmlPath> = match self.find_style(MatchTarget::Table(table)) {
            Some(path) => Cow::Borrowed(path),
            None => Cow::Owned(HtmlPath::top_level_element("table", Attributes::new())),
        };
        path.wrap(|| self.convert_table_children(table))
    }

    fn convert_table_children(&mut self, table: &'a Table) -> Vec<HtmlNode> {
        let body_index = table
            .children
            .iter()
            .position(|child| !matches!(child, Element::TableRow(row) if row.is_header))
            .unwrap_or(table.children.len());
        let mut nodes = vec![HtmlNode::ForceWrite];
        if body_index == 0 {
            nodes.extend(self.convert_elements(
                &table.children,
                Context {
                    is_table_header: false,
                },
            ));
        } else {
            let head_rows = self.convert_elements(
                &table.children[..body_index],
                Context {
                    is_table_header: true,
                },
            );
            let body_rows = self.convert_elements(
                &table.children[body_index..],
                Context {
                    is_table_header: false,
                },
            );
            nodes.push(HtmlNode::fresh("thead", Attributes::new(), head_rows));
            nodes.push(HtmlNode::fresh("tbody", Attributes::new(), body_rows));
        }
        nodes
    }

    fn convert_table_cell(&mut self, cell: &'a TableCell, context: Context) -> Vec<HtmlNode> {
        let tag_name = if context.is_table_header { "th" } else { "td" };
        let mut attributes = Attributes::new();
        if cell.col_span != 1 {
            attributes.set("colspan", cell.col_span.to_string());
        }
        if cell.row_span != 1 {
            attributes.set("rowspan", cell.row_span.to_string());
        }
        let mut children = vec![HtmlNode::ForceWrite];
        children.extend(self.convert_elements(&cell.children, context));
        vec![HtmlNode::fresh(tag_name, attributes, children)]
    }

    fn convert_note(&mut self, note: &'a Note, context: Context) -> HtmlNode {
        let note_type = note.note_type.as_str();
        let mut body = self.convert_elements(&note.body, context);
        body.push(HtmlNode::non_fresh(
            "p",
            Attributes::new(),
            vec![
                HtmlNode::text(" "),
                HtmlNode::fresh(
                    "a",
                    Attributes::new().with(
                        "href",
                        format!("#{}", self.reference_html_id(note_type, &note.note_id)),
                    ),
                    vec![HtmlNode::text("↑")],
                ),
            ],
        ));
        HtmlNode::fresh(
            "li",
            Attributes::new().with("id", self.referent_html_id(note_type, &note.note_id)),
            body,
        )
    }

    fn convert_comment_reference(&mut self, comment_id: &str) -> Vec<HtmlNode> {
        let comments: &'a [Comment] = self.comments;
        let comment = comments.iter().find(|c| c.comment_id == comment_id);
        let count = self.referenced_comments.len() + 1;
        let initials = comment
            .and_then(|c| c.author_initials.as_deref())
            .unwrap_or("");
        let label = format!("[{initials}{count}]");
        if let Some(comment) = comment {
            self.referenced_comments.push(ReferencedComment {
                label: label.clone(),
                comment,
            });
        }
        vec![HtmlNode::fresh(
            "a",
            Attributes::new()
                .with(
                    "href",
                    format!("#{}", self.referent_html_id("comment", comment_id)),
                )
                .with("id", self.reference_html_id("comment", comment_id)),
            vec![HtmlNode::text(label)],
        )]
    }

    fn convert_comment(
        &mut self,
        referenced: &ReferencedComment<'a>,
        context: Context,
    ) -> Vec<HtmlNode> {
        let comment = referenced.comment;
        let mut body = self.convert_elements(&comment.body, context);
        body.push(HtmlNode::non_fresh(
            "p",
            Attributes::new(),
            vec![
                HtmlNode::text(" "),
                HtmlNode::fresh(
                    "a",
                    Attributes::new().with(
                        "href",
                        format!(
                            "#{}",
                            self.reference_html_id("comment", &comment.comment_id)
                        ),
                    ),
                    vec![HtmlNode::text("↑")],
                ),
            ],
        ));
        vec![
            HtmlNode::fresh(
                "dt",
                Attributes::new().with("id", self.referent_html_id("comment", &comment.comment_id)),
                vec![HtmlNode::text(format!("Comment {}", referenced.label))],
            ),
            HtmlNode::fresh("dd", Attributes::new(), body),
        ]
    }
}

fn unrecognised_style_warning(kind: &str, style_name: Option<&str>, style_id: &str) -> Message {
    Message::warning(format!(
        "Unrecognised {kind} style: '{}' (Style ID: {style_id})",
        style_name.unwrap_or("null")
    ))
}
