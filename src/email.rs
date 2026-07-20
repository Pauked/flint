use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use log::{debug, error, info, warn};
use serde_json::json;

use crate::config::EmailConfig;

const RESEND_API_URL: &str = "https://api.resend.com/emails";
const HTTP_REQUEST_TIMEOUT_SECS: u64 = 30;
const HTTP_CONNECT_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone)]
pub enum BookSyncStatus {
    Synced,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct BookSyncResult {
    pub title: String,
    pub author: String,
    pub highlight_count: usize,
    pub duration: Duration,
    pub status: BookSyncStatus,
}

#[derive(Debug)]
pub struct SyncMode {
    pub source: SyncSource,
    pub save_archive: bool,
    pub sync_all: bool,
    pub single_book: bool,
}

#[derive(Debug)]
pub enum SyncSource {
    Amazon,
    Archive,
}

impl std::fmt::Display for SyncMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let source = match self.source {
            SyncSource::Amazon => "Amazon",
            SyncSource::Archive => "Local archive",
        };
        let mut parts = vec![source.to_string()];
        if self.save_archive {
            parts.push("saving archive".to_string());
        }
        if self.sync_all {
            parts.push("all books".to_string());
        }
        if self.single_book {
            parts.push("single book".to_string());
        }
        write!(f, "{}", parts.join(" · "))
    }
}

#[derive(Debug)]
pub struct SyncReport {
    pub synced_count: usize,
    pub skipped_count: usize,
    pub total_highlights: usize,
    pub duration: Duration,
    pub start_time: DateTime<Local>,
    pub books: Vec<BookSyncResult>,
    pub error: Option<String>,
    pub log_file: Option<PathBuf>,
    pub mode: SyncMode,
    pub session_expired: bool,
}

impl SyncReport {
    fn is_error(&self) -> bool {
        self.error.is_some() && !self.session_expired
    }

    fn is_up_to_date(&self) -> bool {
        !self.is_error() && !self.session_expired && self.synced_count == 0
    }
}

pub fn send_sync_email(config: &EmailConfig, report: &SyncReport) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(HTTP_REQUEST_TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
        .build()
        .unwrap_or_else(|e| {
            warn!("Failed to create HTTP client with custom settings: {e}. Using defaults.");
            reqwest::blocking::Client::new()
        });

    let subject = generate_subject(report);
    let html_body = generate_html(report);
    let plain_text = generate_plain_text(report);

    let payload = json!({
        "from": format!("Flint <{}>", config.from),
        "to": [config.to],
        "subject": subject,
        "html": html_body,
        "text": plain_text
    });

    debug!("Email from: {}, to: {}", config.from, config.to);
    debug!("Email subject: {subject}");

    let start_time = std::time::Instant::now();
    info!("Sending email via Resend...");

    let response = client
        .post(RESEND_API_URL)
        .header("Authorization", format!("Bearer {}", config.resend_api_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .with_context(|| {
            let elapsed = start_time.elapsed();
            format!(
                "Email HTTP request failed after {:.1}s",
                elapsed.as_secs_f64()
            )
        })?;

    let elapsed = start_time.elapsed();

    if response.status().is_success() {
        info!("Email sent successfully ({:.1}s)", elapsed.as_secs_f64());
        Ok(())
    } else {
        let status = response.status();
        let error_text = response
            .text()
            .unwrap_or_else(|e| format!("Failed to read error response: {e}"));
        error!("Failed to send email: {status}: {error_text}");
        anyhow::bail!("Resend API error {status}: {error_text}")
    }
}

fn generate_subject(report: &SyncReport) -> String {
    let date = report.start_time.format("%Y-%m-%d");

    if report.session_expired {
        format!("⚠ Flint: Session Expired - {date}")
    } else if report.is_error() {
        format!("✗ Flint Sync Failed - {date}")
    } else if report.is_up_to_date() {
        format!("· Flint Sync: up to date - {date}")
    } else {
        format!(
            "✓ Flint Sync: {} books, {} highlights - {date}",
            report.synced_count, report.total_highlights
        )
    }
}

fn generate_html(report: &SyncReport) -> String {
    let (status_color, stats_grid_gradient, status_text) = if report.session_expired {
        (
            "#b8860b",
            "linear-gradient(135deg, #8b6508, #b8860b)",
            "SESSION EXPIRED ⚠",
        )
    } else if report.is_error() {
        (
            "#8b1538",
            "linear-gradient(135deg, #5c0f24, #8b1538)",
            "SYNC FAILED ✗",
        )
    } else if report.is_up_to_date() {
        (
            "#3d59ab",
            "linear-gradient(135deg, #2d4490, #3d59ab)",
            "UP TO DATE ·",
        )
    } else {
        (
            "#2d5a27",
            "linear-gradient(135deg, #1e3a1c, #2d5a27)",
            "SYNC COMPLETE ✓",
        )
    };

    let date = report.start_time.format("%Y-%m-%d %H:%M");
    let duration_str = format_duration(report.duration);
    let mode_str = report.mode.to_string();

    let log_file_section = report.log_file.as_ref().map_or(String::new(), |path| {
        format!(
            "<div style=\"margin-top: 8px; padding: 6px 8px; background-color: #1e1e1e; border-radius: 4px; font-size: 12px; color: #888;\">\
             📄 Log: {}</div>",
            html_escape(&path.display().to_string())
        )
    });

    // Build detailed summary
    let details = if report.session_expired {
        String::from(
            "<div style=\"color: #ffd700; padding: 8px; background-color: #3d3520; border-radius: 4px; margin: 8px 0;\">\
             <strong>Your Amazon session has expired.</strong><br>\
             Sync was skipped. Run <code style=\"background: #2a2a2a; padding: 2px 6px; border-radius: 3px;\">flint login</code> to re-authenticate.</div>",
        )
    } else if report.is_error() {
        let err = report.error.as_deref().unwrap_or("Unknown error");
        format!(
            "<div style=\"color: #ff6b6b; padding: 8px; background-color: #3d1f1f; border-radius: 4px; margin: 8px 0;\">\
             <strong>Error:</strong> {}</div>",
            html_escape(err)
        )
    } else if report.books.is_empty() {
        String::from("<p style=\"color: #a0a0a0;\">All books are up to date. Nothing to sync.</p>")
    } else {
        let synced_books: Vec<_> = report
            .books
            .iter()
            .filter(|b| matches!(b.status, BookSyncStatus::Synced))
            .collect();
        if synced_books.is_empty() {
            String::from(
                "<p style=\"color: #a0a0a0;\">All books are up to date. Nothing to sync.</p>",
            )
        } else {
            let mut sorted_books = synced_books;
            sorted_books.sort_by_key(|book| book.title.to_lowercase());
            let mut lines = String::new();
            for book in &sorted_books {
                let detail = format!(
                    "{} highlights, {:.1}s",
                    book.highlight_count,
                    book.duration.as_secs_f64()
                );
                lines.push_str(&format!(
                    "<tr>\
                 <td style=\"padding: 4px 8px; color: #e0e0e0;\">{}</td>\
                 <td style=\"padding: 4px 8px; color: #a0a0a0;\">{}</td>\
                 <td style=\"padding: 4px 8px; color: #a0a0a0; text-align: right;\">{detail}</td>\
                 </tr>",
                    html_escape(&book.title),
                    html_escape(&book.author),
                ));
            }
            format!(
                "<table style=\"width: 100%; border-collapse: collapse; font-size: 13px;\">\
             <tr style=\"border-bottom: 1px solid #444;\">\
             <th style=\"padding: 4px 8px; text-align: left; color: #888;\">Title</th>\
             <th style=\"padding: 4px 8px; text-align: left; color: #888;\">Author</th>\
             <th style=\"padding: 4px 8px; text-align: right; color: #888;\">Details</th>\
             </tr>\
             {lines}</table>"
            )
        }
    };

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Kindle Sync Report</title>
    <style>
        body {{ font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; margin: 0; padding: 10px; background-color: #1a1a1a; color: #e0e0e0; }}
        .container {{ max-width: 600px; margin: 0 auto; background-color: #2d2d2d; border-radius: 6px; box-shadow: 0 2px 8px rgba(0,0,0,0.3); }}
        .header {{ background-color: #4a2070; color: #ffffff; padding: 10px 16px; border-radius: 6px 6px 0 0; }}
        .header h1 {{ margin: 0; font-size: 1.4em; }}
        .header p {{ margin: 4px 0 0 0; font-size: 1em; opacity: 0.9; }}
        .status {{ background-color: {status_color}; color: white; padding: 8px 16px; margin: 0; text-align: center; font-weight: bold; font-size: 1.05em; }}
        .content {{ padding: 10px 16px 16px 16px; }}
        .stats-grid {{ background: {stats_grid_gradient}; border-radius: 4px; padding: 10px; margin: 4px 0 8px 0; }}
        .stats-row {{ display: table; width: 100%; table-layout: fixed; border-spacing: 6px 0; }}
        .stat-item {{ display: table-cell; width: 25%; text-align: center; vertical-align: middle; }}
        .stat-number {{ font-size: 1.15em; font-weight: bold; color: #ffffff; display: block; }}
        .stat-label {{ color: #d0d0d0; font-size: 0.85em; margin-top: 2px; }}
        .detailed-summary {{ background-color: #1e1e1e; padding: 8px; border-radius: 4px; margin: 8px 0; }}
        .detailed-summary h4 {{ margin: 0 0 6px 0; font-size: 1.05em; color: #ffffff; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>📚 Kindle Sync Report</h1>
            <p>{date} · {mode_str}</p>
        </div>

        <div class="status">
            {status_text}
        </div>

        <div class="content">
            <div class="stats-grid">
                <div class="stats-row">
                    <div class="stat-item">
                        <span class="stat-number">{synced}</span>
                        <div class="stat-label">Synced</div>
                    </div>
                    <div class="stat-item">
                        <span class="stat-number">{highlights}</span>
                        <div class="stat-label">Highlights</div>
                    </div>
                    <div class="stat-item">
                        <span class="stat-number">{skipped}</span>
                        <div class="stat-label">Skipped</div>
                    </div>
                    <div class="stat-item">
                        <span class="stat-number">{duration}</span>
                        <div class="stat-label">Duration</div>
                    </div>
                </div>
            </div>

            <div class="detailed-summary">
                <h4>📋 Details</h4>
                {details}
            </div>

            {log_file_section}

            <div style="text-align: right; font-size: 0.75em; color: #666; margin-top: 8px;">flint v{version}</div>
        </div>
    </div>
</body>
</html>"#,
        status_color = status_color,
        status_text = status_text,
        stats_grid_gradient = stats_grid_gradient,
        date = date,
        mode_str = mode_str,
        synced = report.synced_count,
        highlights = report.total_highlights,
        skipped = report.skipped_count,
        duration = duration_str,
        details = details,
        log_file_section = log_file_section,
        version = env!("CARGO_PKG_VERSION"),
    )
}

fn generate_plain_text(report: &SyncReport) -> String {
    let date = report.start_time.format("%Y-%m-%d %H:%M");
    let duration_str = format_duration(report.duration);

    let mode_str = report.mode.to_string();
    let mut text =
        format!("KINDLE SYNC REPORT\n==================\nDate: {date}\nMode: {mode_str}\n\n");

    if report.session_expired {
        text.push_str("Status: SESSION EXPIRED\n\n");
        text.push_str("Your Amazon session has expired.\n");
        text.push_str("Sync was skipped. Run 'flint login' to re-authenticate.\n");
        if let Some(log_file) = &report.log_file {
            text.push_str(&format!("\nLog: {}\n", log_file.display()));
        }
        text.push_str(&format!("\nflint v{}\n", env!("CARGO_PKG_VERSION")));
        return text;
    }

    if let Some(err) = &report.error {
        text.push_str(&format!("Status: FAILED\nError: {err}\n"));
        if let Some(log_file) = &report.log_file {
            text.push_str(&format!("\nLog: {}\n", log_file.display()));
        }
        text.push_str(&format!("\nflint v{}\n", env!("CARGO_PKG_VERSION")));
        return text;
    }

    text.push_str(&format!(
        "Synced: {} books, {} highlights\nSkipped: {} (no highlights)\nDuration: {}\n",
        report.synced_count, report.total_highlights, report.skipped_count, duration_str
    ));

    let synced_books: Vec<_> = report
        .books
        .iter()
        .filter(|b| matches!(b.status, BookSyncStatus::Synced))
        .collect();
    if !synced_books.is_empty() {
        let mut sorted_books = synced_books;
        sorted_books.sort_by_key(|book| book.title.to_lowercase());
        text.push_str("\nBooks:\n");
        for book in &sorted_books {
            text.push_str(&format!(
                "  {} by {} - {} highlights, {:.1}s\n",
                book.title,
                book.author,
                book.highlight_count,
                book.duration.as_secs_f64()
            ));
        }
    }

    if let Some(log_file) = &report.log_file {
        text.push_str(&format!("\nLog: {}\n", log_file.display()));
    }

    text.push_str(&format!("\nflint v{}\n", env!("CARGO_PKG_VERSION")));

    text
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_time() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 3, 15, 10, 30, 0).unwrap()
    }

    fn make_mode() -> SyncMode {
        SyncMode {
            source: SyncSource::Amazon,
            save_archive: false,
            sync_all: false,
            single_book: false,
        }
    }

    fn make_success_report() -> SyncReport {
        SyncReport {
            synced_count: 2,
            skipped_count: 1,
            total_highlights: 45,
            duration: Duration::from_secs(12),
            start_time: make_time(),
            books: vec![
                BookSyncResult {
                    title: "Atomic Habits".to_string(),
                    author: "James Clear".to_string(),
                    highlight_count: 30,
                    duration: Duration::from_secs(5),
                    status: BookSyncStatus::Synced,
                },
                BookSyncResult {
                    title: "Meditations".to_string(),
                    author: "Marcus Aurelius".to_string(),
                    highlight_count: 15,
                    duration: Duration::from_secs(3),
                    status: BookSyncStatus::Synced,
                },
                BookSyncResult {
                    title: "Empty Book".to_string(),
                    author: "No One".to_string(),
                    highlight_count: 0,
                    duration: Duration::from_secs(1),
                    status: BookSyncStatus::Skipped,
                },
            ],
            error: None,
            log_file: None,
            mode: make_mode(),
            session_expired: false,
        }
    }

    fn make_up_to_date_report() -> SyncReport {
        SyncReport {
            synced_count: 0,
            skipped_count: 0,
            total_highlights: 0,
            duration: Duration::from_secs(3),
            start_time: make_time(),
            books: vec![],
            error: None,
            log_file: None,
            mode: make_mode(),
            session_expired: false,
        }
    }

    fn make_error_report() -> SyncReport {
        SyncReport {
            synced_count: 0,
            skipped_count: 0,
            total_highlights: 0,
            duration: Duration::from_secs(1),
            start_time: make_time(),
            books: vec![],
            error: Some("Network timeout".to_string()),
            log_file: Some(PathBuf::from("/tmp/flint.log")),
            mode: make_mode(),
            session_expired: false,
        }
    }

    fn make_session_expired_report() -> SyncReport {
        SyncReport {
            synced_count: 0,
            skipped_count: 0,
            total_highlights: 0,
            duration: Duration::from_secs(0),
            start_time: make_time(),
            books: vec![],
            error: Some("Amazon session has expired".to_string()),
            log_file: None,
            mode: make_mode(),
            session_expired: true,
        }
    }

    // --- SyncReport state ---

    #[test]
    fn is_error_with_error() {
        let report = make_error_report();
        assert!(report.is_error());
    }

    #[test]
    fn is_error_false_when_session_expired() {
        let report = make_session_expired_report();
        assert!(!report.is_error());
    }

    #[test]
    fn is_error_false_on_success() {
        let report = make_success_report();
        assert!(!report.is_error());
    }

    #[test]
    fn is_up_to_date_with_no_syncs() {
        let report = make_up_to_date_report();
        assert!(report.is_up_to_date());
    }

    #[test]
    fn is_up_to_date_false_when_synced() {
        let report = make_success_report();
        assert!(!report.is_up_to_date());
    }

    #[test]
    fn is_up_to_date_false_when_session_expired() {
        let report = make_session_expired_report();
        assert!(!report.is_up_to_date());
    }

    // --- SyncMode display ---

    #[test]
    fn sync_mode_amazon_only() {
        let mode = make_mode();
        assert_eq!(mode.to_string(), "Amazon");
    }

    #[test]
    fn sync_mode_archive_with_flags() {
        let mode = SyncMode {
            source: SyncSource::Archive,
            save_archive: true,
            sync_all: true,
            single_book: false,
        };
        assert_eq!(
            mode.to_string(),
            "Local archive · saving archive · all books"
        );
    }

    #[test]
    fn sync_mode_single_book() {
        let mode = SyncMode {
            source: SyncSource::Amazon,
            save_archive: false,
            sync_all: false,
            single_book: true,
        };
        assert_eq!(mode.to_string(), "Amazon · single book");
    }

    // --- format_duration ---

    #[test]
    fn format_duration_seconds_only() {
        assert_eq!(format_duration(Duration::from_secs(42)), "42s");
    }

    #[test]
    fn format_duration_zero() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0s");
    }

    #[test]
    fn format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(Duration::from_secs(125)), "2m 5s");
    }

    #[test]
    fn format_duration_exact_minute() {
        assert_eq!(format_duration(Duration::from_secs(60)), "1m 0s");
    }

    // --- html_escape ---

    #[test]
    fn html_escape_special_chars() {
        assert_eq!(
            html_escape("<script>alert('xss' & \"more\")</script>"),
            "&lt;script&gt;alert(&#x27;xss&#x27; &amp; &quot;more&quot;)&lt;/script&gt;"
        );
    }

    #[test]
    fn html_escape_plain_text() {
        assert_eq!(html_escape("hello world"), "hello world");
    }

    // --- generate_subject ---

    #[test]
    fn subject_success() {
        let report = make_success_report();
        assert_eq!(
            generate_subject(&report),
            "✓ Flint Sync: 2 books, 45 highlights - 2026-03-15"
        );
    }

    #[test]
    fn subject_up_to_date() {
        let report = make_up_to_date_report();
        assert_eq!(
            generate_subject(&report),
            "· Flint Sync: up to date - 2026-03-15"
        );
    }

    #[test]
    fn subject_error() {
        let report = make_error_report();
        assert_eq!(
            generate_subject(&report),
            "✗ Flint Sync Failed - 2026-03-15"
        );
    }

    #[test]
    fn subject_session_expired() {
        let report = make_session_expired_report();
        assert_eq!(
            generate_subject(&report),
            "⚠ Flint: Session Expired - 2026-03-15"
        );
    }

    // --- generate_html ---

    #[test]
    fn html_success_contains_status_and_books() {
        let html = generate_html(&make_success_report());
        assert!(html.contains("SYNC COMPLETE ✓"));
        assert!(html.contains("#2d5a27"));
        assert!(html.contains("Atomic Habits"));
        assert!(html.contains("Meditations"));
    }

    #[test]
    fn html_success_excludes_skipped_books() {
        let html = generate_html(&make_success_report());
        assert!(!html.contains("Empty Book"));
        assert!(!html.contains("No One"));
    }

    #[test]
    fn html_error_contains_error_message() {
        let html = generate_html(&make_error_report());
        assert!(html.contains("SYNC FAILED ✗"));
        assert!(html.contains("#8b1538"));
        assert!(html.contains("Network timeout"));
    }

    #[test]
    fn html_up_to_date() {
        let html = generate_html(&make_up_to_date_report());
        assert!(html.contains("UP TO DATE ·"));
        assert!(html.contains("#3d59ab"));
        assert!(html.contains("All books are up to date"));
    }

    #[test]
    fn html_session_expired() {
        let html = generate_html(&make_session_expired_report());
        assert!(html.contains("SESSION EXPIRED ⚠"));
        assert!(html.contains("#b8860b"));
        assert!(html.contains("flint login"));
    }

    #[test]
    fn html_includes_log_file() {
        let html = generate_html(&make_error_report());
        assert!(html.contains("/tmp/flint.log"));
    }

    #[test]
    fn html_only_skipped_books_shows_up_to_date() {
        let report = SyncReport {
            synced_count: 0,
            skipped_count: 2,
            total_highlights: 0,
            duration: Duration::from_secs(5),
            start_time: make_time(),
            books: vec![
                BookSyncResult {
                    title: "Book A".to_string(),
                    author: "Author A".to_string(),
                    highlight_count: 0,
                    duration: Duration::from_secs(1),
                    status: BookSyncStatus::Skipped,
                },
                BookSyncResult {
                    title: "Book B".to_string(),
                    author: "Author B".to_string(),
                    highlight_count: 0,
                    duration: Duration::from_secs(1),
                    status: BookSyncStatus::Skipped,
                },
            ],
            error: None,
            log_file: None,
            mode: make_mode(),
            session_expired: false,
        };
        let html = generate_html(&report);
        assert!(html.contains("All books are up to date"));
        assert!(!html.contains("Book A"));
    }

    // --- generate_plain_text ---

    #[test]
    fn plain_text_success_lists_synced_books() {
        let text = generate_plain_text(&make_success_report());
        assert!(text.contains("Synced: 2 books, 45 highlights"));
        assert!(text.contains("Atomic Habits by James Clear"));
        assert!(text.contains("Meditations by Marcus Aurelius"));
    }

    #[test]
    fn plain_text_success_excludes_skipped_books() {
        let text = generate_plain_text(&make_success_report());
        assert!(!text.contains("Empty Book"));
    }

    #[test]
    fn plain_text_error() {
        let text = generate_plain_text(&make_error_report());
        assert!(text.contains("Status: FAILED"));
        assert!(text.contains("Network timeout"));
        assert!(text.contains("Log: /tmp/flint.log"));
    }

    #[test]
    fn plain_text_session_expired() {
        let text = generate_plain_text(&make_session_expired_report());
        assert!(text.contains("Status: SESSION EXPIRED"));
        assert!(text.contains("flint login"));
    }

    #[test]
    fn plain_text_up_to_date_no_books_section() {
        let text = generate_plain_text(&make_up_to_date_report());
        assert!(text.contains("Synced: 0 books"));
        assert!(!text.contains("Books:"));
    }
}
