# Flint

Sync your Kindle highlights to Obsidian.

Flint scrapes your highlights from Amazon's Kindle notebook page and writes them as Markdown files with Obsidian-compatible frontmatter and block references.

This Rust based CLI app is based on the [obsidian-kindle-plugin](https://github.com/hadynz/obsidian-kindle-plugin) by Hady Osman, but extends it with a few extras:

- **Local HTML archive** of your Amazon highlights page, so you can resync offline or diff changes over time
- **Email reports** via [Resend](https://resend.com) — useful for scheduled/automated syncs so you know what changed without checking the vault
- **Daily sync scheduling** (launchd) with per-run log files and early session-expiry detection
- **Customisable templates** (Tera) for book files, highlights, and filenames
- **Obsidian-friendly filenames** — sanitises characters that break Obsidian links

## Install

Build from source. Requires a [Rust toolchain](https://rustup.rs) and Chrome/Chromium installed (Chrome is used for Amazon login and fetching the book list).

```
git clone https://github.com/Pauked/flint.git
cd flint
cargo install --path .
```

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
flint sync --email              # send email report after sync
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

Re-running `sync` on existing files is safe. New highlights are inserted in the correct position. Existing highlights (and any edits you've made to them) are preserved. The `^ref-{id}` block references on each highlight line enable this diffing. Books are matched by `kindle-book-id` with fallback to ASIN for resilience against Amazon title changes.

### Email notifications

Opt-in email reports via [Resend](https://resend.com). Add an `[email]` section to `config.toml`:

```toml
highlight_layout = "quote"   # "quote" (default) or "line"

[highlight_colours]
style = "obsidian"   # "obsidian" (==🟣text==) or "painter" (<mark class="hltr-p">text</mark>)
text = false         # colour the highlighted passage
label = true         # colour the colour-name label

[email]
to = "you@example.com"
from = "flint@yourdomain.com"
resend_api_key = "re_xxx..."
```

Then pass `--email` to any sync command. The email includes sync stats, per-book details, the log file path, and the flint version. Email failures are logged as warnings and never affect the sync exit code.

### Logging

```
flint -v sync         # debug output to console + log file
flint -vv sync        # trace output to console + log file
```

Verbose mode writes a rolling log file to your system temp directory (`$TMPDIR/flint.log`, 3MB, 3 rotations). The log path is printed at startup.

Each `sync` command also creates a per-run log file in the log directory (default: `{data_dir}/logs/`). Old logs are cleaned up automatically after 30 days. Both are configurable:

```toml
log_dir = "~/flint-logs"
keep_logs_days = 30
```

## Data directory

Flint stores its config, session cookies, and sync state in a single data directory. It checks two locations in order:

1. **Binary-relative** — if `config.toml` exists next to the `flint` executable, that directory is used for all data files. This makes self-contained deployments possible (e.g. in Dropbox with shell scripts).
2. **Fallback** — `~/.config/flint/`

Run with `-v` to see which data directory is active.

## Config

Optional. Create `config.toml` in the data directory:

```toml
output_dir = "~/Obsidian/Books"
region = "global"
download_metadata = true
ignored_books = ["Sample Book", "Free Preview"]
# log_dir = "~/flint-logs"    # default: {data_dir}/logs/
# keep_logs_days = 30

[templates]
# Override default templates (Tera syntax)
# file_template = "..."
# highlight_template = "..."
filename_template = "{{authors_last_names}}-{{title}}"

[email]
to = "you@example.com"
from = "flint@yourdomain.com"
resend_api_key = "re_xxx..."
```

### Filename template variables

- `{{title}}` — shortened title (strips parentheticals and subtitles)
- `{{authors_last_names}}` — "Clear", "Blandy-Orendorff", or "Smith_et_al"
- `{{lastAnnotatedDate}}` — date in `YYYY-MM-DD` format
- `{{firstAuthorFirstName}}`, `{{firstAuthorLastName}}` — first author's parsed names
- `{{secondAuthorFirstName}}`, `{{secondAuthorLastName}}` — second author's parsed names
- `{{publicationDate}}` — raw publication date string from Amazon metadata

### Highlight template variables

- `{{text}}`, `{{note}}`, `{{location}}`, `{{page}}`, `{{app_link}}`, `{{id}}` — the highlight's raw fields
- `{{color}}`, `{{color_code}}` — Kindle colour name and its `hltr-*` suffix (`y`, `g`, `p`, `b`, `r`, `o`)
- `{{coloured_text}}`, `{{colour_label}}` — text and colour name with `[highlight_colours]` markup applied
- `{{block_ref}}` — the `^ref-…` block ID; place it to choose its line, otherwise Flint appends it to the line holding the highlight text

### Template filters

- `{{ publication_date | dateformat(format="%B %Y") }}` — format date strings; parses `January 1, 2020`, `2024-03-15`, and `2024` formats. Falls back to the original string if parsing fails.

## Output format

Files are written with Obsidian-compatible YAML frontmatter (`kindle-*` properties) and block references. Highlight colours are rendered as Obsidian colour highlights by default — see [Highlight colors](#highlight-colors). Filenames are sanitized to avoid characters that break Obsidian links (`# ^ [ ] |`). Property names are kebab-case; notes written before 0.3.0 (camelCase keys or the nested `kindle-sync:` block) are not recognised, so move them aside and sync to regenerate.

```markdown
---
kindle-book-id: '49849'
kindle-title: 'Atomic Habits'
kindle-author: James Clear
kindle-asin: B01N5AX61W
kindle-last-annotated-date: '2024-08-27'
kindle-book-image-url: 'https://...'
kindle-highlights-count: 97
flint-last-sync-date: '2024-08-27T19:50'
flint-version: 0.2.5
---
# Atomic Habits
## Metadata
* Author: [[James Clear]]
* ASIN: B01N5AX61W
* Reference: https://www.amazon.com/dp/B01N5AX61W
* [Kindle link](kindle://book?action=open&asin=B01N5AX61W)

## Highlights
> Habits are the compound interest of self-improvement.

**Highlight** (==yellow==) - location: [259](kindle://book?action=open&asin=B01N5AX61W&location=259) ^ref-5675

---
```

### Highlight layout

`highlight_layout = "quote"` (default) writes each highlight as a blockquote with a metadata line:

```markdown
> People like this tend to thrive.

**Highlight** (==🟣pink==) - location: [126](kindle://…) ^ref-54321
```

`highlight_layout = "line"` keeps everything on one line, as Flint did up to 0.2.6:

```markdown
People like this tend to thrive. — ==🟣pink== | location: [126](kindle://…) ^ref-54321
```

The `^ref-` block ID sits on the metadata line in the quote layout, so it reads as metadata
rather than part of the quote; an Obsidian link or embed to it shows that line. A custom
`highlight_template` replaces either layout.

### Highlight colors

`[highlight_colours]` sets where the Kindle colour shows (`text`, `label`, both or neither)
and in which markup. By default only the label is coloured, so the passage stays easy to read.

| `style` | Markup | Needs |
|---|---|---|
| `obsidian` (default) | `==🟣text==` | Obsidian 1.14+, nothing else |
| `painter` | `<mark class="hltr-p">text</mark>` | the CSS snippet below |

Obsidian colour mapping: orange 🟠, green 🟢, blue and aqua 🔵, pink 🟣, red 🔴; yellow gets
no emoji, since a plain `==text==` is Obsidian's default yellow. Text that already contains
`==` is left unwrapped. The Painter plugin can interfere with Obsidian's native highlight
swatch, so turn it off when using `obsidian`. For Flint's previous look, use
`highlight_layout = "line"` with `style = "painter"` and `text = false`.

Changing these settings only affects highlights rendered from then on: a sync keeps the
existing lines in a note. Move a note aside and sync again to re-render it in full.

For `painter`, classes are `y` yellow, `g` green, `p` pink, `b` blue, `r` red, `o` orange.
These class names originally came from the Highlightr plugin, which has since been
removed from the community plugin store — existing installs keep working, but it can
no longer be installed or re-enabled. Flint therefore ships its own CSS snippet:

```bash
flint snippet install     # write the snippet into your vault and enable it
flint snippet status      # is it installed? enabled? edited locally?
flint snippet remove      # delete it and disable it
```

The vault is found by walking up from your `output_dir` to the nearest `.obsidian`
directory; pass `--vault <path>` to name one explicitly. Install and remove both
refuse to touch a snippet you have edited yourself — pass `--force` to override,
which is also how you take an updated snippet after a Flint upgrade.

Enabling writes the snippet name into the vault's `appearance.json`, leaving every
other setting intact. If Obsidian is running when you install, reload it (or toggle
the snippet in Settings → Appearance) to pick up the change.

To restyle the colors, edit the snippet in place — Flint will then leave it alone.

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
