# Changelog

All notable changes to this project will be documented in this file.

## [0.2.6] - 2026-07-20

### Added
- `flint snippet install` / `remove` / `status` — manages the CSS snippet that colours highlights in an Obsidian vault
- Snippet is enabled and disabled in the vault's `appearance.json`, preserving all other settings
- Vault auto-detected from `output_dir`; `--vault` overrides it
- Locally edited snippets are protected — `install` and `remove` refuse to touch them without `--force`

### Changed
- Highlight colours no longer depend on the Highlightr plugin, which has been removed from the Obsidian community store
- Book sorting in email reports and highlight insertion ordering now use `sort_by_key`; clippy is warning-free

## [0.2.5] - 2026-03-07

### Added
- `flint-version` property in frontmatter records the version that last wrote the file
- Time component in `flint-lastSyncDate` (`YYYY-MM-DDTHH:MM`)

## [0.2.4] - 2026-03-07

### Added
- `flint-lastSyncDate` frontmatter property, updated on every sync (supported in flat and nested formats)
- Flint version footer in HTML and plain-text email output

### Changed
- Bumped `reqwest` to 0.13 and `scraper` to 0.25

## [0.2.2] - 2026-02-26

### Added
- Email notifications summarising each sync run (configurable via Resend)
- Per-run log files for easier debugging of scheduled syncs
- Daily sync scheduling support
- Early session-expiry detection so automated syncs fail fast instead of silently scraping a logged-out session
- Unit tests for email rendering

### Changed
- Skipped books are now hidden from email reports to reduce noise

### Fixed
- Cookie storage now preserves `path`, `secure`, and `httpOnly` attributes

## [0.2.0] - 2026-02-25

### Added
- Ignore list for books that should never be synced
- Multi-region Amazon support
- User-overridable Tera templates for book and highlight rendering
- `check-config` command to validate configuration
- Sync timing information in output
- File logging alongside console output

### Changed
- Improved error messages across the sync pipeline
- General code cleanup

## [0.1.0] - 2026-02-14

### Added
- Initial release as **Flint** (renamed from the original prototype)
- Rust CLI for syncing Kindle highlights to Markdown: `sync`, `list`, `login`, `logout`, `resync`
- Headless Chrome login flow with persisted cookies
- Amazon notebook scraping with metadata extraction and author-name parsing
- Markdown rendering via Tera templates with YAML frontmatter
- Diff/merge sync that preserves user edits between runs
- Filename sanitisation for Obsidian compatibility
- Binary-relative data directory for portable deployment
- GitHub Actions release workflow for macOS
- MIT license, security note on cookie storage, and attribution
