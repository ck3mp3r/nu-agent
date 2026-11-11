use super::*;

#[test]
fn parse_front_matter_scenarios() -> Result<()> {
    let parser = PulldownCmarkFrontMatterParser;

    // Front matter is extracted and the body is trimmed.
    let input = r#"---
permissions:
  "*": allow
---
# Body

Content here"#;

    let parsed = parser
        .parse(input)
        .map_err(|e| format!("parse should succeed: {e:?}"))?;
    let front_matter = parsed
        .front_matter
        .ok_or("front matter should be present")?;
    assert!(front_matter.contains_key("permissions"));
    assert_eq!(parsed.body, "# Body\n\nContent here");

    // Without front matter the body is the whole input.
    let input = r#"# Body

Content here"#;

    let parsed = parser
        .parse(input)
        .map_err(|e| format!("parse should succeed: {e:?}"))?;
    assert!(parsed.front_matter.is_none());
    assert_eq!(parsed.body, input);

    // An empty YAML block parses to an empty mapping.
    // Note: pulldown-cmark treats `---\n---\n` without content as horizontal rules, not metadata.
    // A truly empty YAML block would need at least whitespace or a comment.
    let input = r#"---
# Empty YAML
---
# Body

Content here"#;

    let parsed = parser
        .parse(input)
        .map_err(|e| format!("parse should succeed: {e:?}"))?;
    let front_matter = parsed
        .front_matter
        .ok_or("front matter should be present")?;
    assert_eq!(front_matter.len(), 0);
    assert_eq!(parsed.body, "# Body\n\nContent here");

    // The body keeps its content without whitespace artifacts.
    let input = r#"---
key: value
---
# Heading

- List item 1
- List item 2

Code:
```rust
fn main() {}
```"#;

    let parsed = parser
        .parse(input)
        .map_err(|e| format!("parse should succeed: {e:?}"))?;

    assert!(parsed.body.starts_with("# Heading"));
    assert!(parsed.body.contains("- List item 1"));
    assert!(parsed.body.contains("fn main() {}"));

    // Multiline YAML keys are all present.
    let input = r#"---
permissions:
  read: allow
  write: deny
author:
  name: Test User
  email: test@example.com
tags:
  - rust
  - testing
---
# Document

Body content"#;

    let parsed = parser
        .parse(input)
        .map_err(|e| format!("parse should succeed: {e:?}"))?;
    let front_matter = parsed
        .front_matter
        .ok_or("front matter should be present")?;
    assert!(front_matter.contains_key("permissions"));
    assert!(front_matter.contains_key("author"));
    assert!(front_matter.contains_key("tags"));
    assert_eq!(parsed.body, "# Document\n\nBody content");
    Ok(())
}

#[test]
fn parse_invalid_yaml() {
    let input = r#"---
[invalid: yaml:
---
# Body"#;

    let parser = PulldownCmarkFrontMatterParser;
    let result = parser.parse(input);

    assert!(result.is_err());
}
