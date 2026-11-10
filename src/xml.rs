//! A tiny XML DOM built on top of `quick-xml`.
//!
//! Element and attribute names are normalised to well-known prefixes based on
//! their namespace URI (so the strict and transitional OOXML flavours look
//! the same), following mammoth.js's `office-xml-reader`.

use quick_xml::NsReader;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;

#[derive(Debug, Clone)]
pub(crate) enum XmlNode {
    Element(XmlElement),
    Text(String),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct XmlElement {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<XmlNode>,
}

pub(crate) static EMPTY: XmlElement = XmlElement {
    name: String::new(),
    attributes: Vec::new(),
    children: Vec::new(),
};

impl XmlElement {
    pub fn new(name: &str) -> Self {
        XmlElement {
            name: name.to_string(),
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn first(&self, name: &str) -> Option<&XmlElement> {
        self.elements().find(|element| element.name == name)
    }

    pub fn first_or_empty(&self, name: &str) -> &XmlElement {
        self.first(name).unwrap_or(&EMPTY)
    }

    pub fn elements(&self) -> impl Iterator<Item = &XmlElement> {
        self.children.iter().filter_map(|child| match child {
            XmlNode::Element(element) => Some(element),
            XmlNode::Text(_) => None,
        })
    }

    pub fn elements_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlElement> {
        self.elements().filter(move |element| element.name == name)
    }

    /// Concatenated text of direct text children.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for child in &self.children {
            if let XmlNode::Text(value) = child {
                text.push_str(value);
            }
        }
        text
    }
}

const NAMESPACES: &[(&str, &str)] = &[
    // Transitional format
    (
        "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        "w",
    ),
    (
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
        "r",
    ),
    (
        "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
        "wp",
    ),
    ("http://schemas.openxmlformats.org/drawingml/2006/main", "a"),
    (
        "http://schemas.openxmlformats.org/drawingml/2006/picture",
        "pic",
    ),
    // Strict format
    ("http://purl.oclc.org/ooxml/wordprocessingml/main", "w"),
    (
        "http://purl.oclc.org/ooxml/officeDocument/relationships",
        "r",
    ),
    (
        "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
        "wp",
    ),
    ("http://purl.oclc.org/ooxml/drawingml/main", "a"),
    ("http://purl.oclc.org/ooxml/drawingml/picture", "pic"),
    // Common
    (
        "http://schemas.openxmlformats.org/package/2006/content-types",
        "content-types",
    ),
    (
        "http://schemas.openxmlformats.org/package/2006/relationships",
        "relationships",
    ),
    (
        "http://schemas.openxmlformats.org/markup-compatibility/2006",
        "mc",
    ),
    ("urn:schemas-microsoft-com:vml", "v"),
    ("urn:schemas-microsoft-com:office:word", "office-word"),
    ("urn:schemas-microsoft-com:office:office", "o"),
    // [MS-DOCX]: Word Extensions to the Office Open XML (.docx) File Format
    (
        "http://schemas.microsoft.com/office/word/2010/wordml",
        "wordml",
    ),
];

fn convert_name(resolved: ResolveResult<'_>, local: &str) -> String {
    match resolved {
        ResolveResult::Bound(namespace) => {
            let uri = namespace.0;
            match NAMESPACES.iter().find(|(known, _)| *known == uri) {
                Some((_, prefix)) => format!("{prefix}:{local}"),
                None => format!("{{{uri}}}{local}"),
            }
        }
        ResolveResult::Unbound => local.to_string(),
        ResolveResult::Unknown(prefix) => format!("{prefix}:{local}"),
    }
}

fn predefined_entity(name: &str) -> Option<&'static str> {
    match name {
        "lt" => Some("<"),
        "gt" => Some(">"),
        "amp" => Some("&"),
        "apos" => Some("'"),
        "quot" => Some("\""),
        _ => None,
    }
}

fn push_text(children: &mut Vec<XmlNode>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(XmlNode::Text(existing)) = children.last_mut() {
        existing.push_str(text);
    } else {
        children.push(XmlNode::Text(text.to_string()));
    }
}

/// Parses an XML string into its root element, collapsing
/// `mc:AlternateContent` elements into their `mc:Fallback` children.
pub(crate) fn parse(source: &str) -> Result<XmlElement, String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut reader = NsReader::from_str(source);
    let mut stack: Vec<XmlElement> = vec![XmlElement::new("#document")];

    loop {
        let event = reader.read_event().map_err(|e| e.to_string())?;
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                let (resolved, local) = reader.resolver().resolve_element(start.name());
                let name = convert_name(resolved, local.into_inner());
                let mut element = XmlElement::new(&name);
                for attribute in start.attributes().with_checks(false) {
                    let attribute = attribute.map_err(|e| e.to_string())?;
                    let key = attribute.key.0;
                    if key == "xmlns" || key.starts_with("xmlns:") {
                        continue;
                    }
                    let (resolved, local) = reader.resolver().resolve_attribute(attribute.key);
                    let name = convert_name(resolved, local.into_inner());
                    let value = attribute
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|e| e.to_string())?
                        .into_owned();
                    element.attributes.push((name, value));
                }
                if matches!(event, Event::Empty(_)) {
                    append_element(stack.last_mut().expect("stack is never empty"), element);
                } else {
                    stack.push(element);
                }
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err("unexpected closing tag".to_string());
                }
                let element = stack.pop().expect("checked length");
                append_element(stack.last_mut().expect("stack is never empty"), element);
            }
            Event::Text(text) => {
                let content = text.xml10_content();
                push_text(&mut stack.last_mut().expect("stack").children, &content);
            }
            Event::CData(data) => {
                let content = data.xml10_content();
                push_text(&mut stack.last_mut().expect("stack").children, &content);
            }
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref().map_err(|e| e.to_string())? {
                    Some(ch) => ch.to_string(),
                    None => {
                        let name = reference.xml10_content();
                        match predefined_entity(&name) {
                            Some(value) => value.to_string(),
                            None => return Err(format!("unknown entity &{name};")),
                        }
                    }
                };
                push_text(&mut stack.last_mut().expect("stack").children, &resolved);
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if stack.len() != 1 {
        return Err("unexpected end of document".to_string());
    }
    let document = stack.pop().expect("checked length");
    document
        .children
        .into_iter()
        .find_map(|child| match child {
            XmlNode::Element(element) => Some(element),
            XmlNode::Text(_) => None,
        })
        .ok_or_else(|| "document has no root element".to_string())
}

fn append_element(parent: &mut XmlElement, element: XmlElement) {
    if element.name == "mc:AlternateContent" {
        if let Some(fallback) = element.children.into_iter().find_map(|child| match child {
            XmlNode::Element(e) if e.name == "mc:Fallback" => Some(e),
            _ => None,
        }) {
            parent.children.extend(fallback.children);
        }
    } else {
        parent.children.push(XmlNode::Element(element));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_are_mapped_to_prefixes() {
        let root = parse(
            r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p w:rsid="1"><w:t xml:space="preserve"> a &amp; b </w:t></w:p></w:body></w:document>"#,
        )
        .unwrap();
        assert_eq!(root.name, "w:document");
        let p = root.first("w:body").unwrap().first("w:p").unwrap();
        assert_eq!(p.attr("w:rsid"), Some("1"));
        assert_eq!(p.first("w:t").unwrap().text(), " a & b ");
    }

    #[test]
    fn alternate_content_is_replaced_by_fallback() {
        let root = parse(
            r#"<w:r xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:AlternateContent><mc:Choice><w:t>choice</w:t></mc:Choice><mc:Fallback><w:t>fallback</w:t></mc:Fallback></mc:AlternateContent></w:r>"#,
        )
        .unwrap();
        let texts: Vec<_> = root.elements().map(|e| e.text()).collect();
        assert_eq!(texts, vec!["fallback"]);
    }

    #[test]
    fn unknown_namespaces_use_clark_notation() {
        let root = parse(r#"<x:a xmlns:x="urn:example"/>"#).unwrap();
        assert_eq!(root.name, "{urn:example}a");
    }

    #[test]
    fn byte_order_mark_is_ignored() {
        let root = parse("\u{feff}<a>b</a>").unwrap();
        assert_eq!(root.text(), "b");
    }
}
