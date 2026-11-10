//! Integration tests ported from mammoth.js's `test/mammoth.tests.js`, using
//! its test documents (BSD-2-Clause, Copyright (c) 2013, Michael Williamson;
//! see `tests/fixtures/mammoth-js/LICENSE`).

use std::path::PathBuf;

use mammoth_rs::{
    ImageConverter, Message, MessageKind, Options, convert_to_html, convert_to_markdown,
    embed_style_map, extract_raw_text, read_embedded_style_map, transforms,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mammoth-js")
        .join(name)
}

const TINY_PICTURE_SRC: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAIAAAACUFjqAAAAAXNSR0IArs4c6QAAAAlwSFlzAAAOvgAADr4B6kKxwAAAABNJREFUKFNj/M+ADzDhlWUYqdIAQSwBE8U+X40AAAAASUVORK5CYII=";

fn simple_document() -> Vec<u8> {
    let xml = std::fs::read_to_string(fixture("simple/word/document.xml")).unwrap();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    std::io::Write::write_all(&mut zip, xml.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn docx_containing_one_paragraph_is_converted_to_single_p_element() {
    let result = convert_to_html(fixture("single-paragraph.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "<p>Walking on imported air</p>");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn docx_can_be_read_from_bytes() {
    let bytes = std::fs::read(fixture("single-paragraph.docx")).unwrap();
    let result = convert_to_html(&bytes, &Options::new()).unwrap();
    assert_eq!(result.value, "<p>Walking on imported air</p>");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn docx_xml_files_with_unicode_byte_order_mark_are_read() {
    let result = convert_to_html(fixture("utf8-bom.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "<p>This XML has a byte order mark.</p>");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn empty_paragraphs_are_ignored_by_default() {
    let result = convert_to_html(fixture("empty.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn empty_paragraphs_are_preserved_if_ignore_empty_paragraphs_is_false() {
    let options = Options::new().ignore_empty_paragraphs(false);
    let result = convert_to_html(fixture("empty.docx"), &options).unwrap();
    assert_eq!(result.value, "<p></p>");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn style_map_can_be_expressed_as_string() {
    let result = convert_to_html(simple_document(), &Options::new().style_map("p => h1")).unwrap();
    assert_eq!(result.value, "<h1>Hello.</h1>");
}

#[test]
fn style_map_can_be_expressed_as_individual_mappings() {
    let result =
        convert_to_html(simple_document(), &Options::new().style_mapping("p => h1")).unwrap();
    assert_eq!(result.value, "<h1>Hello.</h1>");
}

#[test]
fn embedded_style_map_is_used_if_present() {
    let result = convert_to_html(fixture("embedded-style-map.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "<h1>Walking on imported air</h1>");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn explicit_style_map_takes_precedence_over_embedded_style_map() {
    let options = Options::new().style_map("p => p");
    let result = convert_to_html(fixture("embedded-style-map.docx"), &options).unwrap();
    assert_eq!(result.value, "<p>Walking on imported air</p>");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn explicit_style_map_is_combined_with_embedded_style_map() {
    let options = Options::new().style_map("r => strong");
    let result = convert_to_html(fixture("embedded-style-map.docx"), &options).unwrap();
    assert_eq!(
        result.value,
        "<h1><strong>Walking on imported air</strong></h1>"
    );
    assert_eq!(result.messages, vec![]);
}

#[test]
fn embedded_style_maps_can_be_disabled() {
    let options = Options::new().include_embedded_style_map(false);
    let result = convert_to_html(fixture("embedded-style-map.docx"), &options).unwrap();
    assert_eq!(result.value, "<p>Walking on imported air</p>");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn embedded_style_map_can_be_written_and_then_read() {
    let docx = embed_style_map(fixture("single-paragraph.docx"), "p => h1").unwrap();
    let result = convert_to_html(&docx, &Options::new()).unwrap();
    assert_eq!(result.value, "<h1>Walking on imported air</h1>");
    assert_eq!(result.messages, vec![]);
    assert_eq!(
        read_embedded_style_map(&docx).unwrap().as_deref(),
        Some("p => h1")
    );
}

#[test]
fn embedding_a_style_map_twice_replaces_the_first() {
    let once = embed_style_map(fixture("single-paragraph.docx"), "p => h1").unwrap();
    let twice = embed_style_map(&once, "p => h2").unwrap();
    assert_eq!(
        convert_to_html(&twice, &Options::new()).unwrap().value,
        "<h2>Walking on imported air</h2>"
    );
}

#[test]
fn warning_if_style_mapping_is_not_understood() {
    let options = Options::new().style_map("????\np => h1");
    let result = convert_to_html(fixture("single-paragraph.docx"), &options).unwrap();
    assert_eq!(result.value, "<h1>Walking on imported air</h1>");
    assert_eq!(
        result.messages,
        vec![Message::warning(
            "Did not understand this style mapping, so ignored it: ????\n\
             Error was at character number 1: Expected element type but got unrecognisedCharacter \"?\""
        )]
    );
}

#[test]
fn transform_document_is_used_if_set() {
    let options = Options::new().transform_document(|mut document| {
        if let mammoth_rs::Element::Paragraph(paragraph) = &mut document.children[0] {
            paragraph.style_id = Some("Heading1".to_string());
        }
        document
    });
    let result = convert_to_html(simple_document(), &options).unwrap();
    assert_eq!(result.value, "<h1>Hello.</h1>");
}

#[test]
fn transforms_paragraph_only_transforms_paragraphs() {
    let options = Options::new().transform_document(transforms::paragraph(|mut paragraph| {
        paragraph.style_id = Some("Heading1".to_string());
        paragraph
    }));
    let result = convert_to_html(simple_document(), &options).unwrap();
    assert_eq!(result.value, "<h1>Hello.</h1>");
}

#[test]
fn inline_images_referenced_by_path_relative_to_part_are_included_in_output() {
    let result = convert_to_html(fixture("tiny-picture.docx"), &Options::new()).unwrap();
    assert_eq!(
        result.value,
        format!("<p><img src=\"{TINY_PICTURE_SRC}\" /></p>")
    );
}

#[test]
fn inline_images_referenced_by_path_relative_to_base_are_included_in_output() {
    let result = convert_to_html(
        fixture("tiny-picture-target-base-relative.docx"),
        &Options::new(),
    )
    .unwrap();
    assert_eq!(
        result.value,
        format!("<p><img src=\"{TINY_PICTURE_SRC}\" /></p>")
    );
}

#[test]
fn src_of_inline_images_can_be_changed() {
    let options = Options::new().image_converter(ImageConverter::img_element(|image| {
        let encoded = image.read_as_base64()?;
        Ok(vec![(
            "src".to_string(),
            format!(
                "{},{}",
                &encoded[..2],
                image.content_type.as_deref().unwrap_or("")
            ),
        )])
    }));
    let result = convert_to_html(fixture("tiny-picture.docx"), &options).unwrap();
    assert_eq!(result.messages, vec![]);
    assert_eq!(result.value, "<p><img src=\"iV,image/png\" /></p>");
}

#[test]
fn when_external_file_access_is_enabled_then_images_stored_outside_of_document_are_included() {
    let options = Options::new().external_file_access(true);
    let result = convert_to_html(fixture("external-picture.docx"), &options).unwrap();
    assert_eq!(
        result.value,
        format!("<p><img src=\"{TINY_PICTURE_SRC}\" /></p>")
    );
    assert_eq!(result.messages, vec![]);
}

#[test]
fn external_images_are_an_error_when_document_path_is_unknown() {
    let bytes = std::fs::read(fixture("external-picture.docx")).unwrap();
    let options = Options::new().external_file_access(true);
    let result = convert_to_html(&bytes, &options).unwrap();
    assert_eq!(result.value, "");
    assert_eq!(
        result.messages[0],
        Message::error(
            "could not find external image 'tiny-picture.png', path of input document is unknown"
        )
    );
}

#[test]
fn external_images_are_an_error_when_external_file_access_is_disabled() {
    let result = convert_to_html(fixture("external-picture.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "");
    assert_eq!(result.messages[0].kind, MessageKind::Error);
    assert_eq!(
        result.messages[0].message,
        "could not read external image 'tiny-picture.png', external file access is disabled"
    );
}

#[test]
fn simple_list_is_converted_to_list_elements() {
    let result = convert_to_html(fixture("simple-list.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "<ul><li>Apple</li><li>Banana</li></ul>");
}

#[test]
fn word_tables_are_converted_to_html_tables() {
    let result = convert_to_html(fixture("tables.docx"), &Options::new()).unwrap();
    assert_eq!(
        result.value,
        "<p>Above</p>\
         <table>\
         <tr><td><p>Top left</p></td><td><p>Top right</p></td></tr>\
         <tr><td><p>Bottom left</p></td><td><p>Bottom right</p></td></tr>\
         </table>\
         <p>Below</p>"
    );
    assert_eq!(result.messages, vec![]);
}

#[test]
fn footnotes_are_appended_to_text() {
    let options = Options::new().id_prefix("doc-42-");
    let result = convert_to_html(fixture("footnotes.docx"), &options).unwrap();
    assert_eq!(
        result.value,
        "<p>Ouch\
         <sup><a href=\"#doc-42-footnote-1\" id=\"doc-42-footnote-ref-1\">[1]</a></sup>.\
         <sup><a href=\"#doc-42-footnote-2\" id=\"doc-42-footnote-ref-2\">[2]</a></sup></p>\
         <ol><li id=\"doc-42-footnote-1\"><p> A tachyon walks into a bar. <a href=\"#doc-42-footnote-ref-1\">↑</a></p></li>\
         <li id=\"doc-42-footnote-2\"><p> Fin. <a href=\"#doc-42-footnote-ref-2\">↑</a></p></li></ol>"
    );
    assert_eq!(result.messages, vec![]);
}

#[test]
fn endnotes_are_appended_to_text() {
    let options = Options::new().id_prefix("doc-42-");
    let result = convert_to_html(fixture("endnotes.docx"), &options).unwrap();
    assert_eq!(
        result.value,
        "<p>Ouch\
         <sup><a href=\"#doc-42-endnote-2\" id=\"doc-42-endnote-ref-2\">[1]</a></sup>.\
         <sup><a href=\"#doc-42-endnote-3\" id=\"doc-42-endnote-ref-3\">[2]</a></sup></p>\
         <ol><li id=\"doc-42-endnote-2\"><p> A tachyon walks into a bar. <a href=\"#doc-42-endnote-ref-2\">↑</a></p></li>\
         <li id=\"doc-42-endnote-3\"><p> Fin. <a href=\"#doc-42-endnote-ref-3\">↑</a></p></li></ol>"
    );
    assert_eq!(result.messages, vec![]);
}

#[test]
fn relationships_are_handled_properly_in_footnotes() {
    let options = Options::new().id_prefix("doc-42-");
    let result = convert_to_html(fixture("footnote-hyperlink.docx"), &options).unwrap();
    assert_eq!(
        result.value,
        "<p><sup><a href=\"#doc-42-footnote-1\" id=\"doc-42-footnote-ref-1\">[1]</a></sup></p>\
         <ol><li id=\"doc-42-footnote-1\"><p> <a href=\"http://www.example.com\">Example</a> <a href=\"#doc-42-footnote-ref-1\">↑</a></p></li></ol>"
    );
    assert_eq!(result.messages, vec![]);
}

#[test]
fn when_style_mapping_is_defined_for_comment_references_then_comments_are_included() {
    let options = Options::new()
        .id_prefix("doc-42-")
        .style_map("comment-reference => sup");
    let result = convert_to_html(fixture("comments.docx"), &options).unwrap();
    assert_eq!(
        result.value,
        "<p>Ouch\
         <sup><a href=\"#doc-42-comment-0\" id=\"doc-42-comment-ref-0\">[MW1]</a></sup>.\
         <sup><a href=\"#doc-42-comment-2\" id=\"doc-42-comment-ref-2\">[MW2]</a></sup></p>\
         <dl><dt id=\"doc-42-comment-0\">Comment [MW1]</dt><dd><p>A tachyon walks into a bar. <a href=\"#doc-42-comment-ref-0\">↑</a></p></dd>\
         <dt id=\"doc-42-comment-2\">Comment [MW2]</dt><dd><p>Fin. <a href=\"#doc-42-comment-ref-2\">↑</a></p></dd></dl>"
    );
    assert_eq!(result.messages, vec![]);
}

#[test]
fn comments_are_ignored_by_default() {
    let result = convert_to_html(fixture("comments.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "<p>Ouch.</p>");
}

#[test]
fn textboxes_are_read() {
    let result = convert_to_html(fixture("text-box.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "<p>Datum plane</p>");
}

#[test]
fn underline_is_ignored_by_default() {
    let result = convert_to_html(fixture("underline.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "<p><strong>The Sunset Tree</strong></p>");
}

#[test]
fn underline_can_be_configured_with_style_mapping() {
    let options = Options::new().style_map("u => em");
    let result = convert_to_html(fixture("underline.docx"), &options).unwrap();
    assert_eq!(
        result.value,
        "<p><strong>The <em>Sunset</em> Tree</strong></p>"
    );
}

#[test]
fn strikethrough_is_converted_to_s_by_default() {
    let result = convert_to_html(fixture("strikethrough.docx"), &Options::new()).unwrap();
    assert_eq!(
        result.value,
        "<p><s>Today's Special: Salmon</s> Sold out</p>"
    );
}

#[test]
fn strikethrough_conversion_can_be_configured_with_style_mappings() {
    let options = Options::new().style_map("strike => del");
    let result = convert_to_html(fixture("strikethrough.docx"), &options).unwrap();
    assert_eq!(
        result.value,
        "<p><del>Today's Special: Salmon</del> Sold out</p>"
    );
}

#[test]
fn indentation_is_used_if_pretty_print_is_true() {
    let options = Options::new().pretty_print(true);
    let result = convert_to_html(fixture("single-paragraph.docx"), &options).unwrap();
    assert_eq!(result.value, "<p>\n  Walking on imported air\n</p>");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn single_paragraph_can_be_converted_to_markdown() {
    let result = convert_to_markdown(fixture("single-paragraph.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "Walking on imported air\n\n");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn extract_raw_text_only_retains_raw_text() {
    let result = extract_raw_text(fixture("simple-list.docx")).unwrap();
    assert_eq!(result.value, "Apple\n\nBanana\n\n");
}

#[test]
fn extract_raw_text_can_use_bytes() {
    let bytes = std::fs::read(fixture("single-paragraph.docx")).unwrap();
    let result = extract_raw_text(&bytes).unwrap();
    assert_eq!(result.value, "Walking on imported air\n\n");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn strict_format_can_be_read() {
    let result = convert_to_html(fixture("strict-format.docx"), &Options::new()).unwrap();
    assert_eq!(result.value, "<p>Test</p>");
    assert_eq!(result.messages, vec![]);
}

#[test]
fn error_if_file_is_not_a_valid_docx_document() {
    let error = convert_to_html(fixture("empty.zip"), &Options::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Could not find main document part. Are you sure this is a valid .docx file?"
    );
}

#[test]
fn error_if_file_is_not_a_zip() {
    assert!(convert_to_html(&b"not a zip"[..], &Options::new()).is_err());
    assert!(matches!(
        convert_to_html("does-not-exist.docx", &Options::new()),
        Err(mammoth_rs::Error::Io(_))
    ));
}
