use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use headless_chrome::{Browser, LaunchOptions};
use log::{debug, info, warn};
use reqwest::blocking::Client;
use reqwest::cookie::Jar;
use serde::{Deserialize, Serialize};

use crate::config::AmazonRegion;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
}

fn cookies_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("cookies.json")
}

/// Load stored cookies from disk.
pub fn load_stored_cookies(data_dir: &Path) -> Result<Vec<StoredCookie>> {
    let path = cookies_path(data_dir);
    debug!("Loading cookies from {}", path.display());
    let json = fs::read_to_string(&path)
        .with_context(|| format!("No saved session at {}", path.display()))?;
    let cookies: Vec<StoredCookie> =
        serde_json::from_str(&json).context("Failed to parse stored cookies")?;
    debug!("Loaded {} cookies", cookies.len());
    Ok(cookies)
}

/// Save cookies to disk.
fn save_cookies(cookies: &[StoredCookie], data_dir: &Path) -> Result<()> {
    let path = cookies_path(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let json = serde_json::to_string(cookies).context("Failed to serialize cookies")?;
    fs::write(&path, &json)
        .with_context(|| format!("Failed to write cookies to {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).ok();
    }
    debug!("Saved {} cookies to {}", cookies.len(), path.display());
    Ok(())
}

/// Delete saved cookies from disk.
pub fn clear_cookies(data_dir: &Path) -> Result<()> {
    let path = cookies_path(data_dir);
    match fs::remove_file(&path) {
        Ok(()) => info!("Session cleared."),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!("No saved session to clear.");
        }
        Err(e) => return Err(e).context("Failed to remove cookies file"),
    }
    Ok(())
}

/// Build a reqwest client from stored cookies.
fn build_client_from_cookies(cookies: &[StoredCookie], region: &AmazonRegion) -> Result<Client> {
    let jar = Arc::new(Jar::default());
    let notebook_url: reqwest::Url = region
        .notebook_url
        .parse()
        .context("Invalid notebook URL")?;
    for cookie in cookies {
        let cookie_str = format!("{}={}; Domain={}", cookie.name, cookie.value, cookie.domain);
        jar.add_cookie_str(&cookie_str, &notebook_url);
    }
    Client::builder()
        .cookie_provider(jar)
        .timeout(Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .context("Failed to build HTTP client")
}

/// Validate that stored cookies represent a live Amazon session.
/// Makes an HTTP request to the notebook URL and checks for sign-in redirects.
pub fn validate_session(cookies: &[StoredCookie], region: &AmazonRegion) -> Result<()> {
    let client = build_client_from_cookies(cookies, region)?;

    debug!("Verifying saved session against {}", region.notebook_url);
    let response = client
        .get(region.notebook_url)
        .send()
        .context("Session verification request failed")?;

    let final_url = response.url().to_string();
    if final_url.contains("signin") || final_url.contains("/ap/") {
        debug!("Session expired (redirected to {})", final_url);
        bail!("Amazon session has expired. Run 'flint login' to re-authenticate.");
    }

    Ok(())
}

/// Try to build an authenticated client from saved cookies.
/// Returns None if no cookies are stored or they're invalid.
fn try_saved_cookies(region: &AmazonRegion, data_dir: &Path) -> Option<Client> {
    let cookies = match load_stored_cookies(data_dir) {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => {
            debug!("Cookie file is empty");
            return None;
        }
        Err(e) => {
            debug!("Could not load saved cookies: {e}");
            return None;
        }
    };

    if let Err(e) = validate_session(&cookies, region) {
        debug!("Session validation failed: {e}");
        return None;
    }

    match build_client_from_cookies(&cookies, region) {
        Ok(client) => {
            info!("Using saved session.");
            Some(client)
        }
        Err(e) => {
            debug!("Could not build client from saved cookies: {e}");
            None
        }
    }
}

/// Get an authenticated client. Tries saved cookies first, falls back to
/// Chrome login if they're missing or expired.
pub fn authenticate(region: &AmazonRegion, data_dir: &Path) -> Result<Client> {
    if let Some(client) = try_saved_cookies(region, data_dir) {
        return Ok(client);
    }

    login_via_chrome(region, data_dir)
}

/// Open Chrome for Amazon login, extract and save cookies, return client.
pub fn login_via_chrome(region: &AmazonRegion, data_dir: &Path) -> Result<Client> {
    info!("Opening Chrome for Amazon login...");
    info!("Please log in to your Amazon account in the browser window.");

    let launch_options = LaunchOptions {
        headless: false,
        window_size: Some((1200, 800)),
        idle_browser_timeout: Duration::from_secs(300),
        ..LaunchOptions::default()
    };

    let browser = Browser::new(launch_options).context(
        "Failed to launch Chrome. Is Chrome or Chromium installed?\n  \
         Install from: https://www.google.com/chrome/",
    )?;
    let tab = browser
        .new_tab()
        .context("Failed to open new tab in Chrome")?;

    debug!("Navigating to {}", region.notebook_url);
    tab.navigate_to(region.notebook_url)
        .context("Failed to navigate to Amazon notebook")?;

    // Wait for the user to log in by polling the URL
    let timeout = Duration::from_secs(300);
    let poll_interval = Duration::from_secs(2);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            bail!("Login timed out after 5 minutes. Run 'flint login' to try again.");
        }

        std::thread::sleep(poll_interval);

        let current_url = match tab.get_url() {
            url if !url.is_empty() => url,
            _ => continue,
        };

        // User is authenticated when they're on a page for this Amazon region
        if current_url.contains(region.hostname) && current_url.contains("notebook") {
            let has_books = tab
                .evaluate(
                    "document.querySelector('.kp-notebook-library-each-book') !== null",
                    false,
                )
                .ok()
                .and_then(|result| result.value)
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if has_books {
                info!("Login successful!");
                break;
            }
        }
    }

    // Extract cookies from Chrome
    let chrome_cookies = tab
        .get_cookies()
        .context("Failed to extract cookies from Chrome")?;
    debug!("Extracted {} cookies from Chrome", chrome_cookies.len());

    // Store cookies for reuse
    let stored: Vec<StoredCookie> = chrome_cookies
        .iter()
        .map(|c| StoredCookie {
            name: c.name.clone(),
            value: c.value.clone(),
            domain: c.domain.clone(),
        })
        .collect();

    if let Err(e) = save_cookies(&stored, data_dir) {
        warn!("Could not save session: {e}");
    } else {
        info!("Session saved.");
    }

    let client = build_client_from_cookies(&stored, region)?;
    Ok(client)
}
