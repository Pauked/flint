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
    pub text: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindleFrontmatter {
    #[serde(rename = "bookId")]
    pub book_id: String,
    pub title: String,
    pub author: String,
    pub asin: Option<String>,
    #[serde(rename = "lastAnnotatedDate")]
    pub last_annotated_date: Option<String>,
    #[serde(rename = "bookImageUrl")]
    pub book_image_url: Option<String>,
    #[serde(rename = "highlightsCount")]
    pub highlights_count: usize,
}

/// Wrapper for the YAML frontmatter structure: `kindle-sync: { ... }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontmatterWrapper {
    #[serde(rename = "kindle-sync")]
    pub kindle_sync: KindleFrontmatter,
}
