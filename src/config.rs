use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::debug;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct AmazonRegion {
    pub name: &'static str,
    pub hostname: &'static str,
    pub notebook_url: &'static str,
}

pub const REGIONS: &[AmazonRegion] = &[
    AmazonRegion {
        name: "global",
        hostname: "amazon.com",
        notebook_url: "https://read.amazon.com/notebook",
    },
    AmazonRegion {
        name: "india",
        hostname: "amazon.in",
        notebook_url: "https://read.amazon.in/notebook",
    },
    AmazonRegion {
        name: "japan",
        hostname: "amazon.co.jp",
        notebook_url: "https://read.amazon.co.jp/notebook",
    },
    AmazonRegion {
        name: "spain",
        hostname: "amazon.es",
        notebook_url: "https://leer.amazon.es/notebook",
    },
    AmazonRegion {
        name: "germany",
        hostname: "amazon.de",
        notebook_url: "https://lesen.amazon.de/notebook",
    },
    AmazonRegion {
        name: "italy",
        hostname: "amazon.it",
        notebook_url: "https://leggi.amazon.it/notebook",
    },
    AmazonRegion {
        name: "uk",
        hostname: "amazon.co.uk",
        notebook_url: "https://read.amazon.co.uk/notebook",
    },
    AmazonRegion {
        name: "france",
        hostname: "amazon.fr",
        notebook_url: "https://lire.amazon.fr/notebook",
    },
    AmazonRegion {
        name: "netherlands",
        hostname: "amazon.nl",
        notebook_url: "https://lezen.amazon.nl/notebook",
    },
    AmazonRegion {
        name: "canada",
        hostname: "amazon.ca",
        notebook_url: "https://read.amazon.ca/notebook",
    },
];

pub fn get_region(name: &str) -> Result<&'static AmazonRegion> {
    REGIONS
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(name))
        .context(format!(
            "Unknown region '{}'. Valid regions: {}",
            name,
            REGIONS
                .iter()
                .map(|r| r.name)
                .collect::<Vec<_>>()
                .join(", ")
        ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    pub to: String,
    pub from: String,
    pub resend_api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplatesConfig {
    pub file_template: Option<String>,
    pub highlight_template: Option<String>,
    pub filename_template: Option<String>,
}

/// Markup used for a coloured span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColourStyle {
    /// Obsidian 1.14+ native colour highlight: `==🟣text==`.
    #[default]
    Obsidian,
    /// Painter/Highlightr class: `<mark class="hltr-p">text</mark>`.
    Painter,
}

/// How each highlight is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HighlightLayout {
    /// Blockquote, then a `**Highlight** (colour) - location` line.
    #[default]
    Quote,
    /// One line: `text — colour | location`.
    Line,
}

/// Where highlight colours are shown, and in which markup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HighlightColours {
    pub style: ColourStyle,
    /// Colour the highlighted passage.
    pub text: bool,
    /// Colour the colour-name label.
    pub label: bool,
}

impl Default for HighlightColours {
    fn default() -> Self {
        Self {
            style: ColourStyle::Obsidian,
            text: false,
            label: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub output_dir: Option<String>,
    pub region: Option<String>,
    pub download_metadata: Option<bool>,
    pub templates: Option<TemplatesConfig>,
    pub ignored_books: Option<Vec<String>>,
    pub email: Option<EmailConfig>,
    pub log_dir: Option<String>,
    pub keep_logs_days: Option<u32>,
    pub highlight_layout: Option<HighlightLayout>,
    pub highlight_colours: Option<HighlightColours>,
}

/// Resolve the data directory for config, cookies, and state files.
///
/// Checks (in order):
/// 1. The directory containing the running binary — if `config.toml` exists there
/// 2. Fallback: `~/.config/flint/`
pub fn resolve_data_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("Failed to determine executable path")?;
    let exe_dir = exe.parent().context("Executable has no parent directory")?;

    if exe_dir.join("config.toml").exists() {
        debug!("Using binary-relative data dir: {}", exe_dir.display());
        return Ok(exe_dir.to_path_buf());
    }

    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    let fallback = PathBuf::from(home).join(".config/flint");
    debug!("Using default data dir: {}", fallback.display());
    Ok(fallback)
}

impl Config {
    pub fn load(data_dir: &Path) -> Result<Self> {
        let config_path = data_dir.join("config.toml");

        if config_path.exists() {
            let contents = fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config: {}", config_path.display()))?;
            let config: Config =
                toml::from_str(&contents).context("Failed to parse config file")?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    pub fn output_dir(&self) -> PathBuf {
        match &self.output_dir {
            Some(dir) => {
                let expanded = shellexpand::tilde(dir);
                PathBuf::from(expanded.as_ref())
            }
            None => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                PathBuf::from(home).join("kindle-highlights")
            }
        }
    }

    pub fn region_name(&self) -> &str {
        self.region.as_deref().unwrap_or("global")
    }

    pub fn download_metadata(&self) -> bool {
        self.download_metadata.unwrap_or(true)
    }

    pub fn ignored_books(&self) -> &[String] {
        self.ignored_books.as_deref().unwrap_or(&[])
    }

    pub fn log_dir(&self, data_dir: &Path) -> PathBuf {
        match &self.log_dir {
            Some(dir) => {
                let expanded = shellexpand::tilde(dir);
                PathBuf::from(expanded.as_ref())
            }
            None => data_dir.join("logs"),
        }
    }

    pub fn keep_logs_days(&self) -> u32 {
        self.keep_logs_days.unwrap_or(30)
    }

    pub fn highlight_layout(&self) -> HighlightLayout {
        self.highlight_layout.unwrap_or_default()
    }

    pub fn highlight_colours(&self) -> HighlightColours {
        self.highlight_colours.unwrap_or_default()
    }
}

// ── Sync state persistence ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    pub last_sync_date: Option<String>,
}

impl SyncState {
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("state.json");
        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        serde_json::from_str(&contents).unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        let path = data_dir.join("state.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let json = serde_json::to_string_pretty(self).context("Failed to serialize sync state")?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write state file: {}", path.display()))?;
        Ok(())
    }

    pub fn last_sync_date(&self) -> Option<chrono::NaiveDate> {
        self.last_sync_date
            .as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    }

    pub fn record_sync(&mut self) {
        self.last_sync_date = Some(chrono::Utc::now().format("%Y-%m-%d").to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_region_canada() {
        let region = get_region("canada").unwrap();
        assert_eq!(region.hostname, "amazon.ca");
        assert_eq!(region.notebook_url, "https://read.amazon.ca/notebook");
    }

    #[test]
    fn test_get_region_case_insensitive() {
        assert!(get_region("Canada").is_ok());
        assert!(get_region("GLOBAL").is_ok());
    }

    #[test]
    fn test_get_region_netherlands() {
        let region = get_region("netherlands").unwrap();
        assert_eq!(region.hostname, "amazon.nl");
        assert_eq!(region.notebook_url, "https://lezen.amazon.nl/notebook");
    }

    #[test]
    fn test_get_region_unknown() {
        assert!(get_region("narnia").is_err());
    }

    #[test]
    fn test_ignored_books_default_empty() {
        let config = Config::default();
        assert!(config.ignored_books().is_empty());
    }

    #[test]
    fn test_ignored_books_returns_list() {
        let config = Config {
            ignored_books: Some(vec!["Sample".to_string(), "Preview".to_string()]),
            ..Default::default()
        };
        assert_eq!(config.ignored_books(), &["Sample", "Preview"]);
    }

    #[test]
    fn highlight_colours_default_to_obsidian_label_only() {
        assert_eq!(
            Config::default().highlight_colours(),
            HighlightColours {
                style: ColourStyle::Obsidian,
                text: false,
                label: true,
            }
        );
    }

    #[test]
    fn reads_highlight_colours_table() -> Result<()> {
        let config: Config =
            toml::from_str("[highlight_colours]\nstyle = \"painter\"\ntext = false\n")?;
        assert_eq!(
            config.highlight_colours(),
            HighlightColours {
                style: ColourStyle::Painter,
                text: false,
                label: true,
            }
        );
        Ok(())
    }

    #[test]
    fn rejects_unknown_colour_style() {
        let parsed: std::result::Result<Config, _> =
            toml::from_str("[highlight_colours]\nstyle = \"neon\"\n");
        assert!(parsed.is_err());
    }

    #[test]
    fn highlight_layout_defaults_to_quote() {
        assert_eq!(Config::default().highlight_layout(), HighlightLayout::Quote);
    }

    #[test]
    fn reads_line_layout() -> Result<()> {
        let config: Config = toml::from_str("highlight_layout = \"line\"\n")?;
        assert_eq!(config.highlight_layout(), HighlightLayout::Line);
        Ok(())
    }
}
