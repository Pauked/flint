use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use log::debug;
use regex::Regex;

use crate::models::{Book, BookHighlights, Highlight, KindleFrontmatter, LegacyKindleFrontmatter};
use crate::renderer;

/// An existing file in the output directory that has kindle frontmatter.
pub struct ExistingFile {
    pub path: PathBuf,
    pub frontmatter: KindleFrontmatter,
    pub content: String,
}

/// A highlight that already exists in a local file.
struct RenderedHighlight {
    line: usize,
    highlight_id: String,
}

/// Result of diffing remote highlights against local file.
struct DiffResult {
    highlight: Highlight,
    /// Line number of the next existing highlight to insert before.
    /// None means append to end.
    insert_before_line: Option<usize>,
}

/// Collection of existing files indexed by both bookId and ASIN for fallback matching.
pub struct ExistingFiles {
    by_book_id: HashMap<String, ExistingFile>,
    by_asin: HashMap<String, String>, // ASIN -> bookId
}

impl ExistingFiles {
    /// Look up an existing file by bookId, falling back to ASIN.
    pub fn find(&self, book: &Book) -> Option<&ExistingFile> {
        if let Some(file) = self.by_book_id.get(&book.id) {
            return Some(file);
        }
        // Fallback: match by ASIN
        if let Some(asin) = &book.asin
            && let Some(book_id) = self.by_asin.get(asin)
            && let Some(file) = self.by_book_id.get(book_id)
        {
            debug!(
                "Matched '{}' by ASIN {} (bookId {} -> {})",
                book.title, asin, book.id, book_id
            );
            return Some(file);
        }
        None
    }
}

/// Scan an output directory for existing markdown files with kindle frontmatter.
pub fn scan_existing_files(output_dir: &Path) -> Result<ExistingFiles> {
    let mut by_book_id = HashMap::new();
    let mut by_asin: HashMap<String, String> = HashMap::new();

    if !output_dir.exists() {
        return Ok(ExistingFiles {
            by_book_id,
            by_asin,
        });
    }

    let entries = fs::read_dir(output_dir).context("Failed to read output directory")?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if let Some(frontmatter) = parse_frontmatter(&content) {
            let book_id = frontmatter.book_id.clone();
            if let Some(ref asin) = frontmatter.asin {
                by_asin
                    .entry(asin.clone())
                    .or_insert_with(|| book_id.clone());
            }
            by_book_id.insert(
                book_id,
                ExistingFile {
                    path,
                    frontmatter,
                    content,
                },
            );
        }
    }

    debug!(
        "Scanned {} existing files ({} with ASIN)",
        by_book_id.len(),
        by_asin.len()
    );
    Ok(ExistingFiles {
        by_book_id,
        by_asin,
    })
}

/// Load an existing kindle markdown file from a path.
pub fn load_existing_file(path: &Path) -> Result<ExistingFile> {
    let path = path
        .canonicalize()
        .with_context(|| format!("File not found: {}", path.display()))?;
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let frontmatter = parse_frontmatter(&content)
        .with_context(|| format!("No kindle frontmatter found in {}", path.display()))?;
    Ok(ExistingFile {
        path,
        frontmatter,
        content,
    })
}

/// Parse kindle frontmatter properties from a markdown file's content.
/// Supports both the new flat format (`kindle-bookId:`) and the legacy
/// nested format (`kindle-sync: { bookId: ... }`).
fn parse_frontmatter(content: &str) -> Option<KindleFrontmatter> {
    // Find YAML frontmatter between --- markers
    if !content.starts_with("---") {
        return None;
    }

    let rest = &content[3..];
    let end = rest.find("---")?;
    let yaml_str = &rest[..end];

    // Try flat format first, then legacy nested format
    serde_yaml::from_str::<KindleFrontmatter>(yaml_str)
        .ok()
        .or_else(|| {
            serde_yaml::from_str::<LegacyKindleFrontmatter>(yaml_str)
                .ok()
                .map(KindleFrontmatter::from)
        })
}

/// Determine which books need syncing.
///
/// A book needs syncing if any of these are true:
/// 1. It's new (not in any local file, by bookId or ASIN)
/// 2. Its `lastAnnotatedDate` differs from the local frontmatter
/// 3. Its `lastAnnotatedDate` is on or after `last_sync_date - 1 day`
///    (safety net because Amazon dates have day-level granularity only)
pub fn books_to_sync(
    remote_books: &[Book],
    existing: &ExistingFiles,
    last_sync_date: Option<NaiveDate>,
) -> Vec<Book> {
    let mut seen = std::collections::HashSet::new();

    remote_books
        .iter()
        .filter(|book| {
            if !seen.insert(&book.id) {
                return false; // dedup
            }

            match existing.find(book) {
                None => true, // New book
                Some(existing_file) => {
                    let remote_date = book.last_annotated_date;
                    let local_date = existing_file
                        .frontmatter
                        .last_annotated_date
                        .as_deref()
                        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

                    // Criterion 2: annotation date changed
                    let date_changed = match (remote_date, local_date) {
                        (Some(r), Some(l)) => r != l,
                        (Some(_), None) => true,
                        _ => false,
                    };

                    // Criterion 3: annotated since last sync (minus 1 day safety margin)
                    let updated_since_sync = match (remote_date, last_sync_date) {
                        (Some(r), Some(sync)) => {
                            let threshold = sync - chrono::Duration::days(1);
                            r >= threshold
                        }
                        _ => false,
                    };

                    date_changed || updated_since_sync
                }
            }
        })
        .cloned()
        .collect()
}

/// Sync a book's highlights to disk, either creating a new file or
/// incrementally updating an existing one.
pub fn sync_book(
    entry: &BookHighlights,
    output_dir: &Path,
    existing: Option<&ExistingFile>,
    file_template: Option<&str>,
    highlight_template: Option<&str>,
    filename_template: Option<&str>,
    frontmatter_format: &str,
) -> Result<PathBuf> {
    fs::create_dir_all(output_dir).context("Failed to create output directory")?;

    match existing {
        Some(existing_file) => {
            // Incremental update: diff and insert new highlights
            let updated =
                diff_and_merge(entry, existing_file, highlight_template, frontmatter_format)?;
            fs::write(&existing_file.path, &updated)
                .with_context(|| format!("Failed to write {}", existing_file.path.display()))?;
            Ok(existing_file.path.clone())
        }
        None => {
            // New file
            let filename = crate::scraper::book_filename(
                &entry.book,
                entry.metadata.as_ref(),
                filename_template,
            );
            let path = output_dir.join(&filename);

            // Handle duplicate filenames
            let path = if path.exists() {
                let stem = path.file_stem().unwrap().to_string_lossy();
                let timestamp = chrono::Utc::now().timestamp_millis();
                output_dir.join(format!("{stem}-{timestamp}.md"))
            } else {
                path
            };

            let content = renderer::render_book_file(
                entry,
                file_template,
                highlight_template,
                frontmatter_format,
            )?;
            fs::write(&path, &content)
                .with_context(|| format!("Failed to write {}", path.display()))?;
            Ok(path)
        }
    }
}

/// Diff remote highlights against an existing local file and merge in new ones.
fn diff_and_merge(
    entry: &BookHighlights,
    existing: &ExistingFile,
    highlight_template: Option<&str>,
    frontmatter_format: &str,
) -> Result<String> {
    let local_highlights = parse_rendered_highlights(&existing.content);
    let diffs = diff_highlights(&entry.highlights, &local_highlights);

    if diffs.is_empty() {
        // No new highlights, just update frontmatter
        return Ok(update_frontmatter(
            &existing.content,
            &entry.book,
            entry.highlights.len(),
            frontmatter_format,
        ));
    }

    let mut lines: Vec<String> = existing.content.lines().map(|l| l.to_string()).collect();

    // Process diffs in reverse order so line numbers remain valid
    let mut insertions: Vec<(usize, String)> = Vec::new();
    let mut appends: Vec<String> = Vec::new();

    for diff in &diffs {
        let rendered =
            renderer::render_single_highlight(&diff.highlight, &entry.book, highlight_template)?;

        match diff.insert_before_line {
            Some(line) => insertions.push((line, rendered)),
            None => appends.push(rendered),
        }
    }

    // Sort insertions by line number descending so we insert from bottom up
    insertions.sort_by(|a, b| b.0.cmp(&a.0));

    for (line_num, content) in insertions {
        let insert_lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        // Insert before the given line (0-indexed)
        let idx = line_num.saturating_sub(1);
        for (i, insert_line) in insert_lines.into_iter().enumerate() {
            lines.insert(idx + i, insert_line);
        }
    }

    // Append remaining highlights at end
    for content in appends {
        // Ensure there's a newline before appending
        if !lines.last().is_none_or(|l| l.is_empty()) {
            lines.push(String::new());
        }
        for line in content.lines() {
            lines.push(line.to_string());
        }
    }

    let mut result = lines.join("\n");

    // Update frontmatter
    result = update_frontmatter(
        &result,
        &entry.book,
        entry.highlights.len(),
        frontmatter_format,
    );

    Ok(result)
}

/// Parse existing ^ref-{id} block references from file content.
fn parse_rendered_highlights(content: &str) -> Vec<RenderedHighlight> {
    let re = Regex::new(r"\^ref-(\S+)\s*$").unwrap();
    let mut highlights = Vec::new();

    for (i, line) in content.lines().enumerate() {
        if let Some(caps) = re.captures(line) {
            highlights.push(RenderedHighlight {
                line: i + 1, // 1-indexed
                highlight_id: caps[1].to_string(),
            });
        }
    }

    highlights
}

/// Find new highlights not present locally and determine insertion points.
fn diff_highlights(remote: &[Highlight], local: &[RenderedHighlight]) -> Vec<DiffResult> {
    let local_ids: std::collections::HashSet<&str> =
        local.iter().map(|h| h.highlight_id.as_str()).collect();

    // Find highlights that don't exist locally
    let new_highlights: Vec<&Highlight> = remote
        .iter()
        .filter(|h| !local_ids.contains(h.id.as_str()))
        .collect();

    if new_highlights.is_empty() {
        return Vec::new();
    }

    // Build ordered state: for each remote highlight, note if it exists locally
    let remote_ids: Vec<(&str, bool)> = remote
        .iter()
        .map(|h| (h.id.as_str(), local_ids.contains(h.id.as_str())))
        .collect();

    new_highlights
        .into_iter()
        .map(|highlight| {
            // Find the next existing highlight after this one in the remote order
            let pos = remote_ids
                .iter()
                .position(|(id, _)| *id == highlight.id)
                .unwrap();

            let next_existing = remote_ids[pos + 1..].iter().find(|(_, exists)| *exists);

            let insert_before_line = next_existing.and_then(|(next_id, _)| {
                local
                    .iter()
                    .find(|lh| lh.highlight_id == *next_id)
                    .map(|lh| lh.line)
            });

            DiffResult {
                highlight: highlight.clone(),
                insert_before_line,
            }
        })
        .collect()
}

/// Update the frontmatter in a file's content.
fn update_frontmatter(
    content: &str,
    book: &Book,
    highlights_count: usize,
    frontmatter_format: &str,
) -> String {
    let new_fm = renderer::render_frontmatter(book, highlights_count, frontmatter_format);

    if let Some(rest) = content.strip_prefix("---")
        && let Some(end) = rest.find("---")
    {
        let after_fm = rest[end + 3..].trim_start_matches('\n');
        return format!("{new_fm}{after_fm}");
    }

    // No existing frontmatter, prepend
    format!("{new_fm}{content}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let content = "---\nkindle-bookId: '12345'\nkindle-title: 'Test'\nkindle-author: Test Author\nkindle-highlightsCount: 5\n---\n# Test\n";
        let fm = parse_frontmatter(content).unwrap();
        assert_eq!(fm.book_id, "12345");
        assert_eq!(fm.title, "Test");
        assert_eq!(fm.highlights_count, 5);
    }

    #[test]
    fn test_parse_frontmatter_legacy() {
        let content = "---\nkindle-sync:\n  bookId: '213456241'\n  title: 'Atomic Habits'\n  author: James Clear\n  asin: B01N5AX61W\n  bookImageUrl: 'https://example.com/cover.jpg'\n  highlightsCount: 97\n---\n# Atomic Habits\n";
        let fm = parse_frontmatter(content).unwrap();
        assert_eq!(fm.book_id, "213456241");
        assert_eq!(fm.title, "Atomic Habits");
        assert_eq!(fm.author, "James Clear");
        assert_eq!(fm.asin.as_deref(), Some("B01N5AX61W"));
        assert_eq!(fm.highlights_count, 97);
    }

    #[test]
    fn test_parse_frontmatter_legacy_real_file() {
        // Exact content from a real nested-format file produced by the original renderer
        let content = "\
---
kindle-sync:
  bookId: '866356410'
  title: 'C# 12 in a Nutshell: The Definitive Reference'
  author: Joseph Albahari
  asin: B0CN83NT9L
  bookImageUrl: 'https://m.media-amazon.com/images/I/71uN6eGMuFL._SY160.jpg'
  highlightsCount: 15
---

# C# 12 in a Nutshell
## Metadata
";
        let fm = parse_frontmatter(content);
        assert!(
            fm.is_some(),
            "parse_frontmatter returned None for nested format"
        );
        let fm = fm.unwrap();
        assert_eq!(fm.book_id, "866356410");
        assert_eq!(fm.title, "C# 12 in a Nutshell: The Definitive Reference");
        assert_eq!(fm.author, "Joseph Albahari");
        assert_eq!(fm.asin.as_deref(), Some("B0CN83NT9L"));
        assert_eq!(fm.highlights_count, 15);
    }

    #[test]
    fn test_parse_frontmatter_roundtrip_flat() {
        let book = Book {
            id: "866356410".to_string(),
            title: "C# 12 in a Nutshell: The Definitive Reference".to_string(),
            author: "Joseph Albahari".to_string(),
            asin: Some("B0CN83NT9L".to_string()),
            url: None,
            image_url: Some("https://example.com/cover.jpg".to_string()),
            last_annotated_date: Some(NaiveDate::from_ymd_opt(2024, 8, 27).unwrap()),
        };
        let fm_str = renderer::render_frontmatter(&book, 15, "flat");
        let content = format!("{fm_str}\n# Test\n");
        let fm = parse_frontmatter(&content).unwrap();
        assert_eq!(fm.book_id, "866356410");
        assert_eq!(fm.asin.as_deref(), Some("B0CN83NT9L"));
        assert_eq!(fm.highlights_count, 15);
    }

    #[test]
    fn test_parse_frontmatter_roundtrip_nested() {
        let book = Book {
            id: "866356410".to_string(),
            title: "C# 12 in a Nutshell: The Definitive Reference".to_string(),
            author: "Joseph Albahari".to_string(),
            asin: Some("B0CN83NT9L".to_string()),
            url: None,
            image_url: Some("https://example.com/cover.jpg".to_string()),
            last_annotated_date: Some(NaiveDate::from_ymd_opt(2024, 8, 27).unwrap()),
        };
        let fm_str = renderer::render_frontmatter(&book, 15, "nested");
        let content = format!("{fm_str}\n# Test\n");
        let fm = parse_frontmatter(&content).unwrap();
        assert_eq!(fm.book_id, "866356410");
        assert_eq!(fm.asin.as_deref(), Some("B0CN83NT9L"));
        assert_eq!(fm.highlights_count, 15);
    }

    #[test]
    fn test_parse_rendered_highlights() {
        let content = "some text ^ref-12345\nother line\nanother highlight ^ref-67890\n";
        let highlights = parse_rendered_highlights(content);
        assert_eq!(highlights.len(), 2);
        assert_eq!(highlights[0].highlight_id, "12345");
        assert_eq!(highlights[0].line, 1);
        assert_eq!(highlights[1].highlight_id, "67890");
        assert_eq!(highlights[1].line, 3);
    }

    #[test]
    fn test_diff_highlights_all_new() {
        let remote = vec![
            Highlight {
                id: "1".to_string(),
                text: Some("First".to_string()),
                location: None,
                page: None,
                note: None,
                color: None,
            },
            Highlight {
                id: "2".to_string(),
                text: Some("Second".to_string()),
                location: None,
                page: None,
                note: None,
                color: None,
            },
        ];
        let local: Vec<RenderedHighlight> = vec![];
        let diffs = diff_highlights(&remote, &local);
        assert_eq!(diffs.len(), 2);
        // All should append (no existing neighbors)
        assert!(diffs[0].insert_before_line.is_none());
        assert!(diffs[1].insert_before_line.is_none());
    }

    #[test]
    fn test_diff_highlights_some_existing() {
        let remote = vec![
            Highlight {
                id: "1".to_string(),
                text: Some("First".to_string()),
                location: None,
                page: None,
                note: None,
                color: None,
            },
            Highlight {
                id: "2".to_string(),
                text: Some("Second (new)".to_string()),
                location: None,
                page: None,
                note: None,
                color: None,
            },
            Highlight {
                id: "3".to_string(),
                text: Some("Third".to_string()),
                location: None,
                page: None,
                note: None,
                color: None,
            },
        ];
        let local = vec![
            RenderedHighlight {
                line: 5,
                highlight_id: "1".to_string(),
            },
            RenderedHighlight {
                line: 10,
                highlight_id: "3".to_string(),
            },
        ];
        let diffs = diff_highlights(&remote, &local);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].highlight.id, "2");
        // Should insert before highlight "3" (line 10)
        assert_eq!(diffs[0].insert_before_line, Some(10));
    }

    #[test]
    fn test_update_frontmatter() {
        let content = "---\nkindle-bookId: '12345'\nkindle-title: 'Test'\nkindle-author: Author\nkindle-highlightsCount: 5\n---\n# Test\nContent here\n";
        let book = Book {
            id: "12345".to_string(),
            title: "Test".to_string(),
            author: "Author".to_string(),
            asin: None,
            url: None,
            image_url: None,
            last_annotated_date: None,
        };
        let result = update_frontmatter(content, &book, 10, "flat");
        assert!(result.contains("kindle-highlightsCount: 10"));
        assert!(result.contains("---\n# Test"));
        assert!(
            !result.contains("---\n\n"),
            "should not have blank line after frontmatter"
        );
        assert!(result.contains("Content here"));
    }

    #[test]
    fn test_update_frontmatter_nested() {
        let content = "---\nkindle-sync:\n  bookId: '12345'\n  title: 'Test'\n  author: Author\n  highlightsCount: 5\n---\n# Test\nContent here\n";
        let book = Book {
            id: "12345".to_string(),
            title: "Test".to_string(),
            author: "Author".to_string(),
            asin: None,
            url: None,
            image_url: None,
            last_annotated_date: None,
        };
        let result = update_frontmatter(content, &book, 10, "nested");
        assert!(result.contains("kindle-sync:\n"));
        assert!(result.contains("  highlightsCount: 10"));
        assert!(result.contains("---\n# Test"));
        assert!(
            !result.contains("---\n\n"),
            "should not have blank line after frontmatter"
        );
        assert!(result.contains("Content here"));
    }

    fn make_existing_files(entries: Vec<(String, ExistingFile)>) -> ExistingFiles {
        let mut by_book_id = HashMap::new();
        let mut by_asin: HashMap<String, String> = HashMap::new();
        for (book_id, file) in entries {
            if let Some(ref asin) = file.frontmatter.asin {
                by_asin
                    .entry(asin.clone())
                    .or_insert_with(|| book_id.clone());
            }
            by_book_id.insert(book_id, file);
        }
        ExistingFiles {
            by_book_id,
            by_asin,
        }
    }

    #[test]
    fn test_books_to_sync_new_book() {
        let remote = vec![Book {
            id: "new".to_string(),
            title: "New Book".to_string(),
            author: "Author".to_string(),
            asin: None,
            url: None,
            image_url: None,
            last_annotated_date: None,
        }];
        let existing = make_existing_files(vec![]);
        let to_sync = books_to_sync(&remote, &existing, None);
        assert_eq!(to_sync.len(), 1);
    }

    #[test]
    fn test_books_to_sync_asin_fallback() {
        let remote = vec![Book {
            id: "new-id".to_string(),
            title: "Book (New Edition)".to_string(),
            author: "Author".to_string(),
            asin: Some("B01TEST".to_string()),
            url: None,
            image_url: None,
            last_annotated_date: Some(NaiveDate::from_ymd_opt(2024, 8, 27).unwrap()),
        }];
        let existing = make_existing_files(vec![(
            "old-id".to_string(),
            ExistingFile {
                path: PathBuf::from("test.md"),
                frontmatter: KindleFrontmatter {
                    book_id: "old-id".to_string(),
                    title: "Book".to_string(),
                    author: "Author".to_string(),
                    asin: Some("B01TEST".to_string()),
                    last_annotated_date: Some("2024-08-27".to_string()),
                    book_image_url: None,
                    highlights_count: 5,
                    last_sync_date: None,
                },
                content: String::new(),
            },
        )]);

        // Different bookId but same ASIN — should match via fallback, not create new
        let to_sync = books_to_sync(&remote, &existing, None);
        assert_eq!(to_sync.len(), 0);
    }

    #[test]
    fn test_books_to_sync_updated_since_last_sync() {
        let today = NaiveDate::from_ymd_opt(2024, 8, 27).unwrap();
        let remote = vec![Book {
            id: "existing".to_string(),
            title: "Book".to_string(),
            author: "Author".to_string(),
            asin: None,
            url: None,
            image_url: None,
            last_annotated_date: Some(today),
        }];
        let existing = make_existing_files(vec![(
            "existing".to_string(),
            ExistingFile {
                path: PathBuf::from("test.md"),
                frontmatter: KindleFrontmatter {
                    book_id: "existing".to_string(),
                    title: "Book".to_string(),
                    author: "Author".to_string(),
                    asin: None,
                    last_annotated_date: Some("2024-08-27".to_string()),
                    book_image_url: None,
                    highlights_count: 5,
                    last_sync_date: None,
                },
                content: String::new(),
            },
        )]);

        // Same date, no last_sync_date → skip
        let to_sync = books_to_sync(&remote, &existing, None);
        assert_eq!(to_sync.len(), 0);

        // Same date, but synced today → include (safety net)
        let to_sync = books_to_sync(&remote, &existing, Some(today));
        assert_eq!(to_sync.len(), 1);

        // Same date, synced long after the annotation → skip
        let later_sync = NaiveDate::from_ymd_opt(2024, 9, 15).unwrap();
        let to_sync = books_to_sync(&remote, &existing, Some(later_sync));
        assert_eq!(to_sync.len(), 0);
    }
}
