use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use headless_chrome::protocol::cdp::Network::CookieParam;
use headless_chrome::{Browser, LaunchOptions};
use log::{debug, info};
use regex::Regex;
use reqwest::blocking::Client;
use scraper::{Html, Selector};

use crate::auth;
use crate::config::AmazonRegion;
use crate::models::{Book, BookMetadata, Highlight};

// ── Archive support ─────────────────────────────────────────────────────

/// An archive directory for caching raw Amazon HTML responses.
pub struct Archive {
    dir: PathBuf,
}

impl Archive {
    pub fn new(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf() }
    }

    fn notebook_path(&self) -> PathBuf {
        self.dir.join("notebook.html")
    }

    fn highlights_dir(&self) -> PathBuf {
        self.dir.join("highlights")
    }

    fn highlights_path(&self, asin: &str, page: usize) -> PathBuf {
        self.highlights_dir().join(format!("{asin}_page{page}.html"))
    }

    fn metadata_path(&self, asin: &str) -> PathBuf {
        self.dir.join("metadata").join(format!("{asin}.html"))
    }

    fn read(&self, path: &Path) -> Option<String> {
        fs::read_to_string(path).ok()
    }

    fn write(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        Ok(())
    }
}

/// Hash a string using FNV-1a and return a truncated decimal string.
/// Replaces the original plugin's fletcher16 (16-bit, collision-prone).
fn hash_id(value: &str) -> String {
    use std::hash::Hasher;
    let mut hasher = fnv::FnvHasher::default();
    hasher.write(value.to_lowercase().as_bytes());
    let h = hasher.finish();
    // Truncate to 32 bits for a compact ID (same order of magnitude as fletcher16
    // but far fewer collisions)
    (h as u32).to_string()
}

/// Strip author prefix like "By: ", "Par: ", "Von: ", etc.
fn parse_author(raw: &str) -> String {
    let re = Regex::new(r"^.*:\s*").unwrap();
    re.replace(raw, "").trim().to_string()
}

/// Strip Unicode control characters (LRM, RLM, etc.) and excess whitespace from Amazon metadata.
fn clean_metadata_value(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() && !matches!(c, '\u{200E}' | '\u{200F}' | '\u{200B}' | '\u{FEFF}'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse a date string from Amazon based on region.
fn parse_date(date_str: &str, region: &AmazonRegion) -> Option<NaiveDate> {
    let trimmed = date_str.trim();
    if trimmed.is_empty() {
        return None;
    }

    match region.name {
        "japan" => {
            // Format: "2021年11月15日 月曜日" → extract digits
            let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 8 {
                NaiveDate::parse_from_str(&digits[..8], "%Y%m%d").ok()
            } else {
                None
            }
        }
        _ => {
            // English and most regions: "Sunday October 24, 2021"
            // Try multiple formats, stripping the leading day name
            let date_part = if let Some(pos) = trimmed.find(' ') {
                // Try to detect if first word is a day name by checking if
                // removing it yields a parseable date
                // Try the part after the first word first (strips day name)
                &trimmed[pos + 1..]
            } else {
                trimmed
            };

            // English: "October 24, 2021"
            NaiveDate::parse_from_str(date_part, "%B %d, %Y")
                .or_else(|_| NaiveDate::parse_from_str(date_part, "%B %d,%Y"))
                // French: "août 30, 2022"
                .or_else(|_| NaiveDate::parse_from_str(date_part, "%b %d, %Y"))
                // Fallback: try the full string
                .or_else(|_| NaiveDate::parse_from_str(trimmed, "%B %d, %Y"))
                // ISO format
                .or_else(|_| NaiveDate::parse_from_str(trimmed, "%Y-%m-%d"))
                .ok()
        }
    }
}

/// Shorten a book title by removing parenthetical content, text after the
/// last colon, single quotes, and replacing square brackets.
pub fn shorten_title(title: &str) -> String {
    let re_parens = Regex::new(r" *\([^)]*\) *").unwrap();
    let re_colon = Regex::new(r":([^:]*)$").unwrap();

    let result = re_parens.replace_all(title, "");
    let result = re_colon.replace(&result, "");
    result
        .replace(['\'', '\u{2019}'], "")
        .replace('[', "(")
        .replace(']', ")")
        .trim()
        .to_string()
}

// ── Book list scraping ──────────────────────────────────────────────────

/// Scrape the list of books from the Kindle notebook page.
pub fn scrape_books(html: &str, region: &AmazonRegion) -> Result<Vec<Book>> {
    let document = Html::parse_document(html);
    let book_sel = Selector::parse(".kp-notebook-library-each-book").unwrap();
    let title_sel = Selector::parse("h2.kp-notebook-searchable").unwrap();
    let author_sel = Selector::parse("p.kp-notebook-searchable").unwrap();
    let image_sel = Selector::parse(".kp-notebook-cover-image[src]").unwrap();
    let date_sel = Selector::parse("[id^=\"kp-notebook-annotated-date\"]").unwrap();

    let mut books = Vec::new();

    for book_el in document.select(&book_sel) {
        let title = book_el
            .select(&title_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        if title.is_empty() {
            continue;
        }

        let asin = book_el.value().id().map(|s| s.to_string());

        let raw_author = book_el
            .select(&author_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        let author = parse_author(&raw_author);

        let image_url = book_el
            .select(&image_sel)
            .next()
            .and_then(|el| el.value().attr("src").map(|s| s.to_string()));

        let last_annotated_date = book_el
            .select(&date_sel)
            .next()
            .and_then(|el| el.value().attr("value").map(|s| s.to_string()))
            .and_then(|s| parse_date(&s, region));

        let id = hash_id(&title);
        let url = asin.as_ref().map(|a| format!("https://www.amazon.com/dp/{a}"));

        books.push(Book {
            id,
            title,
            author,
            asin,
            url,
            image_url,
            last_annotated_date,
        });
    }

    Ok(books)
}

/// Fetch the notebook page HTML. Reads from archive if available, otherwise
/// uses headless Chrome to load the page (needed because Amazon lazy-loads
/// the book list via JavaScript). Saves to archive when `save_archive` is set.
pub fn fetch_notebook_html(
    region: &AmazonRegion,
    use_archive: Option<&Archive>,
    save_archive: Option<&Archive>,
    data_dir: &Path,
) -> Result<String> {
    if let Some(archive) = use_archive {
        let path = archive.notebook_path();
        if let Some(html) = archive.read(&path) {
            debug!("Reading notebook from archive: {}", path.display());
            return Ok(html);
        }
        anyhow::bail!("Archive missing notebook.html at {}", path.display());
    }

    let cookies = auth::load_stored_cookies(data_dir)
        .context("No saved session. Run 'login' first")?;

    info!("Loading notebook page...");

    let launch_options = LaunchOptions {
        headless: true,
        idle_browser_timeout: Duration::from_secs(120),
        ..LaunchOptions::default()
    };
    let browser = Browser::new(launch_options).context("Failed to launch Chrome")?;
    let tab = browser.new_tab().context("Failed to open tab")?;

    // Set cookies before navigation
    let cookie_params: Vec<CookieParam> = cookies
        .iter()
        .map(|c| CookieParam {
            name: c.name.clone(),
            value: c.value.clone(),
            domain: Some(c.domain.clone()),
            url: None,
            path: None,
            secure: None,
            http_only: None,
            same_site: None,
            expires: None,
            priority: None,
            same_party: None,
            source_scheme: None,
            source_port: None,
            partition_key: None,
        })
        .collect();
    tab.set_cookies(cookie_params)
        .context("Failed to set cookies on browser")?;

    tab.navigate_to(region.notebook_url)
        .context("Failed to navigate to notebook page")?;
    tab.wait_until_navigated()
        .context("Notebook page did not finish loading")?;

    // Wait for at least one book to appear
    tab.wait_for_element_with_custom_timeout(
        ".kp-notebook-library-each-book",
        Duration::from_secs(15),
    )
    .context("Notebook page did not load books. Session may have expired — run 'login' again.")?;

    // Scroll to bottom repeatedly to load all books
    let mut prev_count: u64 = 0;
    let mut stable_rounds = 0;
    for _ in 0..100 {
        let count = tab
            .evaluate(
                "document.querySelectorAll('.kp-notebook-library-each-book').length",
                false,
            )
            .ok()
            .and_then(|r| r.value)
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if count == prev_count {
            stable_rounds += 1;
            if stable_rounds >= 3 {
                break;
            }
        } else {
            stable_rounds = 0;
        }
        prev_count = count;

        tab.evaluate("window.scrollTo(0, document.body.scrollHeight)", false)
            .ok();
        std::thread::sleep(Duration::from_millis(500));
    }

    info!("Found {} books in notebook.", prev_count);

    // Extract full rendered HTML
    let result = tab
        .evaluate("document.documentElement.outerHTML", false)
        .context("Failed to extract page HTML")?;
    let html = result
        .value
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .context("Failed to read page HTML from browser")?;

    if let Some(archive) = save_archive {
        archive.write(&archive.notebook_path(), &html).ok();
    }

    Ok(html)
}

// ── Highlight scraping ──────────────────────────────────────────────────

/// Scrape highlights for a single book, handling pagination.
pub fn scrape_book_highlights(
    client: &Client,
    region: &AmazonRegion,
    book: &Book,
    use_archive: Option<&Archive>,
    save_archive: Option<&Archive>,
) -> Result<Vec<Highlight>> {
    let asin = match &book.asin {
        Some(a) => a,
        None => return Ok(Vec::new()),
    };

    // Archive read path: read all saved pages
    if let Some(archive) = use_archive {
        let mut all_highlights = Vec::new();
        let mut page = 1;
        loop {
            let path = archive.highlights_path(asin, page);
            match archive.read(&path) {
                Some(html) => {
                    let document = Html::parse_document(&html);
                    all_highlights.extend(parse_highlights(&document)?);
                    page += 1;
                }
                None => break,
            }
        }
        return Ok(all_highlights);
    }

    // Live fetch path
    let mut all_highlights = Vec::new();
    let mut content_limit_state: Option<String> = None;
    let mut token: Option<String> = None;
    let mut page_num: usize = 1;

    loop {
        let mut url = format!("{}?asin={}", region.notebook_url, asin);
        if let Some(ref state) = content_limit_state {
            url.push_str(&format!("&contentLimitState={state}"));
        }
        if let Some(ref tok) = token {
            url.push_str(&format!("&token={tok}"));
        }

        let response = client
            .get(&url)
            .send()
            .with_context(|| format!("Failed to fetch highlights for {}", book.title))?;

        let html = response
            .text()
            .context("Failed to read highlights response")?;

        if let Some(archive) = save_archive {
            archive.write(&archive.highlights_path(asin, page_num), &html).ok();
        }

        let document = Html::parse_document(&html);

        let highlights = parse_highlights(&document)?;
        all_highlights.extend(highlights);

        // Check for pagination
        let limit_sel = Selector::parse(".kp-notebook-content-limit-state").unwrap();
        let next_sel = Selector::parse(".kp-notebook-annotations-next-page-start").unwrap();

        let new_state = document
            .select(&limit_sel)
            .next()
            .and_then(|el| el.value().attr("value").map(|s| s.to_string()));

        let new_token = document
            .select(&next_sel)
            .next()
            .and_then(|el| el.value().attr("value").map(|s| s.to_string()))
            .filter(|s| !s.is_empty());

        if new_token.is_some() {
            content_limit_state = new_state;
            token = new_token;
            page_num += 1;
        } else {
            break;
        }
    }

    Ok(all_highlights)
}

/// Parse highlights from a single page of HTML.
fn parse_highlights(document: &Html) -> Result<Vec<Highlight>> {
    let row_sel = Selector::parse(".a-row.a-spacing-base").unwrap();
    let text_sel = Selector::parse("#highlight").unwrap();
    let color_sel = Selector::parse(".kp-notebook-highlight").unwrap();
    let location_sel = Selector::parse("#kp-annotation-location").unwrap();
    let header_sel = Selector::parse("#annotationNoteHeader").unwrap();
    let note_sel = Selector::parse("#note").unwrap();

    let page_re = Regex::new(r"\d+$").unwrap();
    let color_re = Regex::new(r"kp-notebook-highlight-(\w+)").unwrap();
    let br_re = Regex::new(r"(?i)<br\s*/?>").unwrap();
    let tag_re = Regex::new(r"<[^>]+>").unwrap();

    let mut highlights = Vec::new();

    for row in document.select(&row_sel) {
        let text = match row
            .select(&text_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
        {
            Some(t) if !t.is_empty() => t,
            _ => continue,
        };

        let color = row
            .select(&color_sel)
            .next()
            .and_then(|el| {
                let classes = el.value().attr("class").unwrap_or("");
                color_re
                    .captures(classes)
                    .map(|c| c[1].to_string())
            });

        let location = row
            .select(&location_sel)
            .next()
            .and_then(|el| el.value().attr("value").map(|s| s.to_string()));

        let page = row
            .select(&header_sel)
            .next()
            .map(|el| el.text().collect::<String>())
            .and_then(|t| page_re.find(&t).map(|m| m.as_str().to_string()));

        let note = row
            .select(&note_sel)
            .next()
            .map(|el| {
                let inner_html = el.inner_html();
                let converted = br_re.replace_all(&inner_html, "\n");
                tag_re.replace_all(&converted, "").trim().to_string()
            })
            .filter(|s| !s.is_empty());

        let id = hash_id(&text);

        highlights.push(Highlight {
            id,
            text,
            location,
            page,
            note,
            color,
        });
    }

    Ok(highlights)
}

// ── Metadata scraping ───────────────────────────────────────────────────

/// Scrape optional book metadata from the Amazon product page.
pub fn scrape_book_metadata(
    client: &Client,
    book: &Book,
    use_archive: Option<&Archive>,
    save_archive: Option<&Archive>,
) -> Result<BookMetadata> {
    let asin = match &book.asin {
        Some(a) => a,
        None => return Ok(BookMetadata::default()),
    };

    let html = if let Some(archive) = use_archive {
        match archive.read(&archive.metadata_path(asin)) {
            Some(h) => h,
            None => return Ok(BookMetadata::default()),
        }
    } else {
        let url = format!("https://www.amazon.com/dp/{asin}");
        let response = client.get(&url).send();

        let fetched = match response {
            Ok(resp) => match resp.text() {
                Ok(text) => text,
                Err(_) => return Ok(BookMetadata::default()),
            },
            Err(_) => return Ok(BookMetadata::default()),
        };

        if let Some(archive) = save_archive {
            archive.write(&archive.metadata_path(asin), &fetched).ok();
        }

        fetched
    };

    let document = Html::parse_document(&html);
    let mut metadata = BookMetadata::default();

    // Parse detail bullets for ISBN, pages, publication date, publisher
    let detail_sel =
        Selector::parse("#detailBullets_feature_div .detail-bullet-list li span.a-list-item")
            .unwrap();

    let isbn_re = Regex::new(r"\b[\dX]{10,13}\b").unwrap();
    let pages_re = Regex::new(r"(\d+)\s*pages").unwrap();

    for item in document.select(&detail_sel) {
        let text: String = item.text().collect::<String>();
        let text = text.trim();

        if text.contains("ISBN") && metadata.isbn.is_none() {
            if let Some(m) = isbn_re.find(text) {
                metadata.isbn = Some(m.as_str().to_string());
            }
        } else if text.contains("Print length") || text.contains("Pages") {
            if let Some(caps) = pages_re.captures(text) {
                metadata.pages = Some(caps[1].to_string());
            }
        } else if text.contains("Publication date") || text.contains("Publisher") {
            if text.contains("Publication") {
                let parts: Vec<&str> = text.splitn(2, ':').collect();
                if parts.len() > 1 {
                    metadata.publication_date = Some(clean_metadata_value(parts[1]));
                }
            }
            if text.contains("Publisher") {
                let parts: Vec<&str> = text.splitn(2, ':').collect();
                if parts.len() > 1 {
                    metadata.publisher = Some(clean_metadata_value(parts[1]));
                }
            }
        }
    }

    // Try to get ISBN from popover data
    if metadata.isbn.is_none()
        && let Ok(sel) = Selector::parse(
            "#printEditionIsbn_feature_div .a-row:first-child span:nth-child(2)",
        )
        && let Some(el) = document.select(&sel).next()
    {
        let isbn_text = el.text().collect::<String>().trim().to_string();
        if !isbn_text.is_empty() {
            metadata.isbn = Some(isbn_text);
        }
    }

    // Author URL — fix the original plugin bug: only set if href is actually present
    if let Ok(sel) = Selector::parse(".contributorNameID[href]")
        && let Some(el) = document.select(&sel).next()
        && let Some(href) = el.value().attr("href")
        && !href.is_empty()
        && href != "undefined"
    {
        metadata.author_url = Some(format!("https://www.amazon.com{href}"));
    }

    Ok(metadata)
}

// ── Author parsing utilities ────────────────────────────────────────────

struct ParsedAuthor {
    last_name: String,
}

/// Parse an author string into structured names.
/// Handles "First Last", "Last, First", and multiple authors separated by
/// "and", ",", or ";".
fn parse_authors(author: &str) -> Vec<ParsedAuthor> {
    if author.is_empty() {
        return vec![ParsedAuthor {
            last_name: String::new(),
        }];
    }

    let and_re = Regex::new(r"(?i)\b(and)\b").unwrap();

    if and_re.is_match(author) {
        let split_re = Regex::new(r"(?i)\band\b|,").unwrap();
        return split_re
            .split(author)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(parse_single_author)
            .collect();
    }

    if author.contains(';') {
        return author.split(';').map(|s| parse_single_author(s.trim())).collect();
    }

    vec![parse_single_author(author)]
}

fn parse_single_author(author: &str) -> ParsedAuthor {
    let author = author.trim().trim_end_matches('.');

    if author.contains(',') {
        let parts: Vec<&str> = author.splitn(2, ',').collect();
        ParsedAuthor {
            last_name: parts[0].trim().to_string(),
        }
    } else {
        let parts: Vec<&str> = author.split_whitespace().collect();
        ParsedAuthor {
            last_name: parts.last().unwrap_or(&"").to_string(),
        }
    }
}

/// Generate the `authorsLastNames` string used in filenames.
/// - 1 author: "LastName"
/// - 2 authors: "LastName1-LastName2"
/// - 3+ authors: "LastName1_et_al"
pub fn authors_last_names(author: &str) -> String {
    let authors = parse_authors(author);
    if authors.is_empty() {
        return String::new();
    }

    let mut result = authors[0].last_name.clone();
    if authors.len() == 2 {
        result.push('-');
        result.push_str(&authors[1].last_name);
    } else if authors.len() > 2 {
        result.push_str("_et_al");
    }

    result
}

/// Generate a kindle:// deep link.
pub fn kindle_app_link(asin: &str, location: Option<&str>) -> String {
    match location {
        Some(loc) => format!("kindle://book?action=open&asin={asin}&location={loc}"),
        None => format!("kindle://book?action=open&asin={asin}"),
    }
}

/// Sanitize a string for use as a filename.
/// Strips OS-invalid chars and Obsidian-invalid chars (# ^ [ ] |).
pub fn sanitize_filename(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '#' => result.push_str("Sharp"),
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            | '^' | '[' | ']' => result.push('_'),
            _ => result.push(c),
        }
    }
    result
}

/// Generate a filename for a book.
pub fn book_filename(book: &Book, template: Option<&str>) -> String {
    let filename = match template {
        Some(tmpl) => {
            let last_names = authors_last_names(&book.author);
            let title = shorten_title(&book.title);
            tmpl.replace("{{authors_last_names}}", &last_names)
                .replace("{{title}}", &title)
        }
        None => {
            let last_names = authors_last_names(&book.author);
            let title = shorten_title(&book.title);
            format!("{last_names}-{title}")
        }
    };

    format!("{}.md", sanitize_filename(&filename))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_id() {
        let id = hash_id("some title");
        assert!(!id.is_empty());
        // Same input produces same output
        assert_eq!(id, hash_id("some title"));
        // Case insensitive
        assert_eq!(hash_id("Hello"), hash_id("hello"));
    }

    #[test]
    fn test_parse_author_strips_prefix() {
        assert_eq!(parse_author("By: John Doe"), "John Doe");
        assert_eq!(parse_author("Par: Jean Dupont"), "Jean Dupont");
        assert_eq!(parse_author("John Doe"), "John Doe");
    }

    #[test]
    fn test_parse_date_english() {
        let region = &crate::config::REGIONS[0]; // global
        let date = parse_date("Sunday October 24, 2021", region);
        assert_eq!(date, Some(NaiveDate::from_ymd_opt(2021, 10, 24).unwrap()));
    }

    #[test]
    fn test_parse_date_japan() {
        let region = crate::config::get_region("japan").unwrap();
        let date = parse_date("2021年11月15日 月曜日", region);
        assert_eq!(date, Some(NaiveDate::from_ymd_opt(2021, 11, 15).unwrap()));
    }

    #[test]
    fn test_shorten_title() {
        assert_eq!(shorten_title("Title (Subtitle)"), "Title");
        assert_eq!(
            shorten_title("Title: A Long Subtitle"),
            "Title"
        );
        assert_eq!(shorten_title("It's a Test"), "Its a Test");
        assert_eq!(shorten_title("Title [Series]"), "Title (Series)");
    }

    #[test]
    fn test_authors_last_names_single() {
        assert_eq!(authors_last_names("James Clear"), "Clear");
    }

    #[test]
    fn test_authors_last_names_two() {
        assert_eq!(
            authors_last_names("Jim Blandy and Jason Orendorff"),
            "Blandy-Orendorff"
        );
    }

    #[test]
    fn test_authors_last_names_three_plus() {
        assert_eq!(
            authors_last_names("Jim Blandy, Jason Orendorff, and Leonora Tindall"),
            "Blandy_et_al"
        );
    }

    #[test]
    fn test_kindle_app_link() {
        assert_eq!(
            kindle_app_link("B01N5AX61W", Some("250")),
            "kindle://book?action=open&asin=B01N5AX61W&location=250"
        );
        assert_eq!(
            kindle_app_link("B01N5AX61W", None),
            "kindle://book?action=open&asin=B01N5AX61W"
        );
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("foo/bar:baz"), "foo_bar_baz");
        assert_eq!(sanitize_filename("normal-name"), "normal-name");
        assert_eq!(sanitize_filename("C# 12 in a Nutshell"), "CSharp 12 in a Nutshell");
        assert_eq!(sanitize_filename("test[1]^2|3"), "test_1__2_3");
    }

    #[test]
    fn test_book_filename() {
        let book = Book {
            id: "123".to_string(),
            title: "Atomic Habits: The life-changing bestseller".to_string(),
            author: "James Clear".to_string(),
            asin: Some("B01N5AX61W".to_string()),
            url: None,
            image_url: None,
            last_annotated_date: None,
        };
        assert_eq!(book_filename(&book, None), "Clear-Atomic Habits.md");
    }

    #[test]
    fn test_scrape_books_empty() {
        let html = "<html><body></body></html>";
        let region = &crate::config::REGIONS[0];
        let books = scrape_books(html, region).unwrap();
        assert!(books.is_empty());
    }

    #[test]
    fn test_scrape_books() {
        let html = r#"
        <html><body>
        <div class="kp-notebook-library-each-book" id="B01N5AX61W">
            <h2 class="kp-notebook-searchable">Atomic Habits</h2>
            <p class="kp-notebook-searchable">By: James Clear</p>
            <img class="kp-notebook-cover-image" src="https://example.com/cover.jpg" />
            <input id="kp-notebook-annotated-date-1" value="Sunday October 24, 2021" />
        </div>
        </body></html>
        "#;
        let region = &crate::config::REGIONS[0];
        let books = scrape_books(html, region).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].title, "Atomic Habits");
        assert_eq!(books[0].author, "James Clear");
        assert_eq!(books[0].asin.as_deref(), Some("B01N5AX61W"));
        assert_eq!(
            books[0].last_annotated_date,
            Some(NaiveDate::from_ymd_opt(2021, 10, 24).unwrap())
        );
    }

    #[test]
    fn test_parse_highlights() {
        let html = r#"
        <html><body>
        <div class="a-row a-spacing-base">
            <span id="highlight">This is a highlight.</span>
            <span class="kp-notebook-highlight kp-notebook-highlight-yellow"></span>
            <input id="kp-annotation-location" value="250" />
            <span id="annotationNoteHeader">Page 42</span>
            <span id="note"></span>
        </div>
        </body></html>
        "#;
        let document = Html::parse_document(html);
        let highlights = parse_highlights(&document).unwrap();
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].text, "This is a highlight.");
        assert_eq!(highlights[0].location.as_deref(), Some("250"));
        assert_eq!(highlights[0].page.as_deref(), Some("42"));
        assert_eq!(highlights[0].color.as_deref(), Some("yellow"));
    }

    #[test]
    fn test_parse_highlights_with_note() {
        let html = r#"
        <html><body>
        <div class="a-row a-spacing-base">
            <span id="highlight">Important text.</span>
            <span class="kp-notebook-highlight kp-notebook-highlight-blue"></span>
            <input id="kp-annotation-location" value="100" />
            <span id="annotationNoteHeader">Note - Location 100</span>
            <span id="note">My personal note</span>
        </div>
        </body></html>
        "#;
        let document = Html::parse_document(html);
        let highlights = parse_highlights(&document).unwrap();
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].note.as_deref(), Some("My personal note"));
        assert_eq!(highlights[0].color.as_deref(), Some("blue"));
    }
}
