//! Helpers for [`Options::transform_document`](crate::Options::transform_document).
//!
//! ```
//! use mammoth_rs::{transforms, Options};
//!
//! // Treat centred paragraphs as headings.
//! let options = Options::new().transform_document(transforms::paragraph(|mut paragraph| {
//!     if paragraph.alignment.as_deref() == Some("center") && paragraph.style_id.is_none() {
//!         paragraph.style_id = Some("Heading2".to_string());
//!     }
//!     paragraph
//! }));
//! ```

use crate::document::{Document, Element, Paragraph, Run};

/// Applies `transform` to every element of the document body, children first.
pub fn elements<F>(transform: F) -> impl Fn(Document) -> Document + Send + Sync
where
    F: Fn(Element) -> Element + Send + Sync,
{
    move |mut document: Document| {
        let children = std::mem::take(&mut document.children);
        document.children = children
            .into_iter()
            .map(|element| transform_element(element, &transform))
            .collect();
        document
    }
}

fn transform_element<F: Fn(Element) -> Element>(mut element: Element, transform: &F) -> Element {
    if let Some(children) = element.children_mut() {
        let taken = std::mem::take(children);
        *children = taken
            .into_iter()
            .map(|child| transform_element(child, transform))
            .collect();
    }
    transform(element)
}

/// Applies `transform` to every paragraph in the document body.
pub fn paragraph<F>(transform: F) -> impl Fn(Document) -> Document + Send + Sync
where
    F: Fn(Paragraph) -> Paragraph + Send + Sync,
{
    elements(move |element| match element {
        Element::Paragraph(paragraph) => Element::Paragraph(transform(paragraph)),
        other => other,
    })
}

/// Applies `transform` to every run in the document body.
pub fn run<F>(transform: F) -> impl Fn(Document) -> Document + Send + Sync
where
    F: Fn(Run) -> Run + Send + Sync,
{
    elements(move |element| match element {
        Element::Run(run) => Element::Run(transform(run)),
        other => other,
    })
}

/// All descendants of `element` (not including `element` itself), children
/// before their parents.
pub fn get_descendants(element: &Element) -> Vec<&Element> {
    let mut descendants = Vec::new();
    collect_descendants(element, &mut descendants);
    descendants
}

fn collect_descendants<'a>(element: &'a Element, output: &mut Vec<&'a Element>) {
    if let Some(children) = element.children() {
        for child in children {
            collect_descendants(child, output);
            output.push(child);
        }
    }
}

pub(crate) fn raw_text(element: &Element, output: &mut String) {
    match element {
        Element::Text(text) => output.push_str(text),
        Element::Tab => output.push('\t'),
        other => {
            if let Some(children) = other.children() {
                for child in children {
                    raw_text(child, output);
                }
            }
            if matches!(other, Element::Paragraph(_)) {
                output.push_str("\n\n");
            }
        }
    }
}
