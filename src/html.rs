//! HTML tree, simplification and writers (HTML, pretty HTML and Markdown).
//!
//! A port of mammoth.js's `html/` and `writers/` modules.

use std::rc::Rc;

/// Ordered attribute list. Equality ignores order, as in mammoth.js.
#[derive(Debug, Clone, Default)]
pub(crate) struct Attributes(pub Vec<(String, String)>);

impl Attributes {
    pub fn new() -> Self {
        Attributes(Vec::new())
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self.0.iter_mut().find(|(key, _)| key == name) {
            Some(entry) => entry.1 = value,
            None => self.0.push((name.to_string(), value)),
        }
    }

    pub fn with(mut self, name: &str, value: impl Into<String>) -> Self {
        self.set(name, value);
        self
    }
}

impl PartialEq for Attributes {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .all(|(key, value)| other.get(key) == Some(value.as_str()))
    }
}

/// An element in an HTML path, such as `ul|ol.list:fresh`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HtmlTag {
    pub tag_names: Vec<String>,
    pub attributes: Attributes,
    pub fresh: bool,
    pub separator: Option<String>,
}

impl HtmlTag {
    pub fn new(tag_name: &str, attributes: Attributes, fresh: bool) -> Self {
        HtmlTag {
            tag_names: vec![tag_name.to_string()],
            attributes,
            fresh,
            separator: None,
        }
    }

    pub fn tag_name(&self) -> &str {
        &self.tag_names[0]
    }

    fn matches_element(&self, other: &HtmlTag) -> bool {
        self.tag_names.iter().any(|name| name == other.tag_name())
            && self.attributes == other.attributes
    }
}

#[derive(Debug, Clone)]
pub(crate) enum HtmlNode {
    Element {
        tag: Rc<HtmlTag>,
        children: Vec<HtmlNode>,
    },
    Text(String),
    /// Forces the parent element to be written even if otherwise empty.
    ForceWrite,
}

impl HtmlNode {
    pub fn fresh(tag_name: &str, attributes: Attributes, children: Vec<HtmlNode>) -> Self {
        HtmlNode::Element {
            tag: Rc::new(HtmlTag::new(tag_name, attributes, true)),
            children,
        }
    }

    pub fn non_fresh(tag_name: &str, attributes: Attributes, children: Vec<HtmlNode>) -> Self {
        HtmlNode::Element {
            tag: Rc::new(HtmlTag::new(tag_name, attributes, false)),
            children,
        }
    }

    pub fn text(value: impl Into<String>) -> Self {
        HtmlNode::Text(value.into())
    }
}

const VOID_TAG_NAMES: &[&str] = &["br", "hr", "img", "input"];

fn is_void(tag: &HtmlTag, children: &[HtmlNode]) -> bool {
    children.is_empty() && VOID_TAG_NAMES.contains(&tag.tag_name())
}

/// A path of nested HTML elements produced by a style mapping.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HtmlPath {
    Elements(Vec<Rc<HtmlTag>>),
    /// `!`: the matched element and its contents are dropped.
    Ignore,
}

impl HtmlPath {
    pub fn empty() -> Self {
        HtmlPath::Elements(Vec::new())
    }

    pub fn top_level_element(tag_name: &str, attributes: Attributes) -> Self {
        HtmlPath::Elements(vec![Rc::new(HtmlTag::new(tag_name, attributes, true))])
    }

    pub fn element(tag_name: &str, fresh: bool) -> Self {
        HtmlPath::Elements(vec![Rc::new(HtmlTag::new(
            tag_name,
            Attributes::new(),
            fresh,
        ))])
    }

    /// Wraps the nodes produced by `children` in this path. `children` is
    /// not called for [`HtmlPath::Ignore`].
    pub fn wrap(&self, children: impl FnOnce() -> Vec<HtmlNode>) -> Vec<HtmlNode> {
        match self {
            HtmlPath::Ignore => Vec::new(),
            HtmlPath::Elements(tags) => {
                let mut nodes = children();
                for tag in tags.iter().rev() {
                    nodes = vec![HtmlNode::Element {
                        tag: tag.clone(),
                        children: nodes,
                    }];
                }
                nodes
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Simplification: remove empty elements and merge adjacent non-fresh ones.
// ---------------------------------------------------------------------------

pub(crate) fn simplify(nodes: Vec<HtmlNode>) -> Vec<HtmlNode> {
    collapse(remove_empty(nodes))
}

fn remove_empty(nodes: Vec<HtmlNode>) -> Vec<HtmlNode> {
    let mut result = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            HtmlNode::Element { tag, children } => {
                let children = remove_empty(children);
                if !children.is_empty() || is_void(&tag, &children) {
                    result.push(HtmlNode::Element { tag, children });
                }
            }
            HtmlNode::Text(text) => {
                if !text.is_empty() {
                    result.push(HtmlNode::Text(text));
                }
            }
            HtmlNode::ForceWrite => result.push(HtmlNode::ForceWrite),
        }
    }
    result
}

fn collapse(nodes: Vec<HtmlNode>) -> Vec<HtmlNode> {
    let mut children = Vec::with_capacity(nodes.len());
    for node in nodes {
        let node = match node {
            HtmlNode::Element { tag, children } => HtmlNode::Element {
                tag,
                children: collapse(children),
            },
            other => other,
        };
        append_child(&mut children, node);
    }
    children
}

fn append_child(children: &mut Vec<HtmlNode>, child: HtmlNode) {
    let HtmlNode::Element {
        tag: child_tag,
        children: grandchildren,
    } = child
    else {
        children.push(child);
        return;
    };
    if !child_tag.fresh
        && let Some(HtmlNode::Element {
            tag: last_tag,
            children: last_children,
        }) = children.last_mut()
        && child_tag.matches_element(last_tag)
    {
        if let Some(separator) = &child_tag.separator {
            append_child(last_children, HtmlNode::Text(separator.clone()));
        }
        for grandchild in grandchildren {
            append_child(last_children, grandchild);
        }
        return;
    }
    children.push(HtmlNode::Element {
        tag: child_tag,
        children: grandchildren,
    });
}

// ---------------------------------------------------------------------------
// Writers
// ---------------------------------------------------------------------------

pub(crate) trait Writer {
    fn open(&mut self, tag_name: &str, attributes: &Attributes);
    fn close(&mut self, tag_name: &str);
    fn text(&mut self, value: &str);
    fn self_closing(&mut self, tag_name: &str, attributes: &Attributes);
    fn finish(self: Box<Self>) -> String;
}

pub(crate) fn write(writer: &mut dyn Writer, nodes: &[HtmlNode]) {
    for node in nodes {
        match node {
            HtmlNode::Element { tag, children } => {
                if is_void(tag, children) {
                    writer.self_closing(tag.tag_name(), &tag.attributes);
                } else {
                    writer.open(tag.tag_name(), &tag.attributes);
                    write(writer, children);
                    writer.close(tag.tag_name());
                }
            }
            HtmlNode::Text(text) => writer.text(text),
            HtmlNode::ForceWrite => {}
        }
    }
}

pub(crate) fn escape_html_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            other => output.push(other),
        }
    }
    output
}

pub(crate) fn escape_html_attribute(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '"' => output.push_str("&quot;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            other => output.push(other),
        }
    }
    output
}

fn attribute_string(attributes: &Attributes) -> String {
    let mut output = String::new();
    for (key, value) in &attributes.0 {
        output.push(' ');
        output.push_str(key);
        output.push_str("=\"");
        output.push_str(&escape_html_attribute(value));
        output.push('"');
    }
    output
}

#[derive(Default)]
pub(crate) struct SimpleHtmlWriter {
    output: String,
}

impl Writer for SimpleHtmlWriter {
    fn open(&mut self, tag_name: &str, attributes: &Attributes) {
        self.output.push('<');
        self.output.push_str(tag_name);
        self.output.push_str(&attribute_string(attributes));
        self.output.push('>');
    }

    fn close(&mut self, tag_name: &str) {
        self.output.push_str("</");
        self.output.push_str(tag_name);
        self.output.push('>');
    }

    fn text(&mut self, value: &str) {
        self.output.push_str(&escape_html_text(value));
    }

    fn self_closing(&mut self, tag_name: &str, attributes: &Attributes) {
        self.output.push('<');
        self.output.push_str(tag_name);
        self.output.push_str(&attribute_string(attributes));
        self.output.push_str(" />");
    }

    fn finish(self: Box<Self>) -> String {
        self.output
    }
}

const INDENTED_ELEMENTS: &[&str] = &["div", "p", "ul", "li"];
const INDENTATION: &str = "  ";

#[derive(Default)]
pub(crate) struct PrettyHtmlWriter {
    inner: SimpleHtmlWriter,
    indentation_level: usize,
    stack: Vec<String>,
    started: bool,
    in_text: bool,
}

impl PrettyHtmlWriter {
    fn is_indented(tag_name: &str) -> bool {
        INDENTED_ELEMENTS.contains(&tag_name)
    }

    fn inside_indented_element(&self) -> bool {
        self.stack.last().is_none_or(|tag| Self::is_indented(tag))
    }

    fn is_in_pre(&self) -> bool {
        self.stack.iter().any(|tag| tag == "pre")
    }

    fn indent(&mut self) {
        self.in_text = false;
        if self.started && self.inside_indented_element() && !self.is_in_pre() {
            self.inner.output.push('\n');
            for _ in 0..self.indentation_level {
                self.inner.output.push_str(INDENTATION);
            }
        }
    }
}

impl Writer for PrettyHtmlWriter {
    fn open(&mut self, tag_name: &str, attributes: &Attributes) {
        if Self::is_indented(tag_name) {
            self.indent();
        }
        self.stack.push(tag_name.to_string());
        self.inner.open(tag_name, attributes);
        if Self::is_indented(tag_name) {
            self.indentation_level += 1;
        }
        self.started = true;
    }

    fn close(&mut self, tag_name: &str) {
        if Self::is_indented(tag_name) {
            self.indentation_level = self.indentation_level.saturating_sub(1);
            self.indent();
        }
        self.stack.pop();
        self.inner.close(tag_name);
    }

    fn text(&mut self, value: &str) {
        if !self.in_text {
            self.indent();
            self.in_text = true;
        }
        if self.is_in_pre() {
            self.inner.text(value);
        } else {
            let indentation = format!("\n{}", INDENTATION.repeat(self.indentation_level));
            self.inner.text(&value.replace('\n', &indentation));
        }
    }

    fn self_closing(&mut self, tag_name: &str, attributes: &Attributes) {
        self.indent();
        self.inner.self_closing(tag_name, attributes);
    }

    fn finish(self: Box<Self>) -> String {
        self.inner.output
    }
}

// -- Markdown ---------------------------------------------------------------

#[derive(Clone, Copy)]
struct MarkdownList {
    is_ordered: bool,
    indent: usize,
    count: usize,
}

enum MarkdownEnd {
    Text(String),
    ListItem,
}

struct MarkdownEntry {
    end: MarkdownEnd,
    pushed_list: bool,
}

#[derive(Default)]
pub(crate) struct MarkdownWriter {
    output: String,
    stack: Vec<MarkdownEntry>,
    lists: Vec<MarkdownList>,
    list_item_has_closed: bool,
}

pub(crate) fn escape_markdown(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(
            ch,
            '\\' | '`'
                | '*'
                | '_'
                | '{'
                | '}'
                | '['
                | ']'
                | '('
                | ')'
                | '#'
                | '+'
                | '-'
                | '.'
                | '!'
        ) {
            output.push('\\');
        }
        output.push(ch);
    }
    output
}

impl MarkdownWriter {
    fn write_anchor(&mut self, attributes: &Attributes) {
        if let Some(id) = attributes.get("id") {
            let mut attrs = Attributes::new();
            attrs.set("id", id);
            let mut writer = SimpleHtmlWriter::default();
            writer.open("a", &attrs);
            writer.close("a");
            self.output.push_str(&writer.output);
        }
    }
}

impl Writer for MarkdownWriter {
    fn open(&mut self, tag_name: &str, attributes: &Attributes) {
        let mut start = String::new();
        let mut end = MarkdownEnd::Text(String::new());
        let mut anchor_before_start = false;
        let mut pushed_list = false;

        match tag_name {
            "p" => end = MarkdownEnd::Text("\n\n".into()),
            "br" => end = MarkdownEnd::Text("  \n".into()),
            "ul" | "ol" => {
                let parent = self.lists.last().copied();
                if parent.is_some() {
                    start = "\n".into();
                } else {
                    end = MarkdownEnd::Text("\n".into());
                }
                self.lists.push(MarkdownList {
                    is_ordered: tag_name == "ol",
                    indent: parent.map_or(0, |list| list.indent + 1),
                    count: 0,
                });
                pushed_list = true;
            }
            "li" => {
                let list = match self.lists.last_mut() {
                    Some(list) => {
                        list.count += 1;
                        *list
                    }
                    None => MarkdownList {
                        is_ordered: false,
                        indent: 0,
                        count: 1,
                    },
                };
                self.list_item_has_closed = false;
                let bullet = if list.is_ordered {
                    format!("{}.", list.count)
                } else {
                    "-".to_string()
                };
                start = format!("{}{} ", "\t".repeat(list.indent), bullet);
                end = MarkdownEnd::ListItem;
            }
            "strong" => {
                start = "__".into();
                end = MarkdownEnd::Text("__".into());
            }
            "em" => {
                start = "*".into();
                end = MarkdownEnd::Text("*".into());
            }
            "a" => {
                let href = attributes.get("href").unwrap_or("");
                if !href.is_empty() {
                    start = "[".into();
                    end = MarkdownEnd::Text(format!("]({})", escape_markdown(href)));
                    anchor_before_start = true;
                }
            }
            "img" => {
                let src = attributes.get("src").unwrap_or("");
                let alt = attributes.get("alt").unwrap_or("");
                if !src.is_empty() || !alt.is_empty() {
                    start = format!("![{}]({})", escape_markdown(alt), escape_markdown(src));
                }
            }
            _ => {
                if let Some(level) = heading_level(tag_name) {
                    start = format!("{} ", "#".repeat(level));
                    end = MarkdownEnd::Text("\n\n".into());
                }
            }
        }

        self.stack.push(MarkdownEntry { end, pushed_list });
        if anchor_before_start {
            self.write_anchor(attributes);
        }
        self.output.push_str(&start);
        if !anchor_before_start {
            self.write_anchor(attributes);
        }
    }

    fn close(&mut self, _tag_name: &str) {
        let Some(entry) = self.stack.pop() else {
            return;
        };
        if entry.pushed_list {
            self.lists.pop();
        }
        match entry.end {
            MarkdownEnd::Text(text) => self.output.push_str(&text),
            MarkdownEnd::ListItem => {
                if !self.list_item_has_closed {
                    self.list_item_has_closed = true;
                    self.output.push('\n');
                }
            }
        }
    }

    fn text(&mut self, value: &str) {
        self.output.push_str(&escape_markdown(value));
    }

    fn self_closing(&mut self, tag_name: &str, attributes: &Attributes) {
        self.open(tag_name, attributes);
        self.close(tag_name);
    }

    fn finish(self: Box<Self>) -> String {
        self.output
    }
}

fn heading_level(tag_name: &str) -> Option<usize> {
    let level = tag_name.strip_prefix('h')?.parse::<usize>().ok()?;
    (1..=6).contains(&level).then_some(level)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_html(nodes: Vec<HtmlNode>) -> String {
        let mut writer: Box<dyn Writer> = Box::<SimpleHtmlWriter>::default();
        write(writer.as_mut(), &simplify(nodes));
        writer.finish()
    }

    #[test]
    fn adjacent_non_fresh_elements_are_merged() {
        let html = to_html(vec![
            HtmlNode::non_fresh("strong", Attributes::new(), vec![HtmlNode::text("a")]),
            HtmlNode::non_fresh("strong", Attributes::new(), vec![HtmlNode::text("b")]),
        ]);
        assert_eq!(html, "<strong>ab</strong>");
    }

    #[test]
    fn fresh_elements_are_not_merged() {
        let html = to_html(vec![
            HtmlNode::fresh("p", Attributes::new(), vec![HtmlNode::text("a")]),
            HtmlNode::fresh("p", Attributes::new(), vec![HtmlNode::text("b")]),
        ]);
        assert_eq!(html, "<p>a</p><p>b</p>");
    }

    #[test]
    fn empty_elements_are_removed_unless_forced() {
        let html = to_html(vec![
            HtmlNode::fresh("p", Attributes::new(), vec![]),
            HtmlNode::fresh("td", Attributes::new(), vec![HtmlNode::ForceWrite]),
            HtmlNode::fresh("br", Attributes::new(), vec![]),
        ]);
        assert_eq!(html, "<td></td><br />");
    }

    #[test]
    fn text_and_attributes_are_escaped() {
        let html = to_html(vec![HtmlNode::fresh(
            "a",
            Attributes::new().with("href", "?a=\"1\"&b"),
            vec![HtmlNode::text("<&>\"")],
        )]);
        assert_eq!(
            html,
            "<a href=\"?a=&quot;1&quot;&amp;b\">&lt;&amp;&gt;\"</a>"
        );
    }

    #[test]
    fn markdown_nested_lists() {
        let li = |text: &str, children: Vec<HtmlNode>| {
            let mut nodes = vec![HtmlNode::text(text)];
            nodes.extend(children);
            HtmlNode::fresh("li", Attributes::new(), nodes)
        };
        let nodes = vec![HtmlNode::fresh(
            "ul",
            Attributes::new(),
            vec![
                li(
                    "a",
                    vec![HtmlNode::fresh(
                        "ol",
                        Attributes::new(),
                        vec![li("b", vec![]), li("c", vec![])],
                    )],
                ),
                li("d", vec![]),
            ],
        )];
        let mut writer: Box<dyn Writer> = Box::<MarkdownWriter>::default();
        write(writer.as_mut(), &nodes);
        assert_eq!(writer.finish(), "- a\n\t1. b\n\t2. c\n- d\n\n");
    }
}
