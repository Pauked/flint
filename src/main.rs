mod auth;
mod config;
mod log_config;
mod models;
mod renderer;
mod scraper;
mod sync;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use log::{debug, info, warn};

use crate::models::BookHighlights;
use crate::scraper::Archive;

#[derive(Parser)]
#[command(name = "flint")]
#[command(about = "Sync Kindle highlights to Obsidian")]
#[command(version)]
struct Cli {
    /// Enable verbose logging (-v for debug, -vv for trace)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Sync highlights from Amazon Kindle to markdown files
    Sync {
        /// Output directory for markdown files
        #[arg(short, long)]
        output_dir: Option<PathBuf>,

        /// Amazon region (global, india, japan, spain, germany, italy, uk, france, netherlands)
        #[arg(short, long)]
        region: Option<String>,

        /// Sync all books, not just recently updated ones
        #[arg(long)]
        all: bool,

        /// Sync a specific book by ASIN
        #[arg(long)]
        book: Option<String>,

        /// Save raw Amazon HTML to this directory for offline use
        #[arg(long)]
        save_archive: Option<PathBuf>,

        /// Read from a saved archive instead of fetching from Amazon
        #[arg(long)]
        use_archive: Option<PathBuf>,
    },

    /// List books in your Kindle library
    List {
        /// Amazon region (global, india, japan, spain, germany, italy, uk, france, netherlands)
        #[arg(short, long)]
        region: Option<String>,

        /// Read from a saved archive instead of fetching from Amazon
        #[arg(long)]
        use_archive: Option<PathBuf>,
    },

    /// Log in to Amazon and save session
    Login {
        /// Amazon region (global, india, japan, spain, germany, italy, uk, france, netherlands)
        #[arg(short, long)]
        region: Option<String>,
    },

    /// Clear saved Amazon session
    Logout,

    /// Validate config and templates
    CheckConfig,

    /// Resync highlights for an existing markdown file
    Resync {
        /// Path to the markdown file to resync
        file: PathBuf,

        /// Amazon region (global, india, japan, spain, germany, italy, uk, france, netherlands)
        #[arg(short, long)]
        region: Option<String>,

        /// Save raw Amazon HTML to this directory for offline use
        #[arg(long)]
        save_archive: Option<PathBuf>,

        /// Read from a saved archive instead of fetching from Amazon
        #[arg(long)]
        use_archive: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    log_config::init(cli.verbose);

    if let Err(e) = run(cli) {
        eprintln!("\x1b[31mError: {e:?}\x1b[0m");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    if cli.verbose > 0 {
        let log_path = std::env::temp_dir().join("flint.log");
        debug!("Log file: {}", log_path.display());
    }

    let data_dir = config::resolve_data_dir().context("Failed to resolve data directory")?;
    debug!("Data directory: {}", data_dir.display());

    let config = config::Config::load(&data_dir).context("Failed to load config")?;
    debug!(
        "Config loaded: region={}, output_dir={}",
        config.region_name(),
        config.output_dir().display()
    );

    match cli.command {
        Commands::List {
            region,
            use_archive,
        } => cmd_list(
            &config,
            &data_dir,
            region.as_deref(),
            use_archive.as_deref(),
        ),
        cmd @ Commands::Sync { .. } => cmd_sync(&config, &data_dir, cmd),
        Commands::Login { region } => cmd_login(&config, &data_dir, region.as_deref()),
        Commands::Logout => cmd_logout(&data_dir),
        Commands::CheckConfig => cmd_check_config(&config),
        Commands::Resync {
            file,
            region,
            save_archive,
            use_archive,
        } => cmd_resync(
            &config,
            &data_dir,
            &file,
            region.as_deref(),
            save_archive.as_deref(),
            use_archive.as_deref(),
        ),
    }
}

fn cmd_login(
    config: &config::Config,
    data_dir: &std::path::Path,
    region_override: Option<&str>,
) -> Result<()> {
    let region_name = region_override.unwrap_or(config.region_name());
    let region = config::get_region(region_name)?;
    auth::login_via_chrome(region, data_dir)?;
    Ok(())
}

fn cmd_logout(data_dir: &std::path::Path) -> Result<()> {
    auth::clear_cookies(data_dir)?;
    Ok(())
}

fn cmd_check_config(config: &config::Config) -> Result<()> {
    let green = "\x1b[32m";
    let red = "\x1b[31m";
    let reset = "\x1b[0m";
    let mut errors = 0;

    // Display config
    println!("Region:             {}", config.region_name());
    println!("Output directory:   {}", config.output_dir().display());
    println!("Frontmatter format: {}", config.frontmatter_format());
    println!("Download metadata:  {}", config.download_metadata());
    let ignored = config.ignored_books();
    if !ignored.is_empty() {
        println!("Ignored books:      {}", ignored.join(", "));
    }
    println!();

    // Validate region
    match config::get_region(config.region_name()) {
        Ok(_) => println!(
            "{green}OK{reset}  Region '{}' is valid.",
            config.region_name()
        ),
        Err(e) => {
            println!("{red}ERR{reset} {e}");
            errors += 1;
        }
    }

    // Build Tera and validate templates
    let templates = config.templates.as_ref();
    let file_template = templates.and_then(|t| t.file_template.as_deref());
    let highlight_template = templates.and_then(|t| t.highlight_template.as_deref());
    let filename_template = templates.and_then(|t| t.filename_template.as_deref());

    let tera = match renderer::build_tera(file_template, highlight_template) {
        Ok(t) => {
            println!("{green}OK{reset}  Templates parse successfully.");
            Some(t)
        }
        Err(e) => {
            println!("{red}ERR{reset} Template syntax error: {e}");
            errors += 1;
            None
        }
    };

    // Test-render with dummy data
    if let Some(tera) = tera {
        let dummy_book = models::Book {
            id: "00000".to_string(),
            title: "Test Book: A Subtitle".to_string(),
            author: "Test Author".to_string(),
            asin: Some("B00TEST".to_string()),
            url: Some("https://www.amazon.com/dp/B00TEST".to_string()),
            image_url: Some("https://example.com/cover.jpg".to_string()),
            last_annotated_date: Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
        };
        let dummy_highlights = vec![models::Highlight {
            id: "00001".to_string(),
            text: Some("Test highlight text.".to_string()),
            location: Some("100".to_string()),
            page: Some("1".to_string()),
            note: Some("Test note.".to_string()),
            color: Some("yellow".to_string()),
        }];
        let dummy_metadata = models::BookMetadata {
            isbn: Some("1234567890".to_string()),
            pages: Some("200".to_string()),
            publication_date: Some("January 1, 2024".to_string()),
            publisher: Some("Test Publisher".to_string()),
            author_url: Some("https://example.com/author".to_string()),
        };
        let dummy_entry = BookHighlights {
            book: dummy_book,
            highlights: dummy_highlights,
            metadata: Some(dummy_metadata),
        };

        match renderer::render_file(&tera, "book.tera", "highlight.tera", &dummy_entry) {
            Ok(_) => println!("{green}OK{reset}  Test render succeeded."),
            Err(e) => {
                println!("{red}ERR{reset} Test render failed: {e}");
                errors += 1;
            }
        }
    }

    // Validate filename template
    if let Some(tmpl) = filename_template {
        let dummy_book = models::Book {
            id: "00000".to_string(),
            title: "Test Book".to_string(),
            author: "Test Author".to_string(),
            asin: None,
            url: None,
            image_url: None,
            last_annotated_date: Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
        };
        let result = scraper::book_filename(&dummy_book, None, Some(tmpl));
        println!("{green}OK{reset}  Filename template produces: {result}");
    } else {
        println!("{green}OK{reset}  Using default filename template.");
    }

    println!();
    if errors > 0 {
        println!("{red}{errors} error(s) found.{reset}");
        std::process::exit(1);
    } else {
        println!("{green}All checks passed.{reset}");
    }

    Ok(())
}

fn cmd_list(
    config: &config::Config,
    data_dir: &std::path::Path,
    region_override: Option<&str>,
    use_archive: Option<&std::path::Path>,
) -> Result<()> {
    let region_name = region_override.unwrap_or(config.region_name());
    let region = config::get_region(region_name)?;

    let use_arc = use_archive.map(Archive::new);

    let html = scraper::fetch_notebook_html(region, use_arc.as_ref(), None, data_dir)?;
    let books = scraper::scrape_books(&html, region)?;

    if books.is_empty() {
        info!("No books found in your Kindle library.");
        return Ok(());
    }

    println!(
        "{:<15} {:<50} {:<30} LAST ANNOTATED",
        "ASIN", "TITLE", "AUTHOR"
    );
    println!("{}", "-".repeat(110));

    for book in &books {
        let asin = book.asin.as_deref().unwrap_or("-");
        let title = if book.title.len() > 48 {
            format!("{}…", &book.title[..47])
        } else {
            book.title.clone()
        };
        let author = if book.author.len() > 28 {
            format!("{}…", &book.author[..27])
        } else {
            book.author.clone()
        };
        let date = book
            .last_annotated_date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "-".to_string());

        println!("{:<15} {:<50} {:<30} {}", asin, title, author, date);
    }

    info!("{} books found.", books.len());
    Ok(())
}

fn cmd_sync(config: &config::Config, data_dir: &std::path::Path, cmd: Commands) -> Result<()> {
    let Commands::Sync {
        output_dir: output_dir_override,
        region,
        all: sync_all,
        book,
        save_archive,
        use_archive,
    } = cmd
    else {
        unreachable!()
    };

    let region_name = region.as_deref().unwrap_or(config.region_name());
    let region = config::get_region(region_name)?;
    let output_dir = output_dir_override.unwrap_or_else(|| config.output_dir());
    let download_metadata = config.download_metadata();
    let frontmatter_format = config.frontmatter_format();

    let templates = config.templates.as_ref();
    let file_template = templates.and_then(|t| t.file_template.as_deref());
    let highlight_template = templates.and_then(|t| t.highlight_template.as_deref());
    let filename_template = templates.and_then(|t| t.filename_template.as_deref());

    let use_arc = use_archive.as_deref().map(Archive::new);
    let save_arc = save_archive.as_deref().map(Archive::new);

    // Fetch book list (uses headless Chrome for JS-rendered content)
    let html = scraper::fetch_notebook_html(region, use_arc.as_ref(), save_arc.as_ref(), data_dir)?;
    let all_books = scraper::scrape_books(&html, region)?;

    // Determine which books to sync
    let existing = sync::scan_existing_files(&output_dir)?;
    let state = config::SyncState::load(data_dir);

    let specific_asin = book.as_deref();
    let books_to_process = if let Some(asin) = specific_asin {
        all_books
            .into_iter()
            .filter(|b| b.asin.as_deref() == Some(asin))
            .collect::<Vec<_>>()
    } else if sync_all {
        all_books
    } else {
        sync::books_to_sync(&all_books, &existing, state.last_sync_date())
    };

    // Filter out ignored books (case-insensitive substring match)
    let ignored = config.ignored_books();
    let books_to_process: Vec<_> = if ignored.is_empty() {
        books_to_process
    } else {
        let before = books_to_process.len();
        let filtered: Vec<_> = books_to_process
            .into_iter()
            .filter(|b| {
                let title_lower = b.title.to_lowercase();
                !ignored
                    .iter()
                    .any(|ig| title_lower.contains(&ig.to_lowercase()))
            })
            .collect();
        let skipped = before - filtered.len();
        if skipped > 0 {
            info!("Skipped {} ignored book(s).", skipped);
        }
        filtered
    };

    if books_to_process.is_empty() {
        info!("All books are up to date.");
        return Ok(());
    }

    // Build HTTP client for highlight/metadata fetches
    let client = if use_arc.is_some() {
        reqwest::blocking::Client::new()
    } else {
        auth::authenticate(region, data_dir)?
    };

    let total = books_to_process.len();
    let sync_start_time = chrono::Local::now();
    info!(
        "Syncing {} book(s) at {}...",
        total,
        sync_start_time.format("%H:%M:%S")
    );
    let sync_start = std::time::Instant::now();

    let mut synced_count: usize = 0;
    let mut skipped_count: usize = 0;
    let mut total_highlights: usize = 0;

    for (i, book) in books_to_process.iter().enumerate() {
        let book_start = std::time::Instant::now();
        info!(
            "  [{}/{}] {} by {}...",
            i + 1,
            total,
            scraper::shorten_title(&book.title),
            book.author
        );

        let highlights = scraper::scrape_book_highlights(
            &client,
            region,
            book,
            use_arc.as_ref(),
            save_arc.as_ref(),
        )
        .with_context(|| format!("Failed to scrape highlights for {}", book.title))?;

        if highlights.is_empty() {
            info!("    no highlights, skipping.");
            skipped_count += 1;
            continue;
        }

        let metadata = if download_metadata {
            scraper::scrape_book_metadata(&client, book, use_arc.as_ref(), save_arc.as_ref()).ok()
        } else {
            None
        };

        let entry = BookHighlights {
            book: book.clone(),
            highlights,
            metadata,
        };

        let existing_file = existing.find(book);

        let path = sync::sync_book(
            &entry,
            &output_dir,
            existing_file,
            file_template,
            highlight_template,
            filename_template,
            frontmatter_format,
        )?;

        let hl_count = entry.highlights.len();
        let book_elapsed = book_start.elapsed();
        info!(
            "    {} highlights -> {} ({:.1}s)",
            hl_count,
            path.display(),
            book_elapsed.as_secs_f64()
        );
        synced_count += 1;
        total_highlights += hl_count;
    }

    // Record successful sync (skip for archive-only runs)
    if use_arc.is_none() {
        let mut state = config::SyncState::load(data_dir);
        state.record_sync();
        if let Err(e) = state.save(data_dir) {
            warn!("Could not save sync state: {e}");
        }
    }

    let total_elapsed = sync_start.elapsed();
    let sync_end_time = chrono::Local::now();
    info!(
        "Done at {}! Synced {} books ({} highlights) in {:.1}s. {} skipped (no highlights).",
        sync_end_time.format("%H:%M:%S"),
        synced_count,
        total_highlights,
        total_elapsed.as_secs_f64(),
        skipped_count
    );
    Ok(())
}

fn cmd_resync(
    config: &config::Config,
    data_dir: &std::path::Path,
    file_path: &std::path::Path,
    region_override: Option<&str>,
    save_archive: Option<&std::path::Path>,
    use_archive: Option<&std::path::Path>,
) -> Result<()> {
    let region_name = region_override.unwrap_or(config.region_name());
    let region = config::get_region(region_name)?;
    let frontmatter_format = config.frontmatter_format();

    let templates = config.templates.as_ref();
    let highlight_template = templates.and_then(|t| t.highlight_template.as_deref());

    let use_arc = use_archive.map(Archive::new);
    let save_arc = save_archive.map(Archive::new);

    // Read the file and extract frontmatter
    let existing = sync::load_existing_file(file_path)?;

    let asin = existing
        .frontmatter
        .asin
        .as_deref()
        .context("File has no ASIN in frontmatter — cannot resync without it")?;

    info!(
        "Resyncing \"{}\" (ASIN: {})...",
        existing.frontmatter.title, asin
    );

    let client = if use_arc.is_some() {
        reqwest::blocking::Client::new()
    } else {
        auth::authenticate(region, data_dir)?
    };

    let book = models::Book {
        id: existing.frontmatter.book_id.clone(),
        title: existing.frontmatter.title.clone(),
        author: existing.frontmatter.author.clone(),
        asin: Some(asin.to_string()),
        url: Some(format!("https://www.amazon.com/dp/{asin}")),
        image_url: existing.frontmatter.book_image_url.clone(),
        last_annotated_date: existing
            .frontmatter
            .last_annotated_date
            .as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()),
    };

    let highlights = scraper::scrape_book_highlights(
        &client,
        region,
        &book,
        use_arc.as_ref(),
        save_arc.as_ref(),
    )
    .context("Failed to scrape highlights")?;

    if highlights.is_empty() {
        info!("No highlights found.");
        return Ok(());
    }

    let metadata = if config.download_metadata() {
        scraper::scrape_book_metadata(&client, &book, use_arc.as_ref(), save_arc.as_ref()).ok()
    } else {
        None
    };

    let entry = BookHighlights {
        book,
        highlights,
        metadata,
    };

    let output_dir = existing.path.parent().unwrap();
    let path = sync::sync_book(
        &entry,
        output_dir,
        Some(&existing),
        None,
        highlight_template,
        None,
        frontmatter_format,
    )?;

    info!(
        "{} highlights -> {}",
        entry.highlights.len(),
        path.display()
    );
    Ok(())
}
