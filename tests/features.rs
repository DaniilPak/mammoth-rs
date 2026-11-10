//! Feature tests using small generated `.docx` files.

mod common;

use common::{DocxBuilder, html, html_with, p, r, style};
use mammoth_rs::{
    Element, ImageConverter, Message, MessageKind, Options, convert_to_markdown, extract_raw_text,
    read_document,
};

const PNG: &[u8] = include_bytes!("fixtures/mammoth-js/tiny-picture.png");

fn style_p(id: &str) -> String {
    format!(r#"<w:pStyle w:val="{id}"/>"#)
}

// -- Headings and paragraph styles -------------------------------------------

#[test]
fn headings_are_mapped_by_style_id_and_name() {
    let mut body = String::new();
    let mut styles = String::new();
    for level in 1..=6 {
        body.push_str(&p(
            &style_p(&format!("Heading{level}")),
            &r("", &format!("H{level}")),
        ));
        styles.push_str(&style(
            "paragraph",
            &format!("Heading{level}"),
            &format!("heading {level}"),
        ));
    }
    // A heading style with a non-standard ID but standard name.
    body.push_str(&p(&style_p("Titre1"), &r("", "Named")));
    styles.push_str(&style("paragraph", "Titre1", "Heading 1"));
    let docx = DocxBuilder::new(&body).styles(&styles).build();
    assert_eq!(
        html(&docx),
        "<h1>H1</h1><h2>H2</h2><h3>H3</h3><h4>H4</h4><h5>H5</h5><h6>H6</h6><h1>Named</h1>"
    );
}

#[test]
fn custom_style_maps_take_precedence_and_style_names_are_case_insensitive() {
    let docx = DocxBuilder::new(&p(&style_p("SectionTitle"), &r("", "Intro")))
        .styles(&style("paragraph", "SectionTitle", "Section Title"))
        .build();
    let options = Options::new().style_map("p[style-name='section title'] => h2.section:fresh");
    let result = html_with(&docx, &options);
    assert_eq!(result.value, r#"<h2 class="section">Intro</h2>"#);
    assert_eq!(result.messages, vec![]);
}

#[test]
fn style_name_prefix_matcher() {
    let docx = DocxBuilder::new(&p(&style_p("Q"), &r("", "Quote")))
        .styles(&style("paragraph", "Q", "Quote Intense"))
        .build();
    let options = Options::new().style_map("p[style-name^='Quote'] => blockquote:fresh");
    assert_eq!(
        html_with(&docx, &options).value,
        "<blockquote>Quote</blockquote>"
    );
}

#[test]
fn unrecognised_styles_produce_warnings() {
    let body = p(
        &style_p("Fancy"),
        r#"<w:r><w:rPr><w:rStyle w:val="Glow"/></w:rPr><w:t>x</w:t></w:r>"#,
    );
    let docx = DocxBuilder::new(&body)
        .styles(
            &(style("paragraph", "Fancy", "Fancy Para") + &style("character", "Glow", "Glow Run")),
        )
        .build();
    let result = html_with(&docx, &Options::new());
    assert_eq!(result.value, "<p>x</p>");
    assert_eq!(
        result.messages,
        vec![
            Message::warning("Unrecognised paragraph style: 'Fancy Para' (Style ID: Fancy)"),
            Message::warning("Unrecognised run style: 'Glow Run' (Style ID: Glow)"),
        ]
    );
}

#[test]
fn undefined_styles_produce_warnings() {
    let docx = DocxBuilder::new(&p(&style_p("Missing"), &r("", "x"))).build();
    let result = html_with(&docx, &Options::new());
    assert_eq!(
        result.messages,
        vec![
            Message::warning(
                "Paragraph style with ID Missing was referenced but not defined in the document"
            ),
            Message::warning("Unrecognised paragraph style: 'null' (Style ID: Missing)"),
        ]
    );
}

#[test]
fn default_style_map_can_be_disabled() {
    let docx = DocxBuilder::new(&p(&style_p("Heading1"), &r("", "x")))
        .styles(&style("paragraph", "Heading1", "heading 1"))
        .build();
    let options = Options::new().include_default_style_map(false);
    assert_eq!(html_with(&docx, &options).value, "<p>x</p>");
}

#[test]
fn stacked_paths_and_separators() {
    let body = p(&style_p("Code"), &r("", "a = 1")) + &p(&style_p("Code"), &r("", "b = 2"));
    let docx = DocxBuilder::new(&body)
        .styles(&style("paragraph", "Code", "Code Block"))
        .build();
    let options = Options::new().style_map("p[style-name='Code Block'] => pre:separator('\\n')");
    assert_eq!(html_with(&docx, &options).value, "<pre>a = 1\nb = 2</pre>");

    let options = Options::new().style_map("p[style-name='Code Block'] => div.code > p:fresh");
    assert_eq!(
        html_with(&docx, &options).value,
        r#"<div class="code"><p>a = 1</p><p>b = 2</p></div>"#
    );
}

#[test]
fn ignore_path_drops_content() {
    let body = p(&style_p("Hidden"), &r("", "secret")) + &p("", &r("", "shown"));
    let docx = DocxBuilder::new(&body)
        .styles(&style("paragraph", "Hidden", "Hidden"))
        .build();
    let options = Options::new().style_map("p[style-name='Hidden'] => !");
    assert_eq!(html_with(&docx, &options).value, "<p>shown</p>");
}

// -- Run formatting ------------------------------------------------------------

#[test]
fn run_formatting() {
    let body = p(
        "",
        &[
            r("<w:b/>", "bold"),
            r("<w:i/>", "italic"),
            r("<w:u w:val=\"single\"/>", "underline"),
            r("<w:strike/>", "strike"),
            r("<w:vertAlign w:val=\"superscript\"/>", "sup"),
            r("<w:vertAlign w:val=\"subscript\"/>", "sub"),
            r("<w:b/><w:i/>", "both"),
            r("<w:b w:val=\"false\"/>", "notbold"),
            r("<w:b w:val=\"0\"/>", "notbold2"),
        ]
        .concat(),
    );
    let docx = DocxBuilder::new(&body).build();
    assert_eq!(
        html(&docx),
        "<p><strong>bold</strong><em>italic</em>underline<s>strike</s><sup>sup</sup><sub>sub</sub>\
         <strong><em>both</em></strong>notboldnotbold2</p>"
    );
}

#[test]
fn consecutive_formatted_runs_are_merged() {
    let docx = DocxBuilder::new(&p("", &(r("<w:b/>", "Hello") + &r("<w:b/>", " world")))).build();
    assert_eq!(html(&docx), "<p><strong>Hello world</strong></p>");
}

#[test]
fn run_property_style_mappings() {
    let body = p(
        "",
        &[
            r("<w:u w:val=\"single\"/>", "u"),
            r("<w:caps/>", "caps"),
            r("<w:smallCaps/>", "sc"),
            r("<w:highlight w:val=\"yellow\"/>", "hl"),
            r("<w:highlight w:val=\"green\"/>", "hl2"),
            r("<w:b/>", "b"),
        ]
        .concat(),
    );
    let docx = DocxBuilder::new(&body).build();
    let options = Options::new().style_map(
        "u => u\nall-caps => span.caps\nsmall-caps => span.sc\nhighlight[color='yellow'] => mark\nb => b",
    );
    assert_eq!(
        html_with(&docx, &options).value,
        r#"<p><u>u</u><span class="caps">caps</span><span class="sc">sc</span><mark>hl</mark>hl2<b>b</b></p>"#
    );
}

#[test]
fn character_styles_can_be_mapped() {
    let body = p(
        "",
        r#"<w:r><w:rPr><w:rStyle w:val="InlineCode"/></w:rPr><w:t>x()</w:t></w:r>"#,
    );
    let docx = DocxBuilder::new(&body)
        .styles(&style("character", "InlineCode", "Inline Code"))
        .build();
    let options = Options::new().style_map("r[style-name='Inline Code'] => code");
    assert_eq!(html_with(&docx, &options).value, "<p><code>x()</code></p>");
}

#[test]
fn text_is_html_escaped() {
    let docx = DocxBuilder::new(&p("", &r("", "1 &lt; 2 &amp;&amp; \"q\""))).build();
    assert_eq!(html(&docx), "<p>1 &lt; 2 &amp;&amp; \"q\"</p>");
}

#[test]
fn tabs_breaks_and_special_characters() {
    let body = p(
        "",
        r#"<w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t><w:br w:type="page"/><w:t>d</w:t><w:noBreakHyphen/><w:softHyphen/><w:sym w:font="Symbol" w:char="F0B7"/><w:sym w:font="Wingdings" w:char="F0FC"/></w:r>"#,
    );
    let docx = DocxBuilder::new(&body).build();
    assert_eq!(
        html(&docx),
        "<p>a\tb<br />cd\u{2011}\u{ad}\u{2022}\u{2713}</p>"
    );
}

#[test]
fn breaks_can_be_mapped() {
    let body = p(
        "",
        r#"<w:r><w:t>a</w:t><w:br w:type="page"/><w:t>b</w:t></w:r>"#,
    );
    let docx = DocxBuilder::new(&body).build();
    let options = Options::new().style_map("br[type='page'] => hr");
    assert_eq!(html_with(&docx, &options).value, "<p>a<hr />b</p>");
}

#[test]
fn unsupported_symbol_warns() {
    let body = p("", r#"<w:r><w:sym w:font="Nope" w:char="F041"/></w:r>"#);
    let result = html_with(&DocxBuilder::new(&body).build(), &Options::new());
    assert_eq!(
        result.messages,
        vec![Message::warning(
            "A w:sym element with an unsupported character was ignored: char F041 in font Nope"
        )]
    );
}

// -- Hyperlinks and bookmarks --------------------------------------------------

#[test]
fn hyperlinks() {
    let body = p(
        "",
        &format!(
            r#"<w:hyperlink r:id="rIdLink">{}</w:hyperlink> <w:hyperlink w:anchor="sec" w:tgtFrame="_blank">{}</w:hyperlink><w:hyperlink r:id="rIdLink" w:anchor="frag">{}</w:hyperlink><w:bookmarkStart w:id="0" w:name="sec"/><w:bookmarkStart w:id="1" w:name="_GoBack"/>"#,
            r(r#"<w:rStyle w:val="Hyperlink"/>"#, "ext"),
            r("", "int"),
            r("", "frag")
        ),
    );
    let docx = DocxBuilder::new(&body)
        .styles(&style("character", "Hyperlink", "Hyperlink"))
        .relationship("rIdLink", "hyperlink", "http://example.com/a#old", true)
        .build();
    let result = html_with(&docx, &Options::new().id_prefix("x-"));
    assert_eq!(
        result.value,
        r##"<p><a href="http://example.com/a#old">ext</a><a href="#x-sec" target="_blank">int</a><a href="http://example.com/a#frag">frag</a><a id="x-sec"></a></p>"##
    );
    assert_eq!(result.messages, vec![]);
}

#[test]
fn complex_field_hyperlinks() {
    let body = p(
        "",
        r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> HYPERLINK "http://example.com" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>link</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t> after</w:t></w:r>"#,
    );
    let docx = DocxBuilder::new(&body).build();
    assert_eq!(
        html(&docx),
        r#"<p><a href="http://example.com">link</a> after</p>"#
    );
}

#[test]
fn checkboxes() {
    let body = p(
        "",
        r#"<w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:checked/></w:checkBox></w:ffData></w:fldChar></w:r><w:r><w:instrText>FORMCHECKBOX</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
    ) + &format!(
        r#"<w:sdt><w:sdtPr><w14:checkbox><w14:checked w14:val="0"/></w14:checkbox></w:sdtPr><w:sdtContent>{}</w:sdtContent></w:sdt>"#,
        p("", &r("", "☐"))
    );
    let docx = DocxBuilder::new(&body).build();
    assert_eq!(
        html(&docx),
        r#"<p><input type="checkbox" checked="checked" /></p><p><input type="checkbox" /></p>"#
    );
}

// -- Lists -------------------------------------------------------------------------

const NUMBERING: &str = r#"
<w:abstractNum w:abstractNumId="0">
  <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
  <w:lvl w:ilvl="1"><w:numFmt w:val="decimal"/></w:lvl>
  <w:lvl w:ilvl="2"><w:numFmt w:val="bullet"/></w:lvl>
</w:abstractNum>
<w:abstractNum w:abstractNumId="1">
  <w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:pStyle w:val="ListNumber"/></w:lvl>
</w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
"#;

fn list_item(num_id: &str, level: u32, text: &str) -> String {
    p(
        &format!(r#"<w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="{num_id}"/></w:numPr>"#),
        &r("", text),
    )
}

#[test]
fn nested_lists() {
    let body = [
        list_item("1", 0, "Fruit"),
        list_item("1", 1, "Apple"),
        list_item("1", 1, "Banana"),
        list_item("1", 2, "Ripe"),
        list_item("1", 0, "Veg"),
        p("", &r("", "After")),
        list_item("2", 0, "One"),
        list_item("2", 0, "Two"),
    ]
    .concat();
    let docx = DocxBuilder::new(&body).numbering(NUMBERING).build();
    assert_eq!(
        html(&docx),
        "<ul><li>Fruit<ol><li>Apple</li><li>Banana<ul><li>Ripe</li></ul></li></ol></li><li>Veg</li></ul>\
         <p>After</p><ol><li>One</li><li>Two</li></ol>"
    );
}

#[test]
fn list_numbering_can_come_from_paragraph_style() {
    let body =
        p(&style_p("ListNumber"), &r("", "styled")) + &p(&style_p("ListNumber"), &r("", "item"));
    let docx = DocxBuilder::new(&body)
        .numbering(NUMBERING)
        .styles(&style("paragraph", "ListNumber", "List Number"))
        .build();
    let result = html_with(&docx, &Options::new());
    assert_eq!(result.value, "<ol><li>styled</li><li>item</li></ol>");
}

#[test]
fn lists_can_be_mapped_explicitly() {
    let body = list_item("1", 0, "a") + &list_item("1", 0, "b");
    let docx = DocxBuilder::new(&body).numbering(NUMBERING).build();
    let options = Options::new().style_map("p:unordered-list(1) => ul.dashes > li:fresh");
    assert_eq!(
        html_with(&docx, &options).value,
        r#"<ul class="dashes"><li>a</li><li>b</li></ul>"#
    );
}

#[test]
fn lists_in_markdown() {
    let body = [
        list_item("1", 0, "Fruit"),
        list_item("1", 1, "Apple"),
        list_item("1", 1, "Banana"),
        list_item("1", 0, "Veg"),
    ]
    .concat();
    let docx = DocxBuilder::new(&body).numbering(NUMBERING).build();
    let result = convert_to_markdown(&docx, &Options::new()).unwrap();
    assert_eq!(result.value, "- Fruit\n\t1. Apple\n\t2. Banana\n- Veg\n\n");
}

// -- Tables --------------------------------------------------------------------

fn cell(properties: &str, text: &str) -> String {
    format!(
        "<w:tc><w:tcPr>{properties}</w:tcPr>{}</w:tc>",
        p("", &r("", text))
    )
}

#[test]
fn tables_with_header_rows_and_merged_cells() {
    let body = format!(
        "<w:tbl><w:tblPr/><w:tblGrid/>\
         <w:tr><w:trPr><w:tblHeader/></w:trPr>{}{}</w:tr>\
         <w:tr>{}{}{}</w:tr>\
         <w:tr>{}{}{}</w:tr>\
         <w:tr><w:trPr><w:del/></w:trPr>{}</w:tr>\
         </w:tbl>",
        cell(r#"<w:gridSpan w:val="2"/>"#, "Wide"),
        cell("", "H3"),
        cell(r#"<w:vMerge w:val="restart"/>"#, "Tall"),
        cell("", "B"),
        cell("", "C"),
        cell("<w:vMerge/>", ""),
        cell("", "E"),
        cell("", ""),
        cell("", "deleted"),
    );
    let docx = DocxBuilder::new(&body).build();
    assert_eq!(
        html(&docx),
        "<table><thead><tr><th colspan=\"2\"><p>Wide</p></th><th><p>H3</p></th></tr></thead>\
         <tbody><tr><td rowspan=\"2\"><p>Tall</p></td><td><p>B</p></td><td><p>C</p></td></tr>\
         <tr><td><p>E</p></td><td></td></tr></tbody></table>"
    );
}

#[test]
fn table_styles_can_be_mapped() {
    let body = format!(
        r#"<w:tbl><w:tblPr><w:tblStyle w:val="Grid"/></w:tblPr><w:tr>{}</w:tr></w:tbl>"#,
        cell("", "x")
    );
    let docx = DocxBuilder::new(&body)
        .styles(&style("table", "Grid", "Table Grid"))
        .build();
    let options = Options::new().style_map("table[style-name='Table Grid'] => table.grid:fresh");
    assert_eq!(
        html_with(&docx, &options).value,
        r#"<table class="grid"><tr><td><p>x</p></td></tr></table>"#
    );
}

#[test]
fn raw_text_of_tables() {
    let body = format!(
        "<w:tbl><w:tr>{}{}</w:tr></w:tbl>",
        cell("", "a"),
        cell("", "b")
    );
    let docx = DocxBuilder::new(&body).build();
    assert_eq!(extract_raw_text(&docx).unwrap().value, "a\n\nb\n\n");
}

// -- Footnotes, endnotes and comments ------------------------------------------

#[test]
fn footnotes_and_endnotes_from_generated_document() {
    let body = p(
        "",
        r#"<w:r><w:t>Text</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:endnoteReference w:id="1"/></w:r>"#,
    );
    let docx = DocxBuilder::new(&body)
        .footnotes(&format!(
            r#"<w:footnote w:type="separator" w:id="-1">{}</w:footnote><w:footnote w:id="1">{}</w:footnote>"#,
            p("", &r("", "sep")),
            p("", &r("", "Foot"))
        ))
        .endnotes(&format!(r#"<w:endnote w:id="1">{}</w:endnote>"#, p("", &r("", "End"))))
        .build();
    assert_eq!(
        html(&docx),
        "<p>Text<sup><a href=\"#footnote-1\" id=\"footnote-ref-1\">[1]</a></sup><sup><a href=\"#endnote-1\" id=\"endnote-ref-1\">[2]</a></sup></p>\
         <ol><li id=\"footnote-1\"><p>Foot <a href=\"#footnote-ref-1\">↑</a></p></li>\
         <li id=\"endnote-1\"><p>End <a href=\"#endnote-ref-1\">↑</a></p></li></ol>"
    );
}

#[test]
fn comments_without_initials() {
    let body = p(
        "",
        r#"<w:r><w:t>Hi</w:t></w:r><w:r><w:commentReference w:id="7"/></w:r>"#,
    );
    let docx = DocxBuilder::new(&body)
        .comments(&format!(
            r#"<w:comment w:id="7" w:author="Ann">{}</w:comment>"#,
            p("", &r("", "Note"))
        ))
        .build();
    let options = Options::new().style_map("comment-reference => sup");
    assert_eq!(
        html_with(&docx, &options).value,
        "<p>Hi<sup><a href=\"#comment-7\" id=\"comment-ref-7\">[1]</a></sup></p>\
         <dl><dt id=\"comment-7\">Comment [1]</dt><dd><p>Note <a href=\"#comment-ref-7\">↑</a></p></dd></dl>"
    );
    let (document, _) = read_document(&docx, &Options::new()).unwrap();
    assert_eq!(document.comments[0].author_name.as_deref(), Some("Ann"));
    assert_eq!(document.comments[0].author_initials, None);
}

// -- Images --------------------------------------------------------------------

fn drawing(descr: &str, extra_doc_pr: &str) -> String {
    format!(
        r#"<w:r><w:drawing><wp:inline><wp:docPr id="1" name="Picture 1" descr="{descr}" title="Title">{extra_doc_pr}</wp:docPr><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImg"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
    )
}

#[test]
fn images_have_alt_text_and_can_be_linked() {
    let body = p("", &drawing("A tiny picture", ""))
        + &p("", &drawing(" ", r#"<a:hlinkClick r:id="rIdLink"/>"#));
    let docx = DocxBuilder::new(&body)
        .relationship("rIdImg", "image", "media/pic.png", false)
        .relationship("rIdLink", "hyperlink", "http://example.com", true)
        .file("word/media/pic.png", PNG)
        .build();
    let options = Options::new().image_converter(ImageConverter::img_element(|image| {
        Ok(vec![(
            "src".into(),
            format!("{}-bytes", image.read()?.len()),
        )])
    }));
    let result = html_with(&docx, &options);
    let size = PNG.len();
    assert_eq!(
        result.value,
        format!(
            r#"<p><img alt="A tiny picture" src="{size}-bytes" /></p><p><a href="http://example.com"><img alt="Title" src="{size}-bytes" /></a></p>"#
        )
    );
    assert_eq!(result.messages, vec![]);
}

#[test]
fn vml_images_are_read() {
    let body = p(
        "",
        r#"<w:r><w:pict><v:shape><v:imagedata r:id="rIdImg" o:title="Legacy"/></v:shape></w:pict></w:r>"#,
    );
    let docx = DocxBuilder::new(&body)
        .relationship("rIdImg", "image", "media/pic.png", false)
        .file("word/media/pic.png", PNG)
        .build();
    // As in mammoth.js, the contents of w:pict are placed after the paragraph.
    let value = html(&docx);
    assert!(
        value.starts_with(r#"<img alt="Legacy" src="data:image/png;base64,iVBOR"#),
        "{value}"
    );
}

#[test]
fn image_converter_errors_become_messages() {
    let docx = DocxBuilder::new(&p("", &drawing("x", "")))
        .relationship("rIdImg", "image", "media/pic.png", false)
        .file("word/media/pic.png", PNG)
        .build();
    let options =
        Options::new().image_converter(ImageConverter::img_element(|_| Err("boom".into())));
    let result = html_with(&docx, &options);
    assert_eq!(result.value, "");
    assert_eq!(result.messages, vec![Message::error("boom")]);
}

#[test]
fn unusual_image_types_warn() {
    let docx = DocxBuilder::new(&p("", &drawing("x", "")))
        .relationship("rIdImg", "image", "media/pic.emf", false)
        .file("word/media/pic.emf", b"emf")
        .build();
    let result = html_with(&docx, &Options::new());
    assert!(result.value.contains("data:image/x-emf;base64,ZW1m"));
    assert_eq!(
        result.messages,
        vec![Message::warning(
            "Image of type image/x-emf is unlikely to display in web browsers"
        )]
    );
}

// -- Tracked changes and miscellany ----------------------------------------------

#[test]
fn tracked_changes_keep_insertions_and_drop_deletions() {
    let body = p(
        "",
        &format!(
            r#"{}<w:ins w:id="1">{}</w:ins><w:del w:id="2"><w:r><w:delText>gone</w:delText></w:r></w:del>"#,
            r("", "kept "),
            r("", "inserted")
        ),
    ) + &format!(
        r#"<w:p><w:pPr><w:rPr><w:del w:id="3"/></w:rPr></w:pPr>{}</w:p>"#,
        r("", "merged ")
    ) + &p("", &r("", "next"));
    let docx = DocxBuilder::new(&body).build();
    assert_eq!(html(&docx), "<p>kept inserted</p><p>merged next</p>");
}

#[test]
fn unknown_elements_warn() {
    let body = p("", "<w:r><w:t>x</w:t><w:mystery/></w:r>");
    let result = html_with(&DocxBuilder::new(&body).build(), &Options::new());
    assert_eq!(
        result.messages,
        vec![Message::warning(
            "An unrecognised element was ignored: w:mystery"
        )]
    );
    assert_eq!(result.messages[0].kind, MessageKind::Warning);
}

#[test]
fn markdown_formatting() {
    let body = p(&style_p("Heading2"), &r("", "Title"))
        + &p(
            "",
            &format!(
                r#"{}{}<w:hyperlink r:id="rIdLink">{}</w:hyperlink>{}"#,
                r("<w:b/>", "bold"),
                r("<w:i/>", "it"),
                r("", "link"),
                r("", " 1.5 *stars*")
            ),
        );
    let docx = DocxBuilder::new(&body)
        .styles(&style("paragraph", "Heading2", "heading 2"))
        .relationship("rIdLink", "hyperlink", "http://example.com/x_y", true)
        .build();
    let result = convert_to_markdown(&docx, &Options::new()).unwrap();
    assert_eq!(
        result.value,
        "## Title\n\n__bold__*it*[link](http://example\\.com/x\\_y) 1\\.5 \\*stars\\*\n\n"
    );
}

#[test]
fn document_model_is_exposed() {
    let docx = DocxBuilder::new(&p(
        r#"<w:jc w:val="center"/><w:ind w:left="720"/>"#,
        &r(r#"<w:sz w:val="28"/><w:rFonts w:ascii="Arial"/>"#, "x"),
    ))
    .build();
    let (document, messages) = read_document(&docx, &Options::new()).unwrap();
    assert!(messages.is_empty());
    let Element::Paragraph(paragraph) = &document.children[0] else {
        panic!()
    };
    assert_eq!(paragraph.alignment.as_deref(), Some("center"));
    assert_eq!(paragraph.indent.start.as_deref(), Some("720"));
    let Element::Run(run) = &paragraph.children[0] else {
        panic!()
    };
    assert_eq!(run.font_size, Some(14.0));
    assert_eq!(run.font.as_deref(), Some("Arial"));
}
