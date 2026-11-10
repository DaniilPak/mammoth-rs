//! Parser for mammoth's style mapping syntax, e.g.
//! `p[style-name='Section Title'] => h1:fresh`.
//!
//! A port of mammoth.js's `style-reader.js`, `styles/document-matchers.js`
//! and `styles/parser/tokeniser.js`.

use std::rc::Rc;

use crate::document::{BreakType, Paragraph, Run, Table};
use crate::html::{Attributes, HtmlPath, HtmlTag};

/// The default style map used by mammoth.js.
pub const DEFAULT_STYLE_MAP: &[&str] = &[
    "p.Heading1 => h1:fresh",
    "p.Heading2 => h2:fresh",
    "p.Heading3 => h3:fresh",
    "p.Heading4 => h4:fresh",
    "p.Heading5 => h5:fresh",
    "p.Heading6 => h6:fresh",
    "p[style-name='Heading 1'] => h1:fresh",
    "p[style-name='Heading 2'] => h2:fresh",
    "p[style-name='Heading 3'] => h3:fresh",
    "p[style-name='Heading 4'] => h4:fresh",
    "p[style-name='Heading 5'] => h5:fresh",
    "p[style-name='Heading 6'] => h6:fresh",
    "p[style-name='heading 1'] => h1:fresh",
    "p[style-name='heading 2'] => h2:fresh",
    "p[style-name='heading 3'] => h3:fresh",
    "p[style-name='heading 4'] => h4:fresh",
    "p[style-name='heading 5'] => h5:fresh",
    "p[style-name='heading 6'] => h6:fresh",
    // Apple Pages
    "p.Heading => h1:fresh",
    "p[style-name='Heading'] => h1:fresh",
    "r[style-name='Strong'] => strong",
    "p[style-name='footnote text'] => p:fresh",
    "r[style-name='footnote reference'] =>",
    "p[style-name='endnote text'] => p:fresh",
    "r[style-name='endnote reference'] =>",
    "p[style-name='annotation text'] => p:fresh",
    "r[style-name='annotation reference'] =>",
    // LibreOffice
    "p[style-name='Footnote'] => p:fresh",
    "r[style-name='Footnote anchor'] =>",
    "p[style-name='Endnote'] => p:fresh",
    "r[style-name='Endnote anchor'] =>",
    "p:unordered-list(1) => ul > li:fresh",
    "p:unordered-list(2) => ul|ol > li > ul > li:fresh",
    "p:unordered-list(3) => ul|ol > li > ul|ol > li > ul > li:fresh",
    "p:unordered-list(4) => ul|ol > li > ul|ol > li > ul|ol > li > ul > li:fresh",
    "p:unordered-list(5) => ul|ol > li > ul|ol > li > ul|ol > li > ul|ol > li > ul > li:fresh",
    "p:ordered-list(1) => ol > li:fresh",
    "p:ordered-list(2) => ul|ol > li > ol > li:fresh",
    "p:ordered-list(3) => ul|ol > li > ul|ol > li > ol > li:fresh",
    "p:ordered-list(4) => ul|ol > li > ul|ol > li > ul|ol > li > ol > li:fresh",
    "p:ordered-list(5) => ul|ol > li > ul|ol > li > ul|ol > li > ul|ol > li > ol > li:fresh",
    "r[style-name='Hyperlink'] =>",
    "p[style-name='Normal'] => p:fresh",
    // Apple Pages
    "p.Body => p:fresh",
    "p[style-name='Body'] => p:fresh",
];

/// Splits a multi-line style map into mappings, skipping blank lines and
/// `#` comments.
pub fn read_style_map_lines(style_map: &str) -> Vec<String> {
    style_map
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// Matchers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StringMatcher {
    EqualTo(String),
    StartsWith(String),
}

impl StringMatcher {
    fn matches(&self, value: &str) -> bool {
        match self {
            StringMatcher::EqualTo(expected) => expected.to_uppercase() == value.to_uppercase(),
            StringMatcher::StartsWith(prefix) => {
                value.to_uppercase().starts_with(&prefix.to_uppercase())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ListMatcher {
    is_ordered: bool,
    level_index: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ElementMatcher {
    style_id: Option<String>,
    style_name: Option<StringMatcher>,
    list: Option<ListMatcher>,
}

impl ElementMatcher {
    fn matches_style(&self, style_id: Option<&str>, style_name: Option<&str>) -> bool {
        self.style_id
            .as_deref()
            .is_none_or(|expected| style_id == Some(expected))
            && self
                .style_name
                .as_ref()
                .is_none_or(|matcher| style_name.is_some_and(|name| matcher.matches(name)))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DocumentMatcher {
    Paragraph(ElementMatcher),
    Run(ElementMatcher),
    Table(ElementMatcher),
    Bold,
    Italic,
    Underline,
    Strikethrough,
    AllCaps,
    SmallCaps,
    Highlight(Option<String>),
    CommentReference,
    Break(BreakType),
}

/// Something in the document that a style mapping may match.
pub(crate) enum MatchTarget<'a> {
    Paragraph(&'a Paragraph),
    Run(&'a Run),
    Table(&'a Table),
    Bold,
    Italic,
    Underline,
    Strikethrough,
    AllCaps,
    SmallCaps,
    Highlight(&'a str),
    CommentReference,
    Break(BreakType),
}

impl DocumentMatcher {
    pub fn matches(&self, target: &MatchTarget<'_>) -> bool {
        match (self, target) {
            (DocumentMatcher::Paragraph(matcher), MatchTarget::Paragraph(paragraph)) => {
                matcher.matches_style(
                    paragraph.style_id.as_deref(),
                    paragraph.style_name.as_deref(),
                ) && matcher.list.as_ref().is_none_or(|list| {
                    paragraph.numbering.as_ref().is_some_and(|numbering| {
                        numbering.is_ordered == list.is_ordered
                            && numbering
                                .level
                                .trim()
                                .parse::<i64>()
                                .is_ok_and(|level| level == list.level_index)
                    })
                })
            }
            (DocumentMatcher::Run(matcher), MatchTarget::Run(run)) => {
                matcher.list.is_none()
                    && matcher.matches_style(run.style_id.as_deref(), run.style_name.as_deref())
            }
            (DocumentMatcher::Table(matcher), MatchTarget::Table(table)) => {
                matcher.matches_style(table.style_id.as_deref(), table.style_name.as_deref())
            }
            (DocumentMatcher::Bold, MatchTarget::Bold)
            | (DocumentMatcher::Italic, MatchTarget::Italic)
            | (DocumentMatcher::Underline, MatchTarget::Underline)
            | (DocumentMatcher::Strikethrough, MatchTarget::Strikethrough)
            | (DocumentMatcher::AllCaps, MatchTarget::AllCaps)
            | (DocumentMatcher::SmallCaps, MatchTarget::SmallCaps)
            | (DocumentMatcher::CommentReference, MatchTarget::CommentReference) => true,
            (DocumentMatcher::Highlight(color), MatchTarget::Highlight(actual)) => {
                color.as_deref().is_none_or(|color| color == *actual)
            }
            (DocumentMatcher::Break(expected), MatchTarget::Break(actual)) => expected == actual,
            _ => false,
        }
    }
}

/// A parsed style mapping: `from => to`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StyleMapping {
    pub from: DocumentMatcher,
    pub to: HtmlPath,
}

// ---------------------------------------------------------------------------
// Tokeniser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Identifier,
    Dot,
    Colon,
    Gt,
    Whitespace,
    Arrow,
    Equals,
    StartsWith,
    OpenParen,
    CloseParen,
    OpenSquareBracket,
    CloseSquareBracket,
    String,
    UnterminatedString,
    Integer,
    Choice,
    Bang,
    Unrecognised,
    End,
}

impl TokenKind {
    fn name(self) -> &'static str {
        match self {
            TokenKind::Identifier => "identifier",
            TokenKind::Dot => "dot",
            TokenKind::Colon => "colon",
            TokenKind::Gt => "gt",
            TokenKind::Whitespace => "whitespace",
            TokenKind::Arrow => "arrow",
            TokenKind::Equals => "equals",
            TokenKind::StartsWith => "startsWith",
            TokenKind::OpenParen => "open-paren",
            TokenKind::CloseParen => "close-paren",
            TokenKind::OpenSquareBracket => "open-square-bracket",
            TokenKind::CloseSquareBracket => "close-square-bracket",
            TokenKind::String => "string",
            TokenKind::UnterminatedString => "unterminated-string",
            TokenKind::Integer => "integer",
            TokenKind::Choice => "choice",
            TokenKind::Bang => "bang",
            TokenKind::Unrecognised => "unrecognisedCharacter",
            TokenKind::End => "end",
        }
    }
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    /// The token's value: the identifier/string contents (undecoded) or the raw text.
    value: String,
    /// One-based character position.
    position: usize,
}

impl Token {
    fn describe(&self) -> String {
        if self.kind == TokenKind::End {
            "end".to_string()
        } else {
            format!("{} \"{}\"", self.kind.name(), self.value)
        }
    }
}

fn is_identifier_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '-' || c == '_'
}

fn tokenise(input: &str) -> Vec<Token> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let kind = if is_identifier_start(c) || (c == '\\' && next.is_some()) {
            while i < chars.len() {
                if is_identifier_start(chars[i]) || chars[i].is_ascii_digit() {
                    i += 1;
                } else if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 2;
                } else {
                    break;
                }
            }
            TokenKind::Identifier
        } else if c.is_whitespace() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            TokenKind::Whitespace
        } else if c == '=' && next == Some('>') {
            i += 2;
            TokenKind::Arrow
        } else if c == '^' && next == Some('=') {
            i += 2;
            TokenKind::StartsWith
        } else if c == '\'' {
            i += 1;
            let mut terminated = false;
            while i < chars.len() {
                match chars[i] {
                    '\\' => i += 2,
                    '\'' => {
                        terminated = true;
                        break;
                    }
                    _ => i += 1,
                }
            }
            let i_end = i.min(chars.len());
            let value: String = chars[start + 1..i_end].iter().collect();
            if terminated {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::String,
                    value,
                    position: start + 1,
                });
            } else {
                i = chars.len();
                tokens.push(Token {
                    kind: TokenKind::UnterminatedString,
                    value,
                    position: start + 1,
                });
            }
            continue;
        } else if c.is_ascii_digit() {
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            TokenKind::Integer
        } else {
            i += 1;
            match c {
                '.' => TokenKind::Dot,
                ':' => TokenKind::Colon,
                '>' => TokenKind::Gt,
                '=' => TokenKind::Equals,
                '(' => TokenKind::OpenParen,
                ')' => TokenKind::CloseParen,
                '[' => TokenKind::OpenSquareBracket,
                ']' => TokenKind::CloseSquareBracket,
                '|' => TokenKind::Choice,
                '!' => TokenKind::Bang,
                _ => TokenKind::Unrecognised,
            }
        };
        tokens.push(Token {
            kind,
            value: chars[start..i].iter().collect(),
            position: start + 1,
        });
    }
    tokens.push(Token {
        kind: TokenKind::End,
        value: String::new(),
        position: chars.len() + 1,
    });
    tokens
}

fn decode_escape_sequences(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => output.push('\n'),
                Some('r') => output.push('\r'),
                Some('t') => output.push('\t'),
                Some(other) => output.push(other),
                None => output.push('\\'),
            }
        } else {
            output.push(c);
        }
    }
    output
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct ParseError {
    token: Token,
    expected: String,
}

type ParseResult<T> = std::result::Result<T, ParseError>;

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn peek_at(&self, offset: usize) -> &Token {
        let index = (self.index + offset).min(self.tokens.len() - 1);
        &self.tokens[index]
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.index].clone();
        if self.index < self.tokens.len() - 1 {
            self.index += 1;
        }
        token
    }

    fn error<T>(&self, expected: &str) -> ParseResult<T> {
        Err(ParseError {
            token: self.peek().clone(),
            expected: expected.to_string(),
        })
    }

    fn is(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn is_identifier(&self, value: &str) -> bool {
        self.is(TokenKind::Identifier) && self.peek().value == value
    }

    fn expect(&mut self, kind: TokenKind) -> ParseResult<Token> {
        if self.is(kind) {
            Ok(self.advance())
        } else {
            self.error(kind.name())
        }
    }

    fn expect_identifier_value(&mut self, value: &str) -> ParseResult<()> {
        if self.is_identifier(value) {
            self.advance();
            Ok(())
        } else {
            self.error(&format!("identifier \"{value}\""))
        }
    }

    fn identifier(&mut self) -> ParseResult<String> {
        self.expect(TokenKind::Identifier)
            .map(|token| decode_escape_sequences(&token.value))
    }

    fn string(&mut self) -> ParseResult<String> {
        self.expect(TokenKind::String)
            .map(|token| decode_escape_sequences(&token.value))
    }

    fn skip_whitespace(&mut self) -> bool {
        if self.is(TokenKind::Whitespace) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn style_mapping(&mut self) -> ParseResult<StyleMapping> {
        let from = self.document_matcher()?;
        self.skip_whitespace();
        self.expect(TokenKind::Arrow)?;
        self.skip_whitespace();
        let to = if self.is(TokenKind::End) {
            HtmlPath::empty()
        } else {
            self.html_path()?
        };
        self.skip_whitespace();
        self.expect(TokenKind::End)?;
        Ok(StyleMapping { from, to })
    }

    fn document_matcher(&mut self) -> ParseResult<DocumentMatcher> {
        if !self.is(TokenKind::Identifier) {
            return self.error("element type");
        }
        let name = self.peek().value.clone();
        let simple = match name.as_str() {
            "b" => Some(DocumentMatcher::Bold),
            "i" => Some(DocumentMatcher::Italic),
            "u" => Some(DocumentMatcher::Underline),
            "strike" => Some(DocumentMatcher::Strikethrough),
            "all-caps" => Some(DocumentMatcher::AllCaps),
            "small-caps" => Some(DocumentMatcher::SmallCaps),
            "comment-reference" => Some(DocumentMatcher::CommentReference),
            _ => None,
        };
        if let Some(matcher) = simple {
            self.advance();
            return Ok(matcher);
        }
        match name.as_str() {
            "p" => {
                self.advance();
                Ok(DocumentMatcher::Paragraph(self.matcher_suffixes(true)?))
            }
            "r" => {
                self.advance();
                Ok(DocumentMatcher::Run(self.matcher_suffixes(true)?))
            }
            "table" => {
                self.advance();
                Ok(DocumentMatcher::Table(self.matcher_suffixes(false)?))
            }
            "highlight" => {
                self.advance();
                let mut color = None;
                if self.is(TokenKind::OpenSquareBracket) {
                    self.advance();
                    self.expect_identifier_value("color")?;
                    self.expect(TokenKind::Equals)?;
                    color = Some(self.string()?);
                    self.expect(TokenKind::CloseSquareBracket)?;
                }
                Ok(DocumentMatcher::Highlight(color))
            }
            "br" => {
                self.advance();
                self.expect(TokenKind::OpenSquareBracket)?;
                self.expect_identifier_value("type")?;
                self.expect(TokenKind::Equals)?;
                let position = self.index;
                let break_type = match self.string()?.as_str() {
                    "line" => BreakType::Line,
                    "page" => BreakType::Page,
                    "column" => BreakType::Column,
                    _ => {
                        self.index = position;
                        return self.error("break type ('line', 'page' or 'column')");
                    }
                };
                self.expect(TokenKind::CloseSquareBracket)?;
                Ok(DocumentMatcher::Break(break_type))
            }
            _ => self.error("element type"),
        }
    }

    fn matcher_suffixes(&mut self, allow_list: bool) -> ParseResult<ElementMatcher> {
        let mut matcher = ElementMatcher::default();
        loop {
            if self.is(TokenKind::Dot) {
                self.advance();
                matcher.style_id = Some(self.identifier()?);
            } else if self.is(TokenKind::OpenSquareBracket) {
                self.advance();
                self.expect_identifier_value("style-name")?;
                if self.is(TokenKind::Equals) {
                    self.advance();
                    matcher.style_name = Some(StringMatcher::EqualTo(self.string()?));
                } else if self.is(TokenKind::StartsWith) {
                    self.advance();
                    matcher.style_name = Some(StringMatcher::StartsWith(self.string()?));
                } else {
                    return self.error("style name matcher");
                }
                self.expect(TokenKind::CloseSquareBracket)?;
            } else if allow_list
                && self.is(TokenKind::Colon)
                && self.peek_at(1).kind == TokenKind::Identifier
                && matches!(
                    self.peek_at(1).value.as_str(),
                    "ordered-list" | "unordered-list"
                )
            {
                self.advance();
                let is_ordered = self.advance().value == "ordered-list";
                self.expect(TokenKind::OpenParen)?;
                let level = self.expect(TokenKind::Integer)?;
                self.expect(TokenKind::CloseParen)?;
                let level: i64 = level.value.parse().unwrap_or(1);
                matcher.list = Some(ListMatcher {
                    is_ordered,
                    level_index: level - 1,
                });
            } else {
                return Ok(matcher);
            }
        }
    }

    fn html_path(&mut self) -> ParseResult<HtmlPath> {
        if self.is(TokenKind::Bang) {
            self.advance();
            return Ok(HtmlPath::Ignore);
        }
        let mut elements = Vec::new();
        if !self.is(TokenKind::Identifier) {
            return Ok(HtmlPath::Elements(elements));
        }
        elements.push(Rc::new(self.html_element()?));
        loop {
            let save = self.index;
            self.skip_whitespace();
            if self.is(TokenKind::Gt) {
                self.advance();
                self.skip_whitespace();
                elements.push(Rc::new(self.html_element()?));
            } else {
                self.index = save;
                return Ok(HtmlPath::Elements(elements));
            }
        }
    }

    fn html_element(&mut self) -> ParseResult<HtmlTag> {
        let mut tag_names = vec![self.identifier()?];
        while self.is(TokenKind::Choice) {
            self.advance();
            tag_names.push(self.identifier()?);
        }

        let mut attributes = Attributes::new();
        loop {
            if self.is(TokenKind::OpenSquareBracket) {
                self.advance();
                let name = self.identifier()?;
                self.expect(TokenKind::Equals)?;
                let value = self.string()?;
                self.expect(TokenKind::CloseSquareBracket)?;
                attributes.set(&name, value);
            } else if self.is(TokenKind::Dot) {
                self.advance();
                let class_name = self.identifier()?;
                let value = match attributes.get("class") {
                    Some(existing) if !existing.is_empty() => format!("{existing} {class_name}"),
                    _ => class_name,
                };
                attributes.set("class", value);
            } else {
                break;
            }
        }

        let mut fresh = false;
        let mut separator = None;
        while self.is(TokenKind::Colon) {
            let next = self.peek_at(1);
            if next.kind == TokenKind::Identifier && next.value == "fresh" {
                self.advance();
                self.advance();
                fresh = true;
            } else if next.kind == TokenKind::Identifier && next.value == "separator" {
                self.advance();
                self.advance();
                self.expect(TokenKind::OpenParen)?;
                separator = Some(self.string()?);
                self.expect(TokenKind::CloseParen)?;
            } else {
                self.advance();
                return self.error("fresh or separator");
            }
        }

        Ok(HtmlTag {
            tag_names,
            attributes,
            fresh,
            separator: separator.filter(|s| !s.is_empty()),
        })
    }
}

/// Parses a single style mapping. On failure, returns the warning text that
/// mammoth.js would report.
pub(crate) fn parse_style_mapping(input: &str) -> std::result::Result<StyleMapping, String> {
    let mut parser = Parser {
        tokens: tokenise(input),
        index: 0,
    };
    parser.style_mapping().map_err(|error| {
        format!(
            "Did not understand this style mapping, so ignored it: {input}\nError was at character number {}: Expected {} but got {}",
            error.token.position,
            error.expected,
            error.token.describe()
        )
    })
}

/// Checks whether `mapping` is a valid style mapping.
///
/// Returns the same description of the problem that a conversion would
/// report as a warning.
///
/// ```
/// assert!(mammoth_rs::validate_style_mapping("p[style-name='Title'] => h1:fresh").is_ok());
/// assert!(mammoth_rs::validate_style_mapping("p => ").is_ok());
/// assert!(mammoth_rs::validate_style_mapping("????").is_err());
/// ```
pub fn validate_style_mapping(mapping: &str) -> std::result::Result<(), String> {
    parse_style_mapping(mapping).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> StyleMapping {
        parse_style_mapping(input).unwrap_or_else(|e| panic!("{e}"))
    }

    fn tag(names: &[&str], fresh: bool) -> Rc<HtmlTag> {
        Rc::new(HtmlTag {
            tag_names: names.iter().map(|s| s.to_string()).collect(),
            attributes: Attributes::new(),
            fresh,
            separator: None,
        })
    }

    #[test]
    fn default_style_map_parses() {
        for mapping in DEFAULT_STYLE_MAP {
            parse(mapping);
        }
    }

    #[test]
    fn paragraph_with_style_id() {
        let mapping = parse("p.Heading1 => h1:fresh");
        assert_eq!(
            mapping.from,
            DocumentMatcher::Paragraph(ElementMatcher {
                style_id: Some("Heading1".into()),
                ..Default::default()
            })
        );
        assert_eq!(mapping.to, HtmlPath::Elements(vec![tag(&["h1"], true)]));
    }

    #[test]
    fn style_name_matchers() {
        let mapping = parse("r[style-name^='Code'] => code");
        assert_eq!(
            mapping.from,
            DocumentMatcher::Run(ElementMatcher {
                style_name: Some(StringMatcher::StartsWith("Code".into())),
                ..Default::default()
            })
        );
    }

    #[test]
    fn nested_paths_with_choices() {
        let mapping = parse("p:ordered-list(2) => ul|ol > li > ol > li:fresh");
        assert_eq!(
            mapping.from,
            DocumentMatcher::Paragraph(ElementMatcher {
                list: Some(ListMatcher {
                    is_ordered: true,
                    level_index: 1
                }),
                ..Default::default()
            })
        );
        assert_eq!(
            mapping.to,
            HtmlPath::Elements(vec![
                tag(&["ul", "ol"], false),
                tag(&["li"], false),
                tag(&["ol"], false),
                tag(&["li"], true),
            ])
        );
    }

    #[test]
    fn attributes_classes_and_separators() {
        let mapping = parse("p.Code => pre.code.block[data-x='1']:separator('\\n')");
        let HtmlPath::Elements(tags) = mapping.to else {
            panic!()
        };
        assert_eq!(tags[0].attributes.get("class"), Some("code block"));
        assert_eq!(tags[0].attributes.get("data-x"), Some("1"));
        assert_eq!(tags[0].separator.as_deref(), Some("\n"));
    }

    #[test]
    fn ignore_and_empty_paths() {
        assert_eq!(parse("r.Hidden => !").to, HtmlPath::Ignore);
        assert_eq!(parse("r[style-name='x'] =>").to, HtmlPath::empty());
    }

    #[test]
    fn other_matchers() {
        assert_eq!(parse("b => em").from, DocumentMatcher::Bold);
        assert_eq!(
            parse("highlight[color='yellow'] => mark").from,
            DocumentMatcher::Highlight(Some("yellow".into()))
        );
        assert_eq!(
            parse("br[type='page'] => hr").from,
            DocumentMatcher::Break(BreakType::Page)
        );
        assert_eq!(
            parse("table[style-name='Grid'] => table.grid").from,
            DocumentMatcher::Table(ElementMatcher {
                style_name: Some(StringMatcher::EqualTo("Grid".into())),
                ..Default::default()
            })
        );
    }

    #[test]
    fn escapes_in_strings_and_identifiers() {
        let mapping = parse(r"p[style-name='Bob\'s style'] => p.a\.b");
        assert_eq!(
            mapping.from,
            DocumentMatcher::Paragraph(ElementMatcher {
                style_name: Some(StringMatcher::EqualTo("Bob's style".into())),
                ..Default::default()
            })
        );
        let HtmlPath::Elements(tags) = mapping.to else {
            panic!()
        };
        assert_eq!(tags[0].attributes.get("class"), Some("a.b"));
    }

    #[test]
    fn error_messages_match_mammoth() {
        assert_eq!(
            parse_style_mapping("????").unwrap_err(),
            "Did not understand this style mapping, so ignored it: ????\nError was at character number 1: Expected element type but got unrecognisedCharacter \"?\""
        );
        assert!(parse_style_mapping("p => h1 extra").is_err());
        assert!(parse_style_mapping("p[style-name='x' => h1").is_err());
    }
}
