//! Reading `.docx` packages into the [`Document`](crate::Document) model.

mod body;
mod dingbats;
mod parts;

use std::cell::RefCell;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use crate::document::{Comment, Document, Note, NoteType};
use crate::error::{Error, Message, Result};
use crate::xml::{self, XmlElement};
use body::{BodyContext, BodyReader};
use parts::{ContentTypes, Numbering, Relationships, Styles};

const STYLE_MAP_PATH: &str = "mammoth/style-map";

/// Source of image bytes for the body reader.
pub(crate) trait FileSource {
    fn read_zip_entry(&self, name: &str) -> std::result::Result<Vec<u8>, String>;
    fn read_external(&self, uri: &str) -> std::result::Result<Vec<u8>, String>;
}

impl FileSource for PackageFiles<'_> {
    fn read_zip_entry(&self, name: &str) -> std::result::Result<Vec<u8>, String> {
        PackageFiles::read_zip_entry(self, name)
    }

    fn read_external(&self, uri: &str) -> std::result::Result<Vec<u8>, String> {
        PackageFiles::read_external(self, uri)
    }
}

/// Read access to the zip archive and, optionally, files next to it.
pub(crate) struct PackageFiles<'a> {
    archive: RefCell<ZipArchive<Cursor<&'a [u8]>>>,
    external_file_access: bool,
    base_dir: Option<PathBuf>,
}

impl<'a> PackageFiles<'a> {
    pub fn open(
        bytes: &'a [u8],
        external_file_access: bool,
        document_path: Option<&Path>,
    ) -> Result<Self> {
        let archive = ZipArchive::new(Cursor::new(bytes))?;
        Ok(PackageFiles {
            archive: RefCell::new(archive),
            external_file_access,
            base_dir: document_path
                .map(|path| path.parent().map(Path::to_path_buf).unwrap_or_default()),
        })
    }

    pub fn exists(&self, name: &str) -> bool {
        self.archive.borrow().index_for_name(name).is_some()
    }

    pub fn read_zip_entry(&self, name: &str) -> std::result::Result<Vec<u8>, String> {
        let mut archive = self.archive.borrow_mut();
        let mut file = archive
            .by_name(name)
            .map_err(|_| format!("could not find file in document: {name}"))?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|e| format!("could not read {name}: {e}"))?;
        Ok(data)
    }

    fn read_zip_string(&self, name: &str) -> Result<Option<String>> {
        if !self.exists(name) {
            return Ok(None);
        }
        let bytes = self.read_zip_entry(name).map_err(Error::InvalidDocument)?;
        Ok(Some(decode_text(&bytes)))
    }

    fn read_xml(&self, name: &str) -> Result<Option<XmlElement>> {
        match self.read_zip_string(name)? {
            None => Ok(None),
            Some(text) => xml::parse(&text).map(Some).map_err(|message| Error::Xml {
                part: name.to_string(),
                message,
            }),
        }
    }

    /// Reads an image stored outside the document (`r:link`).
    pub fn read_external(&self, uri: &str) -> std::result::Result<Vec<u8>, String> {
        if !self.external_file_access {
            return Err(format!(
                "could not read external image '{uri}', external file access is disabled"
            ));
        }
        let path = uri_to_path(uri)?;
        let path = if path.is_absolute() {
            path
        } else if let Some(base) = &self.base_dir {
            base.join(path)
        } else {
            return Err(format!(
                "could not find external image '{uri}', path of input document is unknown"
            ));
        };
        std::fs::read(&path).map_err(|error| {
            format!(
                "could not open external image: '{uri}' (document directory: '{}')\n{error}",
                self.base_dir
                    .as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            )
        })
    }
}

fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if bytes.starts_with(b"\xFF\xFE") || bytes.starts_with(b"\xFE\xFF") {
        let little_endian = bytes[0] == 0xFF;
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                if little_endian {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn uri_to_path(uri: &str) -> std::result::Result<PathBuf, String> {
    let error = || format!("Could not convert URI to path: {uri}");
    let path = if let Some(rest) = uri.strip_prefix("file://") {
        let (host, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, ""),
        };
        if !host.is_empty() && host != "localhost" {
            return Err(error());
        }
        path
    } else if uri.contains("://") || uri.starts_with("//") {
        return Err(error());
    } else {
        uri.split(['?', '#']).next().unwrap_or(uri)
    };
    let decoded = percent_decode(path);
    #[cfg(windows)]
    {
        let bytes = decoded.as_bytes();
        if bytes.len() > 2 && bytes[0] == b'/' && bytes[2] == b':' {
            return Ok(PathBuf::from(&decoded[1..]));
        }
    }
    Ok(PathBuf::from(decoded))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                output.push(byte);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(index) => (&path[..index], &path[index + 1..]),
        None => ("", path),
    }
}

fn join_path(parts: &[&str]) -> String {
    let mut relevant: Vec<&str> = Vec::new();
    for part in parts.iter().filter(|p| !p.is_empty()) {
        if part.starts_with('/') {
            relevant = vec![part];
        } else {
            relevant.push(part);
        }
    }
    relevant.join("/")
}

fn relationships_filename(path: &str) -> String {
    let (dirname, basename) = split_path(path);
    join_path(&[dirname, "_rels", &format!("{basename}.rels")])
}

struct PartPaths {
    main_document: String,
    comments: String,
    endnotes: String,
    footnotes: String,
    numbering: String,
    styles: String,
}

const RELATIONSHIP_TYPE_PREFIX: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const STRICT_RELATIONSHIP_TYPE_PREFIX: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/";

fn find_part_path(
    files: &PackageFiles<'_>,
    relationships: &Relationships,
    name: &str,
    base_path: &str,
    fallback_path: &str,
) -> String {
    for prefix in [RELATIONSHIP_TYPE_PREFIX, STRICT_RELATIONSHIP_TYPE_PREFIX] {
        for target in relationships.find_targets_by_type(&format!("{prefix}{name}")) {
            let joined = join_path(&[base_path, target]);
            let normalised = joined.strip_prefix('/').unwrap_or(&joined);
            if files.exists(normalised) {
                return normalised.to_string();
            }
        }
    }
    fallback_path.to_string()
}

fn read_relationships(files: &PackageFiles<'_>, path: &str) -> Result<Relationships> {
    Ok(files
        .read_xml(path)?
        .map(|root| Relationships::read(&root))
        .unwrap_or_default())
}

fn find_part_paths(files: &PackageFiles<'_>) -> Result<PartPaths> {
    let package_relationships = read_relationships(files, "_rels/.rels")?;
    let main_document = find_part_path(
        files,
        &package_relationships,
        "officeDocument",
        "",
        "word/document.xml",
    );
    if !files.exists(&main_document) {
        return Err(Error::InvalidDocument(
            "Could not find main document part. Are you sure this is a valid .docx file?"
                .to_string(),
        ));
    }
    let document_relationships =
        read_relationships(files, &relationships_filename(&main_document))?;
    let (dirname, _) = split_path(&main_document);
    let find = |name: &str| {
        find_part_path(
            files,
            &document_relationships,
            name,
            dirname,
            &format!("word/{name}.xml"),
        )
    };
    Ok(PartPaths {
        comments: find("comments"),
        endnotes: find("endnotes"),
        footnotes: find("footnotes"),
        numbering: find("numbering"),
        styles: find("styles"),
        main_document,
    })
}

struct Shared {
    content_types: ContentTypes,
    styles: Styles,
    numbering: Numbering,
}

/// Reads an XML part with a body reader configured with that part's
/// relationships.
fn read_part_with_body<T>(
    files: &PackageFiles<'_>,
    shared: &Shared,
    path: &str,
    read: impl FnOnce(&XmlElement, &BodyContext<'_>) -> Result<T>,
) -> Result<Option<T>> {
    let relationships = read_relationships(files, &relationships_filename(path))?;
    let Some(root) = files.read_xml(path)? else {
        return Ok(None);
    };
    let context = BodyContext {
        relationships: &relationships,
        content_types: &shared.content_types,
        files,
        numbering: &shared.numbering,
        styles: &shared.styles,
    };
    read(&root, &context).map(Some)
}

fn read_notes(
    root: &XmlElement,
    context: &BodyContext<'_>,
    note_type: NoteType,
    messages: &mut Vec<Message>,
) -> Vec<Note> {
    let tag = format!("w:{}", note_type.as_str());
    root.elements_named(&tag)
        .filter(|element| {
            !matches!(
                element.attr("w:type"),
                Some("continuationSeparator") | Some("separator")
            )
        })
        .map(|element| {
            let result = BodyReader::new(context).read_xml_elements(&element.children);
            messages.extend(result.messages);
            Note {
                note_type,
                note_id: element.attr("w:id").unwrap_or_default().to_string(),
                body: result.elements,
            }
        })
        .collect()
}

fn read_comments(
    root: &XmlElement,
    context: &BodyContext<'_>,
    messages: &mut Vec<Message>,
) -> Vec<Comment> {
    root.elements_named("w:comment")
        .map(|element| {
            let result = BodyReader::new(context).read_xml_elements(&element.children);
            messages.extend(result.messages);
            let optional = |name: &str| {
                element
                    .attr(name)
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            };
            Comment {
                comment_id: element.attr("w:id").unwrap_or_default().to_string(),
                body: result.elements,
                author_name: optional("w:author"),
                author_initials: optional("w:initials"),
            }
        })
        .collect()
}

/// Reads a whole `.docx` package into a [`Document`].
pub(crate) fn read(files: &PackageFiles<'_>) -> Result<(Document, Vec<Message>)> {
    let content_types = files
        .read_xml("[Content_Types].xml")?
        .map(|root| ContentTypes::read(&root))
        .unwrap_or_default();
    let part_paths = find_part_paths(files)?;
    let styles = files
        .read_xml(&part_paths.styles)?
        .map(|root| Styles::read(&root))
        .unwrap_or_default();
    let numbering = files
        .read_xml(&part_paths.numbering)?
        .map(|root| Numbering::read(&root, &styles))
        .unwrap_or_default();
    let shared = Shared {
        content_types,
        styles,
        numbering,
    };

    let mut messages = Vec::new();
    let footnotes = read_part_with_body(files, &shared, &part_paths.footnotes, |root, ctx| {
        Ok(read_notes(root, ctx, NoteType::Footnote, &mut messages))
    })?
    .unwrap_or_default();
    let endnotes = read_part_with_body(files, &shared, &part_paths.endnotes, |root, ctx| {
        Ok(read_notes(root, ctx, NoteType::Endnote, &mut messages))
    })?
    .unwrap_or_default();
    let comments = read_part_with_body(files, &shared, &part_paths.comments, |root, ctx| {
        Ok(read_comments(root, ctx, &mut messages))
    })?
    .unwrap_or_default();

    let mut document_messages = Vec::new();
    let children = read_part_with_body(files, &shared, &part_paths.main_document, |root, ctx| {
        let body = root.first("w:body").ok_or_else(|| {
            Error::InvalidDocument(
                "Could not find the body element: are you sure this is a docx file?".to_string(),
            )
        })?;
        let result = BodyReader::new(ctx).read_xml_elements(&body.children);
        document_messages = result.messages;
        Ok(result.elements)
    })?
    .unwrap_or_default();

    messages.extend(document_messages);
    let mut notes = footnotes;
    notes.extend(endnotes);
    Ok((
        Document {
            children,
            notes,
            comments,
        },
        messages,
    ))
}

/// Reads the style map embedded by `embed_style_map`, if any.
pub(crate) fn read_embedded_style_map(files: &PackageFiles<'_>) -> Result<Option<String>> {
    files.read_zip_string(STYLE_MAP_PATH)
}

/// Writes `style_map` into a copy of the `.docx` package.
pub(crate) fn embed_style_map(bytes: &[u8], style_map: &str) -> Result<Vec<u8>> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let rels_path = "word/_rels/document.xml.rels";
    let types_path = "[Content_Types].xml";
    let read_string = |archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str| -> Result<String> {
        let mut file = archive.by_name(name)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Ok(decode_text(&data))
    };
    let relationships = read_string(&mut archive, rels_path)?;
    let content_types = read_string(&mut archive, types_path)?;

    let relationships = upsert_xml_element(
        &relationships,
        "Relationship",
        "Id",
        "rMammothStyleMap",
        r#"<Relationship Id="rMammothStyleMap" Type="http://schemas.zwobble.org/mammoth/style-map" Target="/mammoth/style-map"/>"#,
        "</Relationships>",
    )?;
    let content_types = upsert_xml_element(
        &content_types,
        "Override",
        "PartName",
        "/mammoth/style-map",
        r#"<Override PartName="/mammoth/style-map" ContentType="text/prs.mammoth.style-map"/>"#,
        "</Types>",
    )?;

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for index in 0..archive.len() {
        let file = archive.by_index_raw(index)?;
        let name = file.name()?.to_string();
        if name == rels_path || name == types_path || name == STYLE_MAP_PATH {
            continue;
        }
        writer.raw_copy_file(file)?;
    }
    writer.start_file(rels_path, options)?;
    writer.write_all(relationships.as_bytes())?;
    writer.start_file(types_path, options)?;
    writer.write_all(content_types.as_bytes())?;
    writer.start_file(STYLE_MAP_PATH, options)?;
    writer.write_all(style_map.as_bytes())?;
    Ok(writer.finish()?.into_inner())
}

/// Replaces the element `<{tag} {key}="{value}" …/>` in `xml` with
/// `replacement`, or inserts `replacement` before `closing` if absent.
fn upsert_xml_element(
    xml: &str,
    tag: &str,
    key: &str,
    value: &str,
    replacement: &str,
    closing: &str,
) -> Result<String> {
    let needle_double = format!("{key}=\"{value}\"");
    let needle_single = format!("{key}='{value}'");
    let mut search_from = 0;
    while let Some(offset) = xml[search_from..].find(&format!("<{tag}")) {
        let start = search_from + offset;
        let Some(end_offset) = xml[start..].find('>') else {
            break;
        };
        let mut end = start + end_offset + 1;
        let opening = &xml[start..end];
        if opening.contains(&needle_double) || opening.contains(&needle_single) {
            if !opening.ends_with("/>") {
                let close_tag = format!("</{tag}>");
                if let Some(close) = xml[end..].find(&close_tag) {
                    end += close + close_tag.len();
                }
            }
            return Ok(format!("{}{}{}", &xml[..start], replacement, &xml[end..]));
        }
        search_from = end;
    }
    match xml.rfind(closing) {
        Some(index) => Ok(format!("{}{}{}", &xml[..index], replacement, &xml[index..])),
        None => Err(Error::InvalidDocument(format!(
            "could not find {closing} while embedding style map"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(
            relationships_filename("word/document.xml"),
            "word/_rels/document.xml.rels"
        );
        assert_eq!(
            relationships_filename("document.xml"),
            "_rels/document.xml.rels"
        );
        assert_eq!(join_path(&["word", "/media/a.png"]), "/media/a.png");
        assert_eq!(join_path(&["", "word/document.xml"]), "word/document.xml");
    }

    #[test]
    fn uris() {
        assert_eq!(uri_to_path("a%20b.png").unwrap(), PathBuf::from("a b.png"));
        assert_eq!(
            uri_to_path("file:///tmp/x.png").unwrap(),
            PathBuf::from("/tmp/x.png")
        );
        assert!(uri_to_path("http://example.com/x.png").is_err());
    }

    #[test]
    fn upsert() {
        let xml = r#"<Types><Override PartName="/a" ContentType="x"/></Types>"#;
        assert_eq!(
            upsert_xml_element(xml, "Override", "PartName", "/a", "<O/>", "</Types>").unwrap(),
            "<Types><O/></Types>"
        );
        assert_eq!(
            upsert_xml_element(xml, "Override", "PartName", "/b", "<O/>", "</Types>").unwrap(),
            r#"<Types><Override PartName="/a" ContentType="x"/><O/></Types>"#
        );
    }
}
