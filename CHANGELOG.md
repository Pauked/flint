# Changelog

All notable changes to this project will be documented in this file.

## [0.3.0] - 2026-10-04

**Breaking:** frontmatter keys are now kebab-case and the nested format is gone. Existing notes are not recognised: move them aside and run `sync --all` to regenerate.

### Added
- `[highlight_colours]` config table: `style` (`obsidian` or `painter`), `text` and `label` switches
  - `obsidian` writes Obsidian 1.14+ native colour highlights (`==🟣text==`); `painter` writes `<mark class="hltr-*">`
  - Orange 🟠, green 🟢, blue and aqua 🔵, pink 🟣, red 🔴; yellow and unknown colours get a plain `==text==`
  - Text already containing `==` is left unwrapped in `obsidian` style
- `highlight_layout` config option: `quote` (blockquote, then `**Highlight** (colour) - location`) or `line` (the previous one-line layout)
- `{{block_ref}}` highlight template variable: a template that places it decides which line carries the `^ref-` ID; the quote layout puts it on the metadata line
- `{{coloured_text}}` and `{{colour_label}}` highlight template variables; `{{text}}`, `{{color}}` and `{{color_code}}` are unchanged

### Changed
- Default layout is now `quote`, with the label coloured in `obsidian` style and the passage left plain; the previous look is `highlight_layout = "line"` with `style = "painter"`, `text = false`
- Existing highlight lines are not re-rendered on sync; move a note aside and sync to regenerate it
- New highlights are inserted before the whole block of the next existing highlight (the line after the previous `---` or heading), not before its `^ref-` line, so they never split a quote from its metadata
- Template, layout and colour settings travel together as `RenderOptions`; `sync_book` drops from 7 arguments to 4
- Frontmatter keys renamed to kebab-case: `kindle-book-id`, `kindle-last-annotated-date`, `kindle-book-image-url`, `kindle-highlights-count`, `flint-last-sync-date`; the camelCase names are no longer read
- `parse_rendered_highlights` returns an error instead of panicking on a bad pattern

### Removed
- GitHub Actions release workflow and the stale v0.1.0 release; build from source
- Nested `kindle-sync:` frontmatter (from the original Obsidian Kindle plugin): no longer written or read
- `frontmatter_format` config option (only meaningful for the nested format)

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
