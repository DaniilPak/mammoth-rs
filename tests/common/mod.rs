//! Helpers for building small `.docx` files in tests.
#![allow(dead_code)]

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

pub const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// Namespace declarations used by `document()` and friends.
pub const NAMESPACES: &str = concat!(
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" "#,
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" "#,
    r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" "#,
    r#"xmlns:v="urn:schemas-microsoft-com:vml" "#,
    r#"xmlns:o="urn:schemas-microsoft-com:office:office" "#,
    r#"xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml""#
);

const REL_PREFIX: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

#[derive(Default)]
pub struct DocxBuilder {
    body: String,
    styles: Option<String>,
    numbering: Option<String>,
    footnotes: Option<String>,
    endnotes: Option<String>,
    comments: Option<String>,
    relationships: Vec<(String, String, String, bool)>,
    files: Vec<(String, Vec<u8>)>,
}

impl DocxBuilder {
    pub fn new(body: &str) -> Self {
        DocxBuilder {
            body: body.to_string(),
            ..Default::default()
        }
    }

    /// `styles` is the inner XML of `w:styles`.
    pub fn styles(mut self, styles: &str) -> Self {
        self.styles = Some(styles.to_string());
        self
    }

    /// `numbering` is the inner XML of `w:numbering`.
    pub fn numbering(mut self, numbering: &str) -> Self {
        self.numbering = Some(numbering.to_string());
        self
    }

    /// `footnotes` is the inner XML of `w:footnotes`.
    pub fn footnotes(mut self, footnotes: &str) -> Self {
        self.footnotes = Some(footnotes.to_string());
        self
    }

    /// `endnotes` is the inner XML of `w:endnotes`.
    pub fn endnotes(mut self, endnotes: &str) -> Self {
        self.endnotes = Some(endnotes.to_string());
        self
    }

    /// `comments` is the inner XML of `w:comments`.
    pub fn comments(mut self, comments: &str) -> Self {
        self.comments = Some(comments.to_string());
        self
    }

    pub fn relationship(mut self, id: &str, kind: &str, target: &str, external: bool) -> Self {
        self.relationships
            .push((id.into(), kind.into(), target.into(), external));
        self
    }

    pub fn file(mut self, path: &str, data: &[u8]) -> Self {
        self.files.push((path.into(), data.to_vec()));
        self
    }

    pub fn build(self) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        let add = |zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>, name: &str, data: &[u8]| {
            zip.start_file(name, options).unwrap();
            zip.write_all(data).unwrap();
        };

        add(
            &mut zip,
            "[Content_Types].xml",
            br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="png" ContentType="image/png"/><Default Extension="emf" ContentType="image/x-emf"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        );
        add(
            &mut zip,
            "_rels/.rels",
            format!(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL_PREFIX}officeDocument" Target="word/document.xml"/></Relationships>"#).as_bytes(),
        );

        let mut rels = String::new();
        let part = |zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>,
                    rels: &mut String,
                    name: &str,
                    root: &str,
                    inner: &Option<String>| {
            if let Some(inner) = inner {
                add(zip, &format!("word/{name}.xml"), format!(r#"<?xml version="1.0" encoding="UTF-8"?><w:{root} {NAMESPACES}>{inner}</w:{root}>"#).as_bytes());
                rels.push_str(&format!(r#"<Relationship Id="rIdPart{name}" Type="{REL_PREFIX}{name}" Target="{name}.xml"/>"#));
            }
        };
        part(&mut zip, &mut rels, "styles", "styles", &self.styles);
        part(
            &mut zip,
            &mut rels,
            "numbering",
            "numbering",
            &self.numbering,
        );
        part(
            &mut zip,
            &mut rels,
            "footnotes",
            "footnotes",
            &self.footnotes,
        );
        part(&mut zip, &mut rels, "endnotes", "endnotes", &self.endnotes);
        part(&mut zip, &mut rels, "comments", "comments", &self.comments);
        for (id, kind, target, external) in &self.relationships {
            let mode = if *external {
                r#" TargetMode="External""#
            } else {
                ""
            };
            rels.push_str(&format!(
                r#"<Relationship Id="{id}" Type="{REL_PREFIX}{kind}" Target="{target}"{mode}/>"#
            ));
        }
        add(
            &mut zip,
            "word/_rels/document.xml.rels",
            format!(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#).as_bytes(),
        );
        add(
            &mut zip,
            "word/document.xml",
            format!(r#"<?xml version="1.0" encoding="UTF-8"?><w:document {NAMESPACES}><w:body>{}</w:body></w:document>"#, self.body).as_bytes(),
        );
        for (path, data) in &self.files {
            add(&mut zip, path, data);
        }
        zip.finish().unwrap().into_inner()
    }
}

/// `<w:p>` with optional paragraph properties and the given content.
pub fn p(properties: &str, content: &str) -> String {
    format!("<w:p><w:pPr>{properties}</w:pPr>{content}</w:p>")
}

/// `<w:r>` with optional run properties and text.
pub fn r(properties: &str, text: &str) -> String {
    format!(r#"<w:r><w:rPr>{properties}</w:rPr><w:t xml:space="preserve">{text}</w:t></w:r>"#)
}

pub fn style(kind: &str, id: &str, name: &str) -> String {
    format!(r#"<w:style w:type="{kind}" w:styleId="{id}"><w:name w:val="{name}"/></w:style>"#)
}

pub fn html(docx: &[u8]) -> String {
    mammoth_rs::convert_to_html(docx, &mammoth_rs::Options::new())
        .unwrap()
        .value
}

pub fn html_with(docx: &[u8], options: &mammoth_rs::Options) -> mammoth_rs::ConversionResult {
    mammoth_rs::convert_to_html(docx, options).unwrap()
}
