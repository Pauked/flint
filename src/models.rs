use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Book {
    pub id: String,
    pub title: String,
    pub author: String,
    pub asin: Option<String>,
    pub url: Option<String>,
    pub image_url: Option<String>,
    pub last_annotated_date: Option<NaiveDate>,
}

#[derive(Debug, Clone)]
pub struct Highlight {
    pub id: String,
    pub text: Option<String>,
    pub location: Option<String>,
    pub page: Option<String>,
    pub note: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BookHighlights {
    pub book: Book,
    pub highlights: Vec<Highlight>,
    pub metadata: Option<BookMetadata>,
}

#[derive(Debug, Clone, Default)]
pub struct BookMetadata {
    pub isbn: Option<String>,
    pub pages: Option<String>,
    pub publication_date: Option<String>,
    pub publisher: Option<String>,
    pub author_url: Option<String>,
}

/// Flat Obsidian properties for kindle metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindleFrontmatter {
    #[serde(rename = "kindle-bookId")]
    pub book_id: String,
    #[serde(rename = "kindle-title")]
    pub title: String,
    #[serde(rename = "kindle-author")]
    pub author: String,
    #[serde(rename = "kindle-asin", default)]
    pub asin: Option<String>,
    #[serde(rename = "kindle-lastAnnotatedDate", default)]
    pub last_annotated_date: Option<String>,
    #[serde(rename = "kindle-bookImageUrl", default)]
    pub book_image_url: Option<String>,
    #[serde(rename = "kindle-highlightsCount")]
    pub highlights_count: usize,
}

/// Legacy nested format from the Obsidian Kindle plugin (`kindle-sync:` wrapper).
#[derive(Debug, Clone, Deserialize)]
pub struct LegacyKindleFrontmatter {
    #[serde(rename = "kindle-sync")]
    pub kindle_sync: LegacyKindleSync,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LegacyKindleSync {
    #[serde(rename = "bookId")]
    pub book_id: String,
    pub title: String,
    pub author: String,
    #[serde(default)]
    pub asin: Option<String>,
    #[serde(rename = "lastAnnotatedDate", default)]
    pub last_annotated_date: Option<String>,
    #[serde(rename = "bookImageUrl", default)]
    pub book_image_url: Option<String>,
    #[serde(rename = "highlightsCount", default)]
    pub highlights_count: usize,
}

impl From<LegacyKindleFrontmatter> for KindleFrontmatter {
    fn from(legacy: LegacyKindleFrontmatter) -> Self {
        let s = legacy.kindle_sync;
        Self {
            book_id: s.book_id,
            title: s.title,
            author: s.author,
            asin: s.asin,
            last_annotated_date: s.last_annotated_date,
            book_image_url: s.book_image_url,
            highlights_count: s.highlights_count,
        }
    }
}
