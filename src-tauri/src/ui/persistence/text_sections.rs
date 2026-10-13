//! Splits a text asset into titled sections. The Access importer writes VBA as one
//! file with a `' ==== Module Helpers ====` line before each module and each form's or
//! report's code (access/convert/mod.rs). Text without such lines is one untitled
//! section. Port of src/persistence/textSections.ts.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSection {
    pub title: String,
    pub text: String,
}

fn header(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("' ==== ")?;
    let rest = rest.trim_end();
    let title = rest.strip_suffix(" ====")?;
    (!title.is_empty()).then_some(title)
}

pub fn text_sections(content: &str) -> Vec<TextSection> {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut sections: Vec<TextSection> = vec![];
    let mut current = false;
    for line in normalized.split('\n') {
        if let Some(title) = header(line) {
            sections.push(TextSection {
                title: title.to_string(),
                text: String::new(),
            });
            current = true;
        } else if current {
            let section = sections.last_mut().expect("a current section");
            if !section.text.is_empty() {
                section.text.push('\n');
            }
            section.text.push_str(line);
        } else if !line.trim().is_empty() || !sections.is_empty() {
            sections.push(TextSection {
                title: String::new(),
                text: line.to_string(),
            });
            current = true;
        }
    }
    for s in &mut sections {
        s.text = s.text.trim_end().to_string();
    }
    sections
}

/// Text assets the Assets tab can show (the Access VBA asset among them).
pub fn is_text_asset(media_type: &str) -> bool {
    media_type.to_ascii_lowercase().starts_with("text/")
}

#[cfg(test)]
mod tests {
    use super::{TextSection, text_sections};

    fn section(title: &str, text: &str) -> TextSection {
        TextSection {
            title: title.into(),
            text: text.into(),
        }
    }

    #[test]
    fn splits_the_access_vba_asset_into_one_section_per_module_and_form() {
        let text = "' ==== Module Helpers ====\r\nOption Compare Database\r\nPublic Function Twice(x)\r\n  Twice = x * 2\r\nEnd Function\r\n\r\n' ==== Form Customer Details ====\r\nPrivate Sub Form_Load()\r\nEnd Sub\r\n";
        assert_eq!(
            text_sections(text),
            vec![
                section(
                    "Module Helpers",
                    "Option Compare Database\nPublic Function Twice(x)\n  Twice = x * 2\nEnd Function"
                ),
                section("Form Customer Details", "Private Sub Form_Load()\nEnd Sub"),
            ]
        );
    }

    #[test]
    fn keeps_text_without_section_lines_as_one_untitled_section() {
        assert_eq!(
            text_sections("\nplain notes\nsecond line\n"),
            vec![section("", "plain notes\nsecond line")]
        );
        assert!(text_sections("").is_empty());
    }
}
