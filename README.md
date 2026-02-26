# Flint

Sync your Kindle highlights to Obsidian.

Flint scrapes your highlights from Amazon's Kindle notebook page and writes them as Markdown files with Obsidian-compatible frontmatter and block references.

This Rust based CLI app is based on the [obsidian-kindle-plugin](https://github.com/hadynz/obsidian-kindle-plugin) by Hady Osman, but extends it by adding a local archive of highlights HTML from Amazon and some small improvements. Such as handling characters not legal in Obisidan file names.

## Install

Download the latest binary from [Releases](https://github.com/Pauked/flint/releases), or build from source:

```
cargo install --path .
```

Requires Chrome/Chromium installed (used for Amazon login and fetching the book list).

## Usage

### Authentication

```
flint login           # opens Chrome, saves session
flint logout          # clears saved session
```

All commands reuse the saved session automatically and only open Chrome if the session is missing or expired.

### List your books

```
flint list
```

### Sync highlights

```
flint sync
```

Syncs all new/updated highlights to markdown files. Only books with new annotations since the last sync are processed.

```
flint sync --all                # sync every book
flint sync --book B01N5AX61W    # sync one book by ASIN
flint sync --output-dir ./out   # custom output directory
flint sync --region uk          # use Amazon UK
```

**Regions:** `global` (default), `india`, `japan`, `spain`, `germany`, `italy`, `uk`, `france`, `netherlands`, `canada`

### Archive mode

Save raw Amazon HTML locally for offline use or debugging:

```
flint sync --save-archive ./archive --all
flint sync --use-archive ./archive --output-dir ./test-output --all
```

### Validate config

```
flint check-config
```

Validates region, parses templates, test-renders with dummy data, and checks the filename template. Prints OK/ERR status for each check.

### Resync a single file

```
flint resync path/to/Book.md
```

Reads the ASIN from the file's frontmatter, re-scrapes highlights from Amazon, and updates the file in place. Only new highlights are added — existing content is preserved.

### Incremental sync

Re-running `sync` on existing files is safe. New highlights are inserted in the correct position. Existing highlights (and any edits you've made to them) are preserved. The `^ref-{id}` block references on each highlight line enable this diffing. Books are matched by `bookId` with fallback to ASIN for resilience against Amazon title changes.

### Logging

```
flint -v sync         # debug output to console + log file
flint -vv sync        # trace output to console + log file
```

Verbose mode writes a rolling log file to your system temp directory (`$TMPDIR/flint.log`, 3MB, 3 rotations). The log path is printed at startup. Without `-v`, output goes to the console only and nothing is written to disk.

## Data directory

Flint stores its config, session cookies, and sync state in a single data directory. It checks two locations in order:

1. **Binary-relative** — if `config.toml` exists next to the `flint` executable, that directory is used for all data files. This makes self-contained deployments possible (e.g. in Dropbox with shell scripts).
2. **Fallback** — `~/.config/flint/`

Run with `-v` to see which data directory is active.

## Config

Optional. Create `config.toml` in the data directory:

```toml
output_dir = "~/Obsidian/Zettelkasten/300 Book Highlights"
region = "global"
download_metadata = true
frontmatter_format = "flat"  # or "nested" for legacy kindle-sync: format
ignored_books = ["Sample Book", "Free Preview"]

[templates]
# Override default templates (Tera syntax)
# file_template = "..."
# highlight_template = "..."
filename_template = "{{authors_last_names}}-{{title}}"
```

### Filename template variables

- `{{title}}` — shortened title (strips parentheticals and subtitles)
- `{{authors_last_names}}` — "Clear", "Blandy-Orendorff", or "Smith_et_al"
- `{{lastAnnotatedDate}}` — date in `YYYY-MM-DD` format
- `{{firstAuthorFirstName}}`, `{{firstAuthorLastName}}` — first author's parsed names
- `{{secondAuthorFirstName}}`, `{{secondAuthorLastName}}` — second author's parsed names
- `{{publicationDate}}` — raw publication date string from Amazon metadata

### Template filters

- `{{ publication_date | dateformat(format="%B %Y") }}` — format date strings; parses `January 1, 2020`, `2024-03-15`, and `2024` formats. Falls back to the original string if parsing fails.

## Output format

Files are written with Obsidian-compatible YAML frontmatter (`kindle-*` properties) and block references. Highlight colors are rendered using [Highlightr](https://github.com/chetachiezikeuzor/Highlightr-Plugin) syntax. Filenames are sanitized to avoid characters that break Obsidian links (`# ^ [ ] |`). Both the flat format and legacy nested `kindle-sync:` format are supported for reading existing files.

```markdown
---
kindle-bookId: '49849'
kindle-title: 'Atomic Habits'
kindle-author: James Clear
kindle-asin: B01N5AX61W
kindle-lastAnnotatedDate: '2024-08-27'
kindle-bookImageUrl: 'https://...'
kindle-highlightsCount: 97
---
# Atomic Habits
## Metadata
* Author: [[James Clear]]
* ASIN: B01N5AX61W
* Reference: https://www.amazon.com/dp/B01N5AX61W
* [Kindle link](kindle://book?action=open&asin=B01N5AX61W)

## Highlights
Habits are the compound interest of self-improvement. — <mark class="hltr-y">yellow</mark> | location: [259](kindle://book?action=open&asin=B01N5AX61W&location=259) ^ref-5675

---
```

## Design notes

- **Robust highlight hashing** — FNV-1a (32-bit) for collision-resistant highlight IDs
- **Graceful metadata fallback** — missing Amazon product page data doesn't break sync
- **Author URL safety** — only links when a valid URL is found on the product page
- **Unicode-safe** — handles multi-byte characters and strips Amazon's invisible control characters from metadata

## Security

Flint stores your Amazon session cookies as plaintext JSON (`cookies.json`) in the data directory, with file permissions restricted to your user (chmod 600). The cookies are standard Amazon session cookies — their lifetime is controlled by Amazon and they typically expire after a few weeks. Flint verifies cookies on each run and prompts for re-login when they've expired.

Run `flint logout` to clear saved cookies when you don't need them. If you use a portable deployment in a cloud-synced folder (e.g. Dropbox), be aware that your session cookies will sync too.

## Acknowledgements

Based on [obsidian-kindle-plugin](https://github.com/hadynz/obsidian-kindle-plugin) by Hady Osman, licensed under MIT. Thank you Hady for the plugin, many books were happily synced!

## License

MIT
