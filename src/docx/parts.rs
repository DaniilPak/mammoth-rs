//! Readers for the small supporting parts of a `.docx` package:
//! relationships, content types, styles and numbering.

use std::collections::HashMap;

use crate::document::NumberingLevel;
use crate::xml::XmlElement;

// ---------------------------------------------------------------------------
// Relationships
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub(crate) struct Relationships {
    by_id: HashMap<String, String>,
    by_type: HashMap<String, Vec<String>>,
}

impl Relationships {
    pub fn read(element: &XmlElement) -> Self {
        let mut relationships = Relationships::default();
        for child in element.elements_named("relationships:Relationship") {
            let id = child.attr("Id").unwrap_or_default().to_string();
            let target = child.attr("Target").unwrap_or_default().to_string();
            let kind = child.attr("Type").unwrap_or_default().to_string();
            relationships.by_id.insert(id, target.clone());
            relationships.by_type.entry(kind).or_default().push(target);
        }
        relationships
    }

    pub fn find_target_by_id(&self, id: &str) -> Option<&str> {
        self.by_id.get(id).map(String::as_str)
    }

    pub fn find_targets_by_type(&self, kind: &str) -> &[String] {
        self.by_type.get(kind).map(Vec::as_slice).unwrap_or(&[])
    }
}

// ---------------------------------------------------------------------------
// Content types
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub(crate) struct ContentTypes {
    overrides: HashMap<String, String>,
    extension_defaults: HashMap<String, String>,
}

impl ContentTypes {
    pub fn read(element: &XmlElement) -> Self {
        let mut content_types = ContentTypes::default();
        for child in element.elements() {
            if child.name == "content-types:Default" {
                content_types.extension_defaults.insert(
                    child.attr("Extension").unwrap_or_default().to_string(),
                    child.attr("ContentType").unwrap_or_default().to_string(),
                );
            }
            if child.name == "content-types:Override" {
                let name = child.attr("PartName").unwrap_or_default();
                let name = name.strip_prefix('/').unwrap_or(name);
                content_types.overrides.insert(
                    name.to_string(),
                    child.attr("ContentType").unwrap_or_default().to_string(),
                );
            }
        }
        content_types
    }

    pub fn find_content_type(&self, path: &str) -> Option<String> {
        if let Some(content_type) = self.overrides.get(path)
            && !content_type.is_empty()
        {
            return Some(content_type.clone());
        }
        let extension = path.rsplit('.').next().unwrap_or(path);
        if let Some(content_type) = self.extension_defaults.get(extension) {
            return Some(content_type.clone());
        }
        let fallback = match extension.to_lowercase().as_str() {
            "png" => "png",
            "gif" => "gif",
            "jpeg" | "jpg" => "jpeg",
            "tif" | "tiff" => "tiff",
            "bmp" => "bmp",
            _ => return None,
        };
        Some(format!("image/{fallback}"))
    }
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct Style {
    pub name: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Styles {
    paragraph: HashMap<String, Style>,
    character: HashMap<String, Style>,
    table: HashMap<String, Style>,
    /// Numbering style ID → numId.
    numbering: HashMap<String, Option<String>>,
}

impl Styles {
    pub fn read(root: &XmlElement) -> Self {
        let mut styles = Styles::default();
        for element in root.elements_named("w:style") {
            let style_id = element.attr("w:styleId").unwrap_or_default().to_string();
            let name = element
                .first("w:name")
                .and_then(|name| name.attr("w:val"))
                .map(str::to_string);
            // Per ECMA-376, when several styles share an ID the first one wins.
            match element.attr("w:type") {
                Some("paragraph") => {
                    styles.paragraph.entry(style_id).or_insert(Style { name });
                }
                Some("character") => {
                    styles.character.entry(style_id).or_insert(Style { name });
                }
                Some("table") => {
                    styles.table.entry(style_id).or_insert(Style { name });
                }
                Some("numbering") => {
                    let num_id = element
                        .first_or_empty("w:pPr")
                        .first_or_empty("w:numPr")
                        .first_or_empty("w:numId")
                        .attr("w:val")
                        .map(str::to_string);
                    styles.numbering.entry(style_id).or_insert(num_id);
                }
                _ => {}
            }
        }
        styles
    }

    pub fn find_paragraph_style(&self, id: &str) -> Option<&Style> {
        self.paragraph.get(id)
    }

    pub fn find_character_style(&self, id: &str) -> Option<&Style> {
        self.character.get(id)
    }

    pub fn find_table_style(&self, id: &str) -> Option<&Style> {
        self.table.get(id)
    }

    fn find_numbering_style_num_id(&self, id: &str) -> Option<&str> {
        self.numbering.get(id).and_then(|num_id| num_id.as_deref())
    }
}

// ---------------------------------------------------------------------------
// Numbering
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Level {
    is_ordered: bool,
    level: String,
    paragraph_style_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct AbstractNum {
    levels: HashMap<String, Level>,
    level_order: Vec<String>,
    num_style_link: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Numbering {
    nums: HashMap<String, String>,
    abstract_nums: HashMap<String, AbstractNum>,
    levels_by_paragraph_style_id: HashMap<String, NumberingLevel>,
    styles: Styles,
}

impl Numbering {
    pub fn read(root: &XmlElement, styles: &Styles) -> Self {
        let mut abstract_nums = HashMap::new();
        let mut abstract_order = Vec::new();
        for element in root.elements_named("w:abstractNum") {
            let id = element
                .attr("w:abstractNumId")
                .unwrap_or_default()
                .to_string();
            abstract_order.push(id.clone());
            abstract_nums.insert(id, read_abstract_num(element));
        }

        let mut nums = HashMap::new();
        for element in root.elements_named("w:num") {
            let num_id = element.attr("w:numId").unwrap_or_default().to_string();
            let abstract_num_id = element
                .first_or_empty("w:abstractNumId")
                .attr("w:val")
                .unwrap_or_default()
                .to_string();
            nums.insert(num_id, abstract_num_id);
        }

        // As in mammoth.js, later definitions win when several levels claim
        // the same paragraph style.
        let mut levels_by_paragraph_style_id = HashMap::new();
        for id in &abstract_order {
            let abstract_num = &abstract_nums[id];
            for level_id in &abstract_num.level_order {
                let level: &Level = &abstract_num.levels[level_id];
                if let Some(style_id) = &level.paragraph_style_id {
                    levels_by_paragraph_style_id.insert(
                        style_id.clone(),
                        NumberingLevel {
                            level: level.level.clone(),
                            is_ordered: level.is_ordered,
                        },
                    );
                }
            }
        }

        Numbering {
            nums,
            abstract_nums,
            levels_by_paragraph_style_id,
            styles: styles.clone(),
        }
    }

    pub fn find_level(&self, num_id: &str, level: &str) -> Option<NumberingLevel> {
        let mut seen = Vec::new();
        self.find_level_with_seen(num_id, level, &mut seen)
    }

    fn find_level_with_seen(
        &self,
        num_id: &str,
        level: &str,
        seen: &mut Vec<String>,
    ) -> Option<NumberingLevel> {
        if seen.iter().any(|id| id == num_id) {
            return None;
        }
        seen.push(num_id.to_string());

        let abstract_num_id = self.nums.get(num_id)?;
        let abstract_num = self.abstract_nums.get(abstract_num_id)?;
        match &abstract_num.num_style_link {
            None => abstract_num.levels.get(level).map(|level| NumberingLevel {
                level: level.level.clone(),
                is_ordered: level.is_ordered,
            }),
            Some(link) => {
                let linked_num_id = self.styles.find_numbering_style_num_id(link)?;
                let linked_num_id = linked_num_id.to_string();
                self.find_level_with_seen(&linked_num_id, level, seen)
            }
        }
    }

    pub fn find_level_by_paragraph_style_id(&self, style_id: &str) -> Option<NumberingLevel> {
        self.levels_by_paragraph_style_id.get(style_id).cloned()
    }
}

fn read_abstract_num(element: &XmlElement) -> AbstractNum {
    let mut abstract_num = AbstractNum::default();
    let mut level_without_index: Option<Level> = None;

    for level_element in element.elements_named("w:lvl") {
        let num_fmt = level_element.first_or_empty("w:numFmt").attr("w:val");
        let is_ordered = num_fmt != Some("bullet");
        let paragraph_style_id = level_element
            .first_or_empty("w:pStyle")
            .attr("w:val")
            .map(str::to_string);
        match level_element.attr("w:ilvl") {
            None => {
                level_without_index = Some(Level {
                    is_ordered,
                    level: "0".to_string(),
                    paragraph_style_id,
                });
            }
            Some(index) => {
                if !abstract_num.levels.contains_key(index) {
                    abstract_num.level_order.push(index.to_string());
                }
                abstract_num.levels.insert(
                    index.to_string(),
                    Level {
                        is_ordered,
                        level: index.to_string(),
                        paragraph_style_id,
                    },
                );
            }
        }
    }

    // Some malformed documents define levels without an index: assume level 0.
    if let Some(level) = level_without_index
        && !abstract_num.levels.contains_key(&level.level)
    {
        abstract_num.level_order.push(level.level.clone());
        abstract_num.levels.insert(level.level.clone(), level);
    }

    abstract_num.num_style_link = element
        .first_or_empty("w:numStyleLink")
        .attr("w:val")
        .map(str::to_string);
    abstract_num
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;

    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn numbering_levels_are_read() {
        let root = parse(&format!(
            r#"<w:numbering {W}>
                <w:abstractNum w:abstractNumId="42">
                    <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
                    <w:lvl w:ilvl="1"><w:numFmt w:val="decimal"/></w:lvl>
                </w:abstractNum>
                <w:num w:numId="47"><w:abstractNumId w:val="42"/></w:num>
            </w:numbering>"#
        ))
        .unwrap();
        let numbering = Numbering::read(&root, &Styles::default());
        assert_eq!(
            numbering.find_level("47", "0"),
            Some(NumberingLevel {
                level: "0".into(),
                is_ordered: false
            })
        );
        assert!(numbering.find_level("47", "1").unwrap().is_ordered);
        assert_eq!(numbering.find_level("47", "2"), None);
        assert_eq!(numbering.find_level("99", "0"), None);
    }

    #[test]
    fn numbering_style_links_are_followed() {
        let styles = Styles::read(
            &parse(&format!(
                r#"<w:styles {W}><w:style w:type="numbering" w:styleId="List1"><w:pPr><w:numPr><w:numId w:val="200"/></w:numPr></w:pPr></w:style></w:styles>"#
            ))
            .unwrap(),
        );
        let root = parse(&format!(
            r#"<w:numbering {W}>
                <w:abstractNum w:abstractNumId="100"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
                <w:abstractNum w:abstractNumId="101"><w:numStyleLink w:val="List1"/></w:abstractNum>
                <w:num w:numId="200"><w:abstractNumId w:val="100"/></w:num>
                <w:num w:numId="201"><w:abstractNumId w:val="101"/></w:num>
            </w:numbering>"#
        ))
        .unwrap();
        let numbering = Numbering::read(&root, &styles);
        assert!(!numbering.find_level("201", "0").unwrap().is_ordered);
    }

    #[test]
    fn content_type_fallbacks() {
        let content_types = ContentTypes::default();
        assert_eq!(
            content_types
                .find_content_type("word/media/a.JPG")
                .as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(content_types.find_content_type("a.emf"), None);
    }
}
