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
    #[serde(rename = "kindle-book-id")]
    pub book_id: String,
    #[serde(rename = "kindle-title")]
    pub title: String,
    #[serde(rename = "kindle-author")]
    pub author: String,
    #[serde(rename = "kindle-asin", default)]
    pub asin: Option<String>,
    #[serde(rename = "kindle-last-annotated-date", default)]
    pub last_annotated_date: Option<String>,
    #[serde(rename = "kindle-book-image-url", default)]
    pub book_image_url: Option<String>,
    #[serde(rename = "kindle-highlights-count")]
    pub highlights_count: usize,
    #[serde(rename = "flint-last-sync-date", default)]
    pub last_sync_date: Option<String>,
}
