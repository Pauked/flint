# Flint

Sync your Kindle highlights to Obsidian.

Flint scrapes your highlights from Amazon's Kindle notebook page and writes them as Markdown files with Obsidian-compatible frontmatter and block references.

This Rust based CLI app is based on the [obsidian-kindle-plugin](https://github.com/hadynz/obsidian-kindle-plugin) by Hady Osman, but extends it by adding a local archive of highlights HTML from Amazon and some small improvements. Such as handling characters not legal in Obisidan file names.

## Install

Download the latest binary from [Releases](https://github.com/paulhealey/flint/releases), or build from source:

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

**Regions:** `global` (default), `india`, `japan`, `spain`, `germany`, `italy`, `uk`, `france`

### Archive mode

Save raw Amazon HTML locally for offline use or debugging:

```
flint sync --save-archive ./archive --all
flint sync --use-archive ./archive --output-dir ./test-output --all
```

### Resync a single file

```
flint resync path/to/Book.md
```

Reads the ASIN from the file's frontmatter, re-scrapes highlights from Amazon, and updates the file in place. Only new highlights are added — existing content is preserved.

### Incremental sync

Re-running `sync` on existing files is safe. New highlights are inserted in the correct position. Existing highlights (and any edits you've made to them) are preserved. The `^ref-{id}` block references on each highlight line enable this diffing. Books are matched by `bookId` with fallback to ASIN for resilience against Amazon title changes.

### Verbose logging

```
flint -v sync         # debug logging
flint -vv sync        # trace logging
```

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

[templates]
# Override default templates (Tera syntax)
# file_template = "..."
# highlight_template = "..."
filename_template = "{{authors_last_names}}-{{title}}"
```

## Output format

Files are written with `kindle-sync` YAML frontmatter and Obsidian-compatible block references. Highlight colors are rendered using [Highlightr](https://github.com/chetachiezikeuzor/Highlightr-Plugin) syntax. Filenames are sanitized to avoid characters that break Obsidian links (`# ^ [ ] |`).

```markdown
---
kindle-sync:
  bookId: '49849'
  title: 'Atomic Habits'
  author: James Clear
  asin: B01N5AX61W
  lastAnnotatedDate: '2024-08-27'
  bookImageUrl: 'https://...'
  highlightsCount: 97
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
