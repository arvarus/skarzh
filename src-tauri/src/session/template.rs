//! Path templates: how the organised photos will be laid out in the output folder.
//!
//! A template is a relative path whose segments mix literal text and tokens, for example
//! `{year}/{month}/{camera}/{filename}`.
//!
//! Tokens: `{year}`, `{month}`, `{day}`, `{camera}`, `{original_folder}`, `{filename}`.
//!
//! Rules (each violation is its own [`TemplateError`] variant):
//! - `/` and `\` are both accepted as separators and normalised to `/`;
//! - the template is relative: no leading separator, no drive letter, no UNC path;
//! - no empty segment (`a//b`, trailing separator) and no `.` or `..` segment;
//! - literal text cannot contain characters invalid on Windows (`<>:"|?*` and control
//!   characters), cannot end with a dot or a space, and a literal segment cannot be a reserved
//!   Windows device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`, also with an
//!   extension such as `CON.txt`);
//! - `{filename}` appears exactly once, in the last segment (it may be combined with other text,
//!   e.g. `{year}_{filename}`);
//! - the template rendered with the longest plausible values stays within [`MAX_RENDERED_LEN`]
//!   characters.
//!
//! Applying a template to real files is not done here: this module only parses, validates and
//! renders a preview from sample metadata.

use std::fmt;

/// Longest accepted rendered relative path (in characters), leaving room for the output folder
/// under the 260 characters `MAX_PATH` limit.
pub const MAX_RENDERED_LEN: usize = 240;

/// Replacement for an empty or unusable camera name.
pub const UNKNOWN_CAMERA: &str = "unknown-camera";

/// Characters invalid in a Windows file name, besides the separators and control characters.
const INVALID_CHARS: [char; 7] = ['<', '>', ':', '"', '|', '?', '*'];

const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Why a template is rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    #[error("the path template is empty")]
    Empty,
    #[error("unknown token {{{0}}}")]
    UnknownToken(String),
    /// A `}` without `{`, or a `{` never closed (position in characters).
    #[error("unbalanced brace at position {position}")]
    UnbalancedBraces { position: usize },
    /// A `{` inside a token (position in characters).
    #[error("nested brace at position {position}")]
    NestedBraces { position: usize },
    #[error("the template must be a relative path (no leading separator)")]
    AbsolutePath,
    #[error("the template must not start with a drive letter")]
    DriveLetter,
    #[error("the template must not be a UNC path")]
    UncPath,
    #[error("`{0}` is not allowed as a path segment")]
    DotSegment(String),
    #[error("the template contains an empty path segment")]
    EmptySegment,
    #[error("the character {0:?} is not allowed in a Windows path")]
    InvalidCharacter(char),
    #[error("{0} is a reserved Windows name")]
    ReservedName(String),
    #[error("the segment `{0}` ends with a dot or a space")]
    InvalidSegmentEnd(String),
    #[error("the template must contain {{filename}}")]
    MissingFilename,
    #[error("the template must contain {{filename}} only once")]
    DuplicateFilename,
    #[error("{{filename}} must be in the last path segment")]
    FilenameNotLast,
    #[error("the rendered path can reach {length} characters, the maximum is {max}")]
    TooLong { length: usize, max: usize },
}

impl TemplateError {
    /// Stable machine-readable code of the error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Empty => "template_empty",
            Self::UnknownToken(_) => "template_unknown_token",
            Self::UnbalancedBraces { .. } => "template_unbalanced_braces",
            Self::NestedBraces { .. } => "template_nested_braces",
            Self::AbsolutePath => "template_absolute_path",
            Self::DriveLetter => "template_drive_letter",
            Self::UncPath => "template_unc_path",
            Self::DotSegment(_) => "template_dot_segment",
            Self::EmptySegment => "template_empty_segment",
            Self::InvalidCharacter(_) => "template_invalid_character",
            Self::ReservedName(_) => "template_reserved_name",
            Self::InvalidSegmentEnd(_) => "template_invalid_segment_end",
            Self::MissingFilename => "template_missing_filename",
            Self::DuplicateFilename => "template_duplicate_filename",
            Self::FilenameNotLast => "template_filename_not_last",
            Self::TooLong { .. } => "template_too_long",
        }
    }
}

/// A substitution token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Year,
    Month,
    Day,
    Camera,
    OriginalFolder,
    Filename,
}

impl Token {
    /// Name of the token as written between braces.
    pub fn name(self) -> &'static str {
        match self {
            Self::Year => "year",
            Self::Month => "month",
            Self::Day => "day",
            Self::Camera => "camera",
            Self::OriginalFolder => "original_folder",
            Self::Filename => "filename",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        [
            Self::Year,
            Self::Month,
            Self::Day,
            Self::Camera,
            Self::OriginalFolder,
            Self::Filename,
        ]
        .into_iter()
        .find(|token| token.name() == name)
    }
}

/// A piece of a path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Literal(String),
    Token(Token),
}

/// One path segment: literal text and tokens, in order.
pub type Segment = Vec<Part>;

/// Metadata used to render a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSample {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub camera: String,
    pub original_folder: String,
    pub filename: String,
}

impl TemplateSample {
    /// Fixed sample used by the preview shown in the setup screen.
    pub fn preview() -> Self {
        Self {
            year: 2024,
            month: 7,
            day: 14,
            camera: "Canon EOS R6".into(),
            original_folder: "Holidays".into(),
            filename: "IMG_0042.jpg".into(),
        }
    }

    /// Longest plausible values, used to check the rendered length.
    fn worst_case() -> Self {
        Self {
            year: 9999,
            month: 12,
            day: 31,
            camera: "c".repeat(40),
            original_folder: "f".repeat(40),
            filename: format!("{}.jpg", "n".repeat(36)),
        }
    }
}

/// A parsed template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTemplate {
    segments: Vec<Segment>,
}

fn is_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

impl PathTemplate {
    /// Parses a template: syntax, tokens and path shape. Does not apply the Windows naming rules
    /// nor the filename rule: use [`PathTemplate::validate`] or [`validate`] for that.
    pub fn parse(input: &str) -> Result<Self, TemplateError> {
        if input.trim().is_empty() {
            return Err(TemplateError::Empty);
        }
        let mut chars = input.chars();
        let first = chars.next().unwrap_or_default();
        let second = chars.next();
        if is_separator(first) {
            return Err(if second.is_some_and(is_separator) {
                TemplateError::UncPath
            } else {
                TemplateError::AbsolutePath
            });
        }
        if first.is_ascii_alphabetic() && second == Some(':') {
            return Err(TemplateError::DriveLetter);
        }

        let mut segments: Vec<Segment> = vec![Vec::new()];
        let mut literal = String::new();
        let mut open: Option<usize> = None;
        let mut name = String::new();

        for (position, c) in input.chars().enumerate() {
            let current = segments.last_mut().expect("at least one segment");
            if let Some(start) = open {
                match c {
                    '{' => return Err(TemplateError::NestedBraces { position }),
                    '}' => {
                        let token = Token::from_name(&name)
                            .ok_or_else(|| TemplateError::UnknownToken(name.clone()))?;
                        current.push(Part::Token(token));
                        name.clear();
                        open = None;
                    }
                    c if is_separator(c) => {
                        return Err(TemplateError::UnbalancedBraces { position: start });
                    }
                    c => name.push(c),
                }
            } else {
                match c {
                    '{' => {
                        if !literal.is_empty() {
                            current.push(Part::Literal(std::mem::take(&mut literal)));
                        }
                        open = Some(position);
                    }
                    '}' => return Err(TemplateError::UnbalancedBraces { position }),
                    c if is_separator(c) => {
                        if !literal.is_empty() {
                            current.push(Part::Literal(std::mem::take(&mut literal)));
                        }
                        segments.push(Vec::new());
                    }
                    c => literal.push(c),
                }
            }
        }
        if let Some(start) = open {
            return Err(TemplateError::UnbalancedBraces { position: start });
        }
        if !literal.is_empty() {
            let current = segments.last_mut().expect("at least one segment");
            current.push(Part::Literal(literal));
        }

        for segment in &segments {
            match segment.as_slice() {
                [] => return Err(TemplateError::EmptySegment),
                [Part::Literal(text)] if text == "." || text == ".." => {
                    return Err(TemplateError::DotSegment(text.clone()));
                }
                _ => {}
            }
        }
        Ok(Self { segments })
    }

    /// The parsed segments.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The template as a string with `/` separators.
    pub fn normalized(&self) -> String {
        self.segments
            .iter()
            .map(|segment| {
                segment
                    .iter()
                    .map(|part| match part {
                        Part::Literal(text) => text.clone(),
                        Part::Token(token) => format!("{{{}}}", token.name()),
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Checks the Windows naming rules, the filename rule and the rendered length.
    pub fn validate(&self) -> Result<(), TemplateError> {
        for segment in &self.segments {
            for part in segment {
                if let Part::Literal(text) = part
                    && let Some(c) = text
                        .chars()
                        .find(|c| c.is_control() || INVALID_CHARS.contains(c))
                {
                    return Err(TemplateError::InvalidCharacter(c));
                }
            }
            if let Some(stem) = fixed_stem(segment)
                && is_reserved_name(stem)
            {
                return Err(TemplateError::ReservedName(stem.trim_end().to_uppercase()));
            }
            if let Some(Part::Literal(text)) = segment.last()
                && text.ends_with(['.', ' '])
            {
                return Err(TemplateError::InvalidSegmentEnd(segment_text(segment)));
            }
        }

        let filename_count = self
            .segments
            .iter()
            .flatten()
            .filter(|part| **part == Part::Token(Token::Filename))
            .count();
        match filename_count {
            0 => return Err(TemplateError::MissingFilename),
            1 => {}
            _ => return Err(TemplateError::DuplicateFilename),
        }
        let in_last = self
            .segments
            .last()
            .is_some_and(|segment| segment.contains(&Part::Token(Token::Filename)));
        if !in_last {
            return Err(TemplateError::FilenameNotLast);
        }

        let length = self.render(&TemplateSample::worst_case()).chars().count();
        if length > MAX_RENDERED_LEN {
            return Err(TemplateError::TooLong {
                length,
                max: MAX_RENDERED_LEN,
            });
        }
        Ok(())
    }

    /// Renders the relative path (with `/` separators) for the given metadata.
    ///
    /// Substituted values are sanitised: characters invalid on Windows and control characters
    /// become `_`, trailing dots and spaces are trimmed, month and day are padded to two digits
    /// and the year to four. An empty camera becomes [`UNKNOWN_CAMERA`]. A segment that ends up
    /// empty becomes `_` and one that is a reserved Windows name gets a `_` prefix, so the result
    /// is always a usable path whatever the metadata.
    pub fn render(&self, sample: &TemplateSample) -> String {
        self.segments
            .iter()
            .map(|segment| render_segment(segment, sample))
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// Parses then validates a template, returning its normalised form (`/` separators).
pub fn validate(input: &str) -> Result<PathTemplate, TemplateError> {
    let template = PathTemplate::parse(input)?;
    template.validate()?;
    Ok(template)
}

/// Validates a template and renders it with the fixed preview sample.
pub fn preview(input: &str) -> Result<String, TemplateError> {
    Ok(validate(input)?.render(&TemplateSample::preview()))
}

fn segment_text(segment: &Segment) -> String {
    segment
        .iter()
        .map(|part| match part {
            Part::Literal(text) => text.clone(),
            Part::Token(token) => format!("{{{}}}", token.name()),
        })
        .collect()
}

/// The part of a segment before its first dot, when it is fixed text whatever the tokens are:
/// the whole text of a literal-only segment, or the literal prefix when it already contains a dot.
fn fixed_stem(segment: &Segment) -> Option<&str> {
    let Some(Part::Literal(first)) = segment.first() else {
        return None;
    };
    match first.split_once('.') {
        Some((stem, _)) => Some(stem),
        None if segment.len() == 1 => Some(first),
        None => None,
    }
}

fn is_reserved_name(stem: &str) -> bool {
    let stem = stem.trim_end();
    RESERVED_NAMES
        .iter()
        .any(|name| name.eq_ignore_ascii_case(stem))
}

fn sanitize(value: &str) -> String {
    let replaced: String = value
        .chars()
        .map(|c| {
            if c.is_control() || INVALID_CHARS.contains(&c) || is_separator(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    replaced.trim_end_matches(['.', ' ']).to_string()
}

fn render_segment(segment: &Segment, sample: &TemplateSample) -> String {
    let mut out = String::new();
    for part in segment {
        match part {
            Part::Literal(text) => out.push_str(text),
            Part::Token(token) => match token {
                Token::Year => out.push_str(&format!("{:04}", sample.year)),
                Token::Month => out.push_str(&format!("{:02}", sample.month)),
                Token::Day => out.push_str(&format!("{:02}", sample.day)),
                Token::Camera => {
                    let camera = sanitize(sample.camera.trim());
                    if camera.is_empty() {
                        out.push_str(UNKNOWN_CAMERA);
                    } else {
                        out.push_str(&camera);
                    }
                }
                Token::OriginalFolder => out.push_str(&sanitize(&sample.original_folder)),
                Token::Filename => out.push_str(&sanitize(&sample.filename)),
            },
        }
    }
    let mut out = out.trim_end_matches(['.', ' ']).to_string();
    if out.is_empty() {
        out.push('_');
    }
    let stem = out.split('.').next().unwrap_or_default();
    if is_reserved_name(stem) {
        out.insert(0, '_');
    }
    out
}

impl fmt::Display for PathTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.normalized())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TemplateSample {
        TemplateSample::preview()
    }

    fn err(input: &str) -> TemplateError {
        validate(input).unwrap_err()
    }

    #[test]
    fn parses_every_token() {
        let template =
            PathTemplate::parse("{year}/{month}/{day}/{camera}/{original_folder}/{filename}")
                .unwrap();
        let tokens: Vec<Token> = template
            .segments()
            .iter()
            .map(|segment| match segment[0] {
                Part::Token(token) => token,
                Part::Literal(_) => panic!("expected a token"),
            })
            .collect();
        assert_eq!(
            tokens,
            [
                Token::Year,
                Token::Month,
                Token::Day,
                Token::Camera,
                Token::OriginalFolder,
                Token::Filename
            ]
        );
    }

    #[test]
    fn parses_literals_mixed_with_tokens() {
        let template = PathTemplate::parse("Photos {year}-{month}/{filename}").unwrap();
        assert_eq!(
            template.segments()[0],
            vec![
                Part::Literal("Photos ".into()),
                Part::Token(Token::Year),
                Part::Literal("-".into()),
                Part::Token(Token::Month)
            ]
        );
    }

    #[test]
    fn accepts_both_separators_and_normalises_them() {
        let template = validate(r"{year}\{month}/{filename}").unwrap();
        assert_eq!(template.normalized(), "{year}/{month}/{filename}");
        assert_eq!(template.to_string(), "{year}/{month}/{filename}");
    }

    #[test]
    fn renders_every_token_with_padding() {
        let template =
            validate("{year}/{month}/{day}/{camera}/{original_folder}/{filename}").unwrap();
        assert_eq!(
            template.render(&sample()),
            "2024/07/14/Canon EOS R6/Holidays/IMG_0042.jpg"
        );
    }

    #[test]
    fn preview_uses_the_fixed_sample() {
        assert_eq!(
            preview("{year}-{month}-{day}/{filename}").unwrap(),
            "2024-07-14/IMG_0042.jpg"
        );
        assert_eq!(preview("a/{filename}").unwrap(), "a/IMG_0042.jpg");
        assert_eq!(err_of_preview("a"), TemplateError::MissingFilename);
    }

    fn err_of_preview(input: &str) -> TemplateError {
        preview(input).unwrap_err()
    }

    #[test]
    fn render_pads_small_numbers() {
        let mut s = sample();
        s.year = 987;
        s.month = 1;
        s.day = 2;
        let template = validate("{year}-{month}-{day}/{filename}").unwrap();
        assert_eq!(template.render(&s), "0987-01-02/IMG_0042.jpg");
    }

    #[test]
    fn render_sanitises_values() {
        let mut s = sample();
        s.camera = "Canon: <EOS>/R6 ".into();
        s.original_folder = "a|b?c*d\"e\\f\u{7}g".into();
        s.filename = "pho:to. ".into();
        let template = validate("{camera}/{original_folder}/{filename}").unwrap();
        assert_eq!(template.render(&s), "Canon_ _EOS__R6/a_b_c_d_e_f_g/pho_to");
    }

    #[test]
    fn render_trims_trailing_dots_and_spaces() {
        let mut s = sample();
        s.camera = "Camera. . ".into();
        let template = validate("{camera}/{filename}").unwrap();
        assert_eq!(template.render(&s), "Camera/IMG_0042.jpg");
    }

    #[test]
    fn render_empty_camera_uses_placeholder() {
        let template = validate("{camera}/{filename}").unwrap();
        for camera in ["", "   ", "..."] {
            let mut s = sample();
            s.camera = camera.into();
            assert_eq!(template.render(&s), "unknown-camera/IMG_0042.jpg");
        }
    }

    #[test]
    fn render_never_produces_empty_or_dot_segments() {
        let mut s = sample();
        s.original_folder = "..".into();
        let template = validate("{original_folder}/{filename}").unwrap();
        assert_eq!(template.render(&s), "_/IMG_0042.jpg");
        s.original_folder = String::new();
        assert_eq!(template.render(&s), "_/IMG_0042.jpg");
    }

    #[test]
    fn render_prefixes_reserved_names() {
        let mut s = sample();
        s.camera = "con".into();
        s.filename = "NUL.txt".into();
        let template = validate("{camera}/{filename}").unwrap();
        assert_eq!(template.render(&s), "_con/_NUL.txt");
    }

    #[test]
    fn rejects_empty_templates() {
        assert_eq!(err(""), TemplateError::Empty);
        assert_eq!(err("   "), TemplateError::Empty);
    }

    #[test]
    fn rejects_unknown_tokens() {
        assert_eq!(
            err("{foo}/{filename}"),
            TemplateError::UnknownToken("foo".into())
        );
        assert_eq!(
            err("{Year}/{filename}"),
            TemplateError::UnknownToken("Year".into())
        );
        assert_eq!(
            err("{}/{filename}"),
            TemplateError::UnknownToken(String::new())
        );
    }

    #[test]
    fn rejects_unbalanced_braces() {
        assert_eq!(
            err("{year/{filename}"),
            TemplateError::UnbalancedBraces { position: 0 }
        );
        assert_eq!(
            err("a}/{filename}"),
            TemplateError::UnbalancedBraces { position: 1 }
        );
        assert_eq!(
            err("{filename"),
            TemplateError::UnbalancedBraces { position: 0 }
        );
        assert_eq!(
            err("{year/x}/{filename}"),
            TemplateError::UnbalancedBraces { position: 0 }
        );
    }

    #[test]
    fn rejects_nested_braces() {
        assert_eq!(
            err("{{year}}/{filename}"),
            TemplateError::NestedBraces { position: 1 }
        );
        assert_eq!(
            err("{ye{ar}/{filename}"),
            TemplateError::NestedBraces { position: 3 }
        );
    }

    #[test]
    fn rejects_absolute_drive_and_unc_paths() {
        assert_eq!(err("/{filename}"), TemplateError::AbsolutePath);
        assert_eq!(err(r"\{filename}"), TemplateError::AbsolutePath);
        assert_eq!(err(r"C:\out\{filename}"), TemplateError::DriveLetter);
        assert_eq!(err("c:{filename}"), TemplateError::DriveLetter);
        assert_eq!(err(r"\\server\share\{filename}"), TemplateError::UncPath);
        assert_eq!(err("//server/share/{filename}"), TemplateError::UncPath);
    }

    #[test]
    fn rejects_dot_segments() {
        assert_eq!(err("../{filename}"), TemplateError::DotSegment("..".into()));
        assert_eq!(err("a/./{filename}"), TemplateError::DotSegment(".".into()));
        assert_eq!(
            err(r"a\..\{filename}"),
            TemplateError::DotSegment("..".into())
        );
    }

    #[test]
    fn rejects_empty_segments() {
        assert_eq!(err("a//{filename}"), TemplateError::EmptySegment);
        assert_eq!(err(r"a\\{filename}"), TemplateError::EmptySegment);
        assert_eq!(err("a/{filename}/"), TemplateError::EmptySegment);
    }

    #[test]
    fn rejects_invalid_characters_in_literals() {
        for c in ['<', '>', ':', '"', '|', '?', '*', '\u{1}', '\t'] {
            assert_eq!(
                err(&format!("ab{c}c/{{filename}}")),
                TemplateError::InvalidCharacter(c),
                "{c:?}"
            );
        }
    }

    #[test]
    fn rejects_reserved_names_in_literal_segments() {
        for name in [
            "CON", "con", "Prn", "AUX", "NUL", "COM1", "com9", "LPT1", "lpt9",
        ] {
            assert_eq!(
                err(&format!("{name}/{{filename}}")),
                TemplateError::ReservedName(name.to_uppercase()),
                "{name}"
            );
        }
        assert_eq!(
            err("CON.txt/{filename}"),
            TemplateError::ReservedName("CON".into())
        );
        assert_eq!(
            err("nul.{year}/{filename}"),
            TemplateError::ReservedName("NUL".into())
        );
        assert_eq!(
            err("{year}/aux.backup"),
            TemplateError::ReservedName("AUX".into())
        );
    }

    #[test]
    fn accepts_names_that_only_resemble_reserved_ones() {
        for ok in [
            "CONSOLE/{filename}",
            "COM10/{filename}",
            "COM0/{filename}",
            "CON{year}/{filename}",
            "{camera}/{filename}",
            "{year}CON/{filename}",
        ] {
            assert!(validate(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn rejects_literal_segments_ending_with_dot_or_space() {
        assert_eq!(
            err("a./{filename}"),
            TemplateError::InvalidSegmentEnd("a.".into())
        );
        assert_eq!(
            err("{year} /{filename}"),
            TemplateError::InvalidSegmentEnd("{year} ".into())
        );
        assert!(validate("{year}.old/{filename}").is_ok());
    }

    #[test]
    fn filename_rules() {
        assert_eq!(err("{year}"), TemplateError::MissingFilename);
        assert_eq!(err("a/b"), TemplateError::MissingFilename);
        assert_eq!(
            err("{filename}/{filename}"),
            TemplateError::DuplicateFilename
        );
        assert_eq!(
            err("{filename}_{filename}"),
            TemplateError::DuplicateFilename
        );
        assert_eq!(err("{filename}/{year}"), TemplateError::FilenameNotLast);
        assert!(validate("{filename}").is_ok());
        assert_eq!(
            validate("{year}_{filename}").unwrap().render(&sample()),
            "2024_IMG_0042.jpg"
        );
    }

    #[test]
    fn rejects_templates_that_can_render_too_long() {
        let long = format!("{}/{{filename}}", "x".repeat(260));
        assert!(matches!(
            err(&long),
            TemplateError::TooLong { max: 240, .. }
        ));
        // 6 segments of 40 characters plus separators and a 40 characters filename.
        let many = "{camera}/{original_folder}/{camera}/{original_folder}/{camera}/{filename}";
        assert!(matches!(err(many), TemplateError::TooLong { .. }));
        // Still fine just below the limit.
        assert!(validate("{camera}/{original_folder}/{filename}").is_ok());
    }

    #[test]
    fn error_codes_are_unique_and_stable() {
        let errors = [
            TemplateError::Empty,
            TemplateError::UnknownToken(String::new()),
            TemplateError::UnbalancedBraces { position: 0 },
            TemplateError::NestedBraces { position: 0 },
            TemplateError::AbsolutePath,
            TemplateError::DriveLetter,
            TemplateError::UncPath,
            TemplateError::DotSegment(String::new()),
            TemplateError::EmptySegment,
            TemplateError::InvalidCharacter('<'),
            TemplateError::ReservedName(String::new()),
            TemplateError::InvalidSegmentEnd(String::new()),
            TemplateError::MissingFilename,
            TemplateError::DuplicateFilename,
            TemplateError::FilenameNotLast,
            TemplateError::TooLong { length: 0, max: 0 },
        ];
        let mut codes: Vec<&str> = errors.iter().map(TemplateError::code).collect();
        assert!(codes.iter().all(|code| code.starts_with("template_")));
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), errors.len());
    }

    #[test]
    fn messages_mention_the_offending_token() {
        assert_eq!(
            TemplateError::UnknownToken("foo".into()).to_string(),
            "unknown token {foo}"
        );
        assert_eq!(
            TemplateError::MissingFilename.to_string(),
            "the template must contain {filename}"
        );
    }
}
