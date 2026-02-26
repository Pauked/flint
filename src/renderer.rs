use std::collections::HashMap;

use anyhow::{Context, Result};
use tera::Tera;

use crate::models::{Book, BookHighlights, Highlight};
use crate::scraper::{kindle_app_link, parsed_author_names};

const DEFAULT_FILE_TEMPLATE: &str = include_str!("../templates/book.tera");
const DEFAULT_HIGHLIGHT_TEMPLATE: &str = include_str!("../templates/highlight.tera");

/// Escape a string for use in a YAML single-quoted value.
/// In YAML, single quotes are escaped by doubling them: ' → ''
fn escape_yaml(s: &str) -> String {
    s.replace('\'', "''").replace('\n', " ")
}

/// Render YAML frontmatter for a book.
///
/// `format` controls the output layout:
/// - `"flat"` (default): Obsidian properties (`kindle-bookId`, `kindle-title`, etc.)
/// - `"nested"`: legacy `kindle-sync:` wrapper with indented keys
pub fn render_frontmatter(book: &Book, highlights_count: usize, format: &str) -> String {
    if format == "nested" {
        return render_frontmatter_nested(book, highlights_count);
    }

    let mut fm = String::from("---\n");

    fm.push_str(&format!("kindle-bookId: '{}'\n", book.id));
    fm.push_str(&format!("kindle-title: '{}'\n", escape_yaml(&book.title)));

    // Author: only quote if it contains special YAML characters
    let author = &book.author;
    if author.contains(':')
        || author.contains(',')
        || author.contains('#')
        || author.contains('\'')
        || author.contains('"')
        || author.contains('[')
        || author.contains(']')
        || author.contains('{')
        || author.contains('}')
    {
        fm.push_str(&format!("kindle-author: '{}'\n", escape_yaml(author)));
    } else {
        fm.push_str(&format!("kindle-author: {author}\n"));
    }

    if let Some(ref asin) = book.asin {
        fm.push_str(&format!("kindle-asin: {asin}\n"));
    }

    if let Some(ref date) = book.last_annotated_date {
        fm.push_str(&format!(
            "kindle-lastAnnotatedDate: '{}'\n",
            date.format("%Y-%m-%d")
        ));
    }

    if let Some(ref img) = book.image_url {
        fm.push_str(&format!("kindle-bookImageUrl: '{}'\n", escape_yaml(img)));
    }

    fm.push_str(&format!("kindle-highlightsCount: {highlights_count}\n"));
    fm.push_str("---\n");

    fm
}

/// Render frontmatter in the legacy nested `kindle-sync:` format.
fn render_frontmatter_nested(book: &Book, highlights_count: usize) -> String {
    let mut fm = String::from("---\nkindle-sync:\n");

    fm.push_str(&format!("  bookId: '{}'\n", book.id));
    fm.push_str(&format!("  title: '{}'\n", escape_yaml(&book.title)));

    let author = &book.author;
    if author.contains(':')
        || author.contains(',')
        || author.contains('#')
        || author.contains('\'')
        || author.contains('"')
        || author.contains('[')
        || author.contains(']')
        || author.contains('{')
        || author.contains('}')
    {
        fm.push_str(&format!("  author: '{}'\n", escape_yaml(author)));
    } else {
        fm.push_str(&format!("  author: {author}\n"));
    }

    if let Some(ref asin) = book.asin {
        fm.push_str(&format!("  asin: {asin}\n"));
    }

    if let Some(ref date) = book.last_annotated_date {
        fm.push_str(&format!(
            "  lastAnnotatedDate: '{}'\n",
            date.format("%Y-%m-%d")
        ));
    }

    if let Some(ref img) = book.image_url {
        fm.push_str(&format!("  bookImageUrl: '{}'\n", escape_yaml(img)));
    }

    fm.push_str(&format!("  highlightsCount: {highlights_count}\n"));
    fm.push_str("---\n");

    fm
}

/// Map Kindle highlight color names to Obsidian Highlightr class suffixes.
fn highlight_color_code(color: &str) -> &str {
    match color {
        "yellow" => "y",
        "green" => "g",
        "pink" => "p",
        "blue" => "b",
        "red" => "r",
        "orange" => "o",
        _ => "y",
    }
}

/// Render a single highlight to markdown using a Tera template.
pub fn render_highlight(
    tera: &Tera,
    template_name: &str,
    highlight: &Highlight,
    book: &Book,
) -> Result<String> {
    let mut ctx = tera::Context::new();
    ctx.insert("id", &highlight.id);
    ctx.insert("text", &highlight.text.as_deref().unwrap_or(""));
    ctx.insert("location", &highlight.location.as_deref().unwrap_or(""));
    ctx.insert("page", &highlight.page.as_deref().unwrap_or(""));
    ctx.insert("note", &highlight.note.as_deref().unwrap_or(""));
    let color = highlight.color.as_deref().unwrap_or("");
    ctx.insert("color", &color);
    ctx.insert("color_code", highlight_color_code(color));

    let app_link = book
        .asin
        .as_ref()
        .map(|asin| kindle_app_link(asin, highlight.location.as_deref()));
    ctx.insert("app_link", &app_link.as_deref().unwrap_or(""));

    let rendered = tera
        .render(template_name, &ctx)
        .with_context(|| "Failed to render highlight template")?;

    // Append block reference to the line containing the highlight text
    let ref_suffix = format!(" ^ref-{}", highlight.id);
    let lines: Vec<&str> = rendered.lines().collect();

    let mut result = String::new();
    let mut ref_added = false;

    // Find a safe prefix of the highlight text to match against (respects char boundaries)
    let match_prefix = highlight.text.as_deref().map(|text| {
        let max_bytes = 40;
        let mut end = text.len().min(max_bytes);
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    });

    for line in &lines {
        if !ref_added && match_prefix.is_some_and(|p| line.contains(p)) {
            result.push_str(line);
            result.push_str(&ref_suffix);
            ref_added = true;
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    // If we couldn't find the text line (e.g., custom template), append ref to first non-empty line
    if !ref_added {
        result.clear();
        for line in &lines {
            if !ref_added && !line.trim().is_empty() {
                result.push_str(line);
                result.push_str(&ref_suffix);
                ref_added = true;
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }
    }

    Ok(result)
}

/// Render the complete file template (metadata section + highlights).
pub fn render_file(
    tera: &Tera,
    file_template_name: &str,
    highlight_template_name: &str,
    entry: &BookHighlights,
) -> Result<String> {
    let book = &entry.book;

    // Render all highlights
    let rendered_highlights: Vec<String> = entry
        .highlights
        .iter()
        .map(|h| render_highlight(tera, highlight_template_name, h, book))
        .collect::<Result<Vec<_>>>()?;

    let highlights_str = rendered_highlights.join("");

    // Build template context
    let mut ctx = tera::Context::new();
    let title = crate::scraper::shorten_title(&book.title);
    ctx.insert("title", &title);
    ctx.insert("long_title", &book.title);
    ctx.insert("author", &book.author);
    ctx.insert("asin", &book.asin.as_deref().unwrap_or(""));
    ctx.insert("url", &book.url.as_deref().unwrap_or(""));
    ctx.insert("image_url", &book.image_url.as_deref().unwrap_or(""));

    let app_link = book.asin.as_ref().map(|asin| kindle_app_link(asin, None));
    ctx.insert("app_link", &app_link.as_deref().unwrap_or(""));

    // Metadata fields
    let metadata = entry.metadata.as_ref();
    ctx.insert(
        "isbn",
        &metadata.and_then(|m| m.isbn.as_deref()).unwrap_or(""),
    );
    ctx.insert(
        "pages",
        &metadata.and_then(|m| m.pages.as_deref()).unwrap_or(""),
    );
    ctx.insert(
        "publication_date",
        &metadata
            .and_then(|m| m.publication_date.as_deref())
            .unwrap_or(""),
    );
    ctx.insert(
        "publisher",
        &metadata.and_then(|m| m.publisher.as_deref()).unwrap_or(""),
    );
    ctx.insert(
        "author_url",
        &metadata.and_then(|m| m.author_url.as_deref()).unwrap_or(""),
    );

    // Author name variables
    let author_names = parsed_author_names(&book.author);
    ctx.insert(
        "firstAuthorFirstName",
        &author_names.first_author_first_name,
    );
    ctx.insert("firstAuthorLastName", &author_names.first_author_last_name);
    ctx.insert(
        "secondAuthorFirstName",
        &author_names.second_author_first_name,
    );
    ctx.insert(
        "secondAuthorLastName",
        &author_names.second_author_last_name,
    );

    ctx.insert("highlights_count", &entry.highlights.len());
    ctx.insert("highlights", &highlights_str);

    let rendered = tera
        .render(file_template_name, &ctx)
        .context("Failed to render file template")?;

    // Clean up multiple blank lines (fix the original plugin's incomplete regex)
    let cleaned = regex::Regex::new(r"\n{3,}")
        .unwrap()
        .replace_all(&rendered, "\n\n");

    Ok(cleaned.into_owned())
}

/// Custom Tera filter for date formatting.
/// Tries parsing common date formats and reformats with the given `format` argument.
/// Falls back to the original string if parsing fails.
fn dateformat(
    value: &tera::Value,
    args: &HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let input = value
        .as_str()
        .ok_or_else(|| tera::Error::msg("dateformat: expected a string value"))?;

    if input.is_empty() {
        return Ok(tera::Value::String(String::new()));
    }

    let fmt = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("%Y-%m-%d");

    // Try common date formats
    let parsed = chrono::NaiveDate::parse_from_str(input, "%B %d, %Y")
        .or_else(|_| chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d"))
        .or_else(|_| chrono::NaiveDate::parse_from_str(input, "%Y"));

    match parsed {
        Ok(date) => Ok(tera::Value::String(date.format(fmt).to_string())),
        Err(_) => Ok(tera::Value::String(input.to_string())),
    }
}

/// Build a Tera instance with default or custom templates loaded.
pub fn build_tera(file_template: Option<&str>, highlight_template: Option<&str>) -> Result<Tera> {
    let mut tera = Tera::default();

    tera.register_filter("dateformat", dateformat);

    tera.add_raw_template("book.tera", file_template.unwrap_or(DEFAULT_FILE_TEMPLATE))
        .context("Failed to parse file template")?;

    tera.add_raw_template(
        "highlight.tera",
        highlight_template.unwrap_or(DEFAULT_HIGHLIGHT_TEMPLATE),
    )
    .context("Failed to parse highlight template")?;

    Ok(tera)
}

/// Render a complete markdown file (frontmatter + content) for a book.
pub fn render_book_file(
    entry: &BookHighlights,
    file_template: Option<&str>,
    highlight_template: Option<&str>,
    frontmatter_format: &str,
) -> Result<String> {
    let tera = build_tera(file_template, highlight_template)?;
    let frontmatter = render_frontmatter(&entry.book, entry.highlights.len(), frontmatter_format);
    let content = render_file(&tera, "book.tera", "highlight.tera", entry)?;

    Ok(format!("{frontmatter}{content}"))
}

/// Render a single highlight for diff/sync operations.
pub fn render_single_highlight(
    highlight: &Highlight,
    book: &Book,
    highlight_template: Option<&str>,
) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template(
        "highlight.tera",
        highlight_template.unwrap_or(DEFAULT_HIGHLIGHT_TEMPLATE),
    )
    .context("Failed to parse highlight template")?;

    render_highlight(&tera, "highlight.tera", highlight, book)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::BookMetadata;
    use std::collections::HashMap;

    #[test]
    fn test_dateformat_filter_full_date() {
        let value = tera::Value::String("January 1, 2020".to_string());
        let mut args = HashMap::new();
        args.insert(
            "format".to_string(),
            tera::Value::String("%Y-%m-%d".to_string()),
        );
        let result = dateformat(&value, &args).unwrap();
        assert_eq!(result.as_str().unwrap(), "2020-01-01");
    }

    #[test]
    fn test_dateformat_filter_iso() {
        let value = tera::Value::String("2024-03-15".to_string());
        let mut args = HashMap::new();
        args.insert(
            "format".to_string(),
            tera::Value::String("%B %Y".to_string()),
        );
        let result = dateformat(&value, &args).unwrap();
        assert_eq!(result.as_str().unwrap(), "March 2024");
    }

    #[test]
    fn test_dateformat_filter_passthrough() {
        let value = tera::Value::String("not a date".to_string());
        let args = HashMap::new();
        let result = dateformat(&value, &args).unwrap();
        assert_eq!(result.as_str().unwrap(), "not a date");
    }

    #[test]
    fn test_dateformat_filter_in_template() {
        let mut tera = Tera::default();
        tera.register_filter("dateformat", dateformat);
        tera.add_raw_template(
            "test",
            r#"{{ publication_date | dateformat(format="%B %Y") }}"#,
        )
        .unwrap();
        let mut ctx = tera::Context::new();
        ctx.insert("publication_date", "January 1, 2020");
        let result = tera.render("test", &ctx).unwrap();
        assert_eq!(result, "January 2020");
    }

    fn sample_book() -> Book {
        Book {
            id: "12345".to_string(),
            title: "Test Book: A Subtitle".to_string(),
            author: "John Doe".to_string(),
            asin: Some("B01TEST".to_string()),
            url: Some("https://www.amazon.com/dp/B01TEST".to_string()),
            image_url: Some("https://example.com/cover.jpg".to_string()),
            last_annotated_date: Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()),
        }
    }

    fn sample_highlight() -> Highlight {
        Highlight {
            id: "54321".to_string(),
            text: Some("This is a test highlight.".to_string()),
            location: Some("100".to_string()),
            page: Some("42".to_string()),
            note: None,
            color: Some("yellow".to_string()),
        }
    }

    #[test]
    fn test_render_frontmatter_flat() {
        let book = sample_book();
        let fm = render_frontmatter(&book, 10, "flat");
        assert!(!fm.contains("kindle-sync:"));
        assert!(fm.contains("kindle-bookId: '12345'"));
        assert!(fm.contains("kindle-title: 'Test Book: A Subtitle'"));
        assert!(fm.contains("kindle-author: John Doe"));
        assert!(fm.contains("kindle-asin: B01TEST"));
        assert!(fm.contains("kindle-lastAnnotatedDate: '2024-01-15'"));
        assert!(fm.contains("kindle-highlightsCount: 10"));
    }

    #[test]
    fn test_render_frontmatter_nested() {
        let book = sample_book();
        let fm = render_frontmatter(&book, 10, "nested");
        assert!(fm.contains("kindle-sync:\n"));
        assert!(fm.contains("  bookId: '12345'"));
        assert!(fm.contains("  title: 'Test Book: A Subtitle'"));
        assert!(fm.contains("  author: John Doe"));
        assert!(fm.contains("  asin: B01TEST"));
        assert!(fm.contains("  lastAnnotatedDate: '2024-01-15'"));
        assert!(fm.contains("  highlightsCount: 10"));
        assert!(!fm.contains("kindle-bookId:"));
    }

    #[test]
    fn test_render_highlight() {
        let book = sample_book();
        let highlight = sample_highlight();
        let rendered = render_single_highlight(&highlight, &book, None).unwrap();
        assert!(rendered.contains("This is a test highlight."));
        assert!(rendered.contains("location: [100]"));
        assert!(rendered.contains("^ref-54321"));
        assert!(rendered.contains("kindle://book?action=open&asin=B01TEST&location=100"));
        assert!(rendered.contains("---"));
    }

    #[test]
    fn test_render_highlight_with_note() {
        let book = sample_book();
        let highlight = Highlight {
            note: Some("My note here".to_string()),
            ..sample_highlight()
        };
        let rendered = render_single_highlight(&highlight, &book, None).unwrap();
        assert!(rendered.contains("My note here"));
    }

    #[test]
    fn test_render_book_file() {
        let book = sample_book();
        let entry = BookHighlights {
            book: book.clone(),
            highlights: vec![sample_highlight()],
            metadata: Some(BookMetadata {
                isbn: Some("1234567890".to_string()),
                ..Default::default()
            }),
        };
        let result = render_book_file(&entry, None, None, "flat").unwrap();
        assert!(result.contains("---\nkindle-bookId:"));
        assert!(result.contains("# Test Book"));
        assert!(result.contains("## Metadata"));
        assert!(result.contains("* ASIN: B01TEST"));
        assert!(result.contains("* ISBN: 1234567890"));
        assert!(result.contains("## Highlights"));
        assert!(result.contains("This is a test highlight."));
        assert!(result.contains("^ref-54321"));
    }

    #[test]
    fn test_render_highlight_with_multibyte_chars() {
        let book = sample_book();
        let highlight = Highlight {
            id: "99999".to_string(),
            text: Some("When you find yourself saying, \u{2018}I don\u{2019}t know,\u{2019} be sure to follow it up".to_string()),
            location: Some("200".to_string()),
            page: None,
            note: None,
            color: None,
        };
        let rendered = render_single_highlight(&highlight, &book, None).unwrap();
        assert!(rendered.contains("^ref-99999"));
        assert!(rendered.contains("\u{2018}I don\u{2019}t know"));
    }

    #[test]
    fn test_render_note_only_highlight() {
        let book = sample_book();
        let highlight = Highlight {
            id: "note1".to_string(),
            text: None,
            location: Some("300".to_string()),
            page: None,
            note: Some("My standalone note".to_string()),
            color: None,
        };
        let rendered = render_single_highlight(&highlight, &book, None).unwrap();
        assert!(rendered.contains("My standalone note"));
        assert!(rendered.contains("^ref-note1"));
    }

    #[test]
    fn test_escape_yaml() {
        assert_eq!(escape_yaml("simple"), "simple");
        assert_eq!(escape_yaml("has \"quotes\""), "has \"quotes\"");
        assert_eq!(escape_yaml("Beginner's Guide"), "Beginner''s Guide");
        assert_eq!(escape_yaml("has\nnewline"), "has newline");
    }

    #[test]
    fn test_format_matches_existing() {
        let book = Book {
            id: "49849".to_string(),
            title: "Atomic Habits: The life-changing million-copy #1 bestseller".to_string(),
            author: "James Clear".to_string(),
            asin: Some("B01N5AX61W".to_string()),
            url: Some("https://www.amazon.com/dp/B01N5AX61W".to_string()),
            image_url: Some(
                "https://m.media-amazon.com/images/I/81IL8Dy4vmL._SY160.jpg".to_string(),
            ),
            last_annotated_date: Some(chrono::NaiveDate::from_ymd_opt(2024, 8, 27).unwrap()),
        };
        let highlight = Highlight {
            id: "54880".to_string(),
            text: Some("improving by 1 percent isn't particularly notable".to_string()),
            location: Some("250".to_string()),
            page: None,
            note: None,
            color: Some("yellow".to_string()),
        };
        let entry = BookHighlights {
            book,
            highlights: vec![highlight],
            metadata: Some(BookMetadata {
                isbn: Some("B0CF9BKHNT".to_string()),
                ..Default::default()
            }),
        };
        let result = render_book_file(&entry, None, None, "flat").unwrap();

        // Verify frontmatter
        assert!(result.starts_with("---\n"));
        assert!(result.contains("kindle-bookId: '49849'"));
        assert!(result.contains("kindle-author: James Clear\n"));
        assert!(result.contains("kindle-asin: B01N5AX61W\n"));
        assert!(result.contains("kindle-lastAnnotatedDate: '2024-08-27'"));
        assert!(result.contains("kindle-highlightsCount: 1"));

        // Verify metadata section
        assert!(result.contains("# Atomic Habits"));
        assert!(result.contains("* Author: [[James Clear]]"));
        assert!(result.contains("* ASIN: B01N5AX61W"));
        assert!(result.contains("* ISBN: B0CF9BKHNT"));
        assert!(result.contains("* Reference: https://www.amazon.com/dp/B01N5AX61W"));
        assert!(result.contains("* [Kindle link](kindle://book?action=open&asin=B01N5AX61W)"));

        // Verify highlight format
        assert!(result.contains(
            "improving by 1 percent isn't particularly notable — <mark class=\"hltr-y\">yellow</mark> | location: [250](kindle://book?action=open&asin=B01N5AX61W&location=250) ^ref-54880"
        ));
        assert!(result.contains("---"));
    }
}
