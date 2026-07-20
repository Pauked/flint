//! Install and remove the Flint CSS snippet in an Obsidian vault.
//!
//! Flint renders highlight colours as `<mark class="hltr-y">yellow</mark>`. Those
//! classes need CSS to show up. This module manages a vault CSS snippet that
//! supplies it, including toggling it on in the vault's `appearance.json`.

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// Snippet name as Obsidian shows it in Settings → Appearance.
pub const SNIPPET_NAME: &str = "flint-highlight-colors";

/// Contents Flint installs, embedded at compile time.
pub const SNIPPET_CSS: &str = include_str!("../assets/flint-highlight-colors.css");

/// Key in `appearance.json` holding the list of active snippet names.
const ENABLED_KEY: &str = "enabledCssSnippets";

/// What `install` did, so the caller can report it accurately.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Snippet was not present and has been written.
    Installed,
    /// Snippet was already present and identical; only the enabled flag was checked.
    AlreadyCurrent,
    /// Snippet was present at an older version and has been overwritten.
    Updated,
    /// Snippet was present with local edits; left alone because `force` was not set.
    RefusedModified,
}

/// What `remove` did.
#[derive(Debug, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// Snippet file was deleted and disabled.
    Removed,
    /// Nothing to do — no snippet file in this vault.
    NotInstalled,
    /// Snippet had local edits; left alone because `force` was not set.
    RefusedModified,
}

/// Current state of the snippet in a vault.
#[derive(Debug, PartialEq, Eq)]
pub struct Status {
    pub installed: bool,
    pub enabled: bool,
    pub modified: bool,
}

/// Find the Obsidian vault containing `start` by walking up to the first
/// ancestor holding a `.obsidian` directory.
pub fn find_vault_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(".obsidian").is_dir())
        .map(Path::to_path_buf)
}

/// Resolve the vault to act on: an explicit path if given, otherwise the vault
/// containing `output_dir`.
pub fn resolve_vault(explicit: Option<&Path>, output_dir: &Path) -> Result<PathBuf> {
    match explicit {
        Some(path) if path.join(".obsidian").is_dir() => Ok(path.to_path_buf()),
        Some(path) => bail!(
            "{} does not look like an Obsidian vault (no .obsidian directory)",
            path.display()
        ),
        None => find_vault_root(output_dir).with_context(|| {
            format!(
                "Could not find an Obsidian vault above {}. Pass --vault to name one explicitly.",
                output_dir.display()
            )
        }),
    }
}

fn snippet_path(vault: &Path) -> PathBuf {
    vault
        .join(".obsidian")
        .join("snippets")
        .join(format!("{SNIPPET_NAME}.css"))
}

fn appearance_path(vault: &Path) -> PathBuf {
    vault.join(".obsidian").join("appearance.json")
}

/// Add the snippet to `enabledCssSnippets`, preserving every other setting.
/// An empty or absent file is treated as an empty settings object.
pub fn enable_in_appearance(json: &str) -> Result<String> {
    edit_enabled_list(json, |names| {
        if !names.iter().any(is_snippet_name) {
            names.push(Value::String(SNIPPET_NAME.to_string()));
        }
    })
}

/// Drop the snippet from `enabledCssSnippets`, preserving every other setting.
pub fn disable_in_appearance(json: &str) -> Result<String> {
    edit_enabled_list(json, |names| names.retain(|name| !is_snippet_name(name)))
}

fn is_snippet_name(value: &Value) -> bool {
    value.as_str() == Some(SNIPPET_NAME)
}

fn edit_enabled_list(json: &str, edit: impl FnOnce(&mut Vec<Value>)) -> Result<String> {
    let mut settings = match json.trim() {
        "" => Map::new(),
        text => serde_json::from_str(text).context("appearance.json is not valid JSON")?,
    };

    let mut names = settings
        .get(ENABLED_KEY)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    edit(&mut names);
    settings.insert(ENABLED_KEY.to_string(), Value::Array(names));

    serde_json::to_string_pretty(&settings).context("Failed to serialise appearance.json")
}

fn read_if_present(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("Failed to read {}", path.display())),
    }
}

/// Report whether the snippet is present, enabled, and locally edited.
pub fn status(vault: &Path) -> Result<Status> {
    let existing = read_if_present(&snippet_path(vault))?;
    let enabled = read_if_present(&appearance_path(vault))?
        .filter(|text| !text.trim().is_empty())
        .map(|text| -> Result<bool> {
            let settings: Value =
                serde_json::from_str(&text).context("appearance.json is not valid JSON")?;
            Ok(settings
                .get(ENABLED_KEY)
                .and_then(Value::as_array)
                .is_some_and(|names| names.iter().any(is_snippet_name)))
        })
        .transpose()?
        .unwrap_or(false);

    Ok(Status {
        installed: existing.is_some(),
        enabled,
        modified: existing.is_some_and(|text| text != SNIPPET_CSS),
    })
}

/// Write the snippet into the vault and enable it.
///
/// Refuses to overwrite a snippet that has been edited locally unless `force`
/// is set, so hand-tuned colours are never silently discarded.
pub fn install(vault: &Path, force: bool) -> Result<InstallOutcome> {
    let path = snippet_path(vault);
    let existing = read_if_present(&path)?;

    let outcome = match existing.as_deref() {
        Some(text) if text == SNIPPET_CSS => InstallOutcome::AlreadyCurrent,
        Some(_) if !force && !is_known_version(&existing) => InstallOutcome::RefusedModified,
        Some(_) => InstallOutcome::Updated,
        None => InstallOutcome::Installed,
    };

    if outcome == InstallOutcome::RefusedModified {
        return Ok(outcome);
    }

    if outcome != InstallOutcome::AlreadyCurrent {
        let dir = path.parent().unwrap_or(vault);
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create {}", dir.display()))?;
        std::fs::write(&path, SNIPPET_CSS)
            .with_context(|| format!("Failed to write {}", path.display()))?;
    }

    let appearance = appearance_path(vault);
    let current = read_if_present(&appearance)?.unwrap_or_default();
    std::fs::write(&appearance, enable_in_appearance(&current)?)
        .with_context(|| format!("Failed to write {}", appearance.display()))?;

    Ok(outcome)
}

/// Delete the snippet from the vault and disable it.
///
/// Refuses to delete a locally edited snippet unless `force` is set.
pub fn remove(vault: &Path, force: bool) -> Result<RemoveOutcome> {
    let path = snippet_path(vault);
    let Some(existing) = read_if_present(&path)? else {
        return Ok(RemoveOutcome::NotInstalled);
    };

    if existing != SNIPPET_CSS && !force && !is_known_version(&Some(existing)) {
        return Ok(RemoveOutcome::RefusedModified);
    }

    std::fs::remove_file(&path).with_context(|| format!("Failed to delete {}", path.display()))?;

    let appearance = appearance_path(vault);
    if let Some(current) = read_if_present(&appearance)? {
        std::fs::write(&appearance, disable_in_appearance(&current)?)
            .with_context(|| format!("Failed to write {}", appearance.display()))?;
    }

    Ok(RemoveOutcome::Removed)
}

/// Recognise snippets Flint wrote itself, so upgrades from an earlier Flint
/// version overwrite cleanly while genuinely hand-edited files are protected.
fn is_known_version(existing: &Option<String>) -> bool {
    existing
        .as_deref()
        .is_some_and(|text| text.contains("Managed by `flint snippet install`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enable_adds_snippet_to_empty_settings() {
        let result = enable_in_appearance("").unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed[ENABLED_KEY][0], SNIPPET_NAME);
    }

    #[test]
    fn test_enable_preserves_other_settings() {
        let json = r##"{"theme":"obsidian","accentColor":"#ff0000"}"##;
        let result = enable_in_appearance(json).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["theme"], "obsidian");
        assert_eq!(parsed["accentColor"], "#ff0000");
        assert_eq!(parsed[ENABLED_KEY][0], SNIPPET_NAME);
    }

    #[test]
    fn test_enable_keeps_existing_snippets() {
        let json = r#"{"enabledCssSnippets":["task-fixes"]}"#;
        let result = enable_in_appearance(json).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let names = parsed[ENABLED_KEY].as_array().unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0], "task-fixes");
        assert_eq!(names[1], SNIPPET_NAME);
    }

    #[test]
    fn test_enable_is_idempotent() {
        let json = format!(r#"{{"enabledCssSnippets":["{SNIPPET_NAME}"]}}"#);
        let result = enable_in_appearance(&json).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed[ENABLED_KEY].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_disable_removes_only_our_snippet() {
        let json = format!(r#"{{"enabledCssSnippets":["task-fixes","{SNIPPET_NAME}"]}}"#);
        let result = disable_in_appearance(&json).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let names = parsed[ENABLED_KEY].as_array().unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "task-fixes");
    }

    #[test]
    fn test_disable_on_missing_key_is_harmless() {
        let result = disable_in_appearance(r#"{"theme":"obsidian"}"#).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["theme"], "obsidian");
        assert!(parsed[ENABLED_KEY].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_invalid_json_is_an_error() {
        assert!(enable_in_appearance("{not json").is_err());
    }

    #[test]
    fn test_shipped_css_is_self_identifying() {
        // install/remove rely on this marker to tell Flint's own file from a hand-edited one
        assert!(is_known_version(&Some(SNIPPET_CSS.to_string())));
        assert!(!is_known_version(&Some("mark { background: red }".into())));
    }

    #[test]
    fn test_shipped_css_covers_every_colour_code() {
        for code in ["y", "g", "p", "b", "r", "o"] {
            assert!(
                SNIPPET_CSS.contains(&format!("hltr-{code}")),
                "snippet is missing a rule for hltr-{code}"
            );
        }
    }

    /// Build a throwaway vault under the system temp dir.
    fn temp_vault(label: &str) -> PathBuf {
        let vault = std::env::temp_dir().join(format!("flint-snippet-test-{label}"));
        let _ = std::fs::remove_dir_all(&vault);
        std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
        vault
    }

    #[test]
    fn test_find_vault_root_walks_up() {
        let vault = temp_vault("find-root");
        let nested = vault.join("300 Book Highlights").join("sub");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(find_vault_root(&nested), Some(vault.clone()));
        std::fs::remove_dir_all(&vault).unwrap();
    }

    #[test]
    fn test_find_vault_root_returns_none_outside_a_vault() {
        let dir = std::env::temp_dir().join("flint-snippet-test-no-vault");
        std::fs::create_dir_all(&dir).unwrap();
        // temp_dir itself has no .obsidian, so the walk should come up empty
        assert_eq!(find_vault_root(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_install_then_remove_round_trip() {
        let vault = temp_vault("round-trip");

        assert_eq!(install(&vault, false).unwrap(), InstallOutcome::Installed);
        let after_install = status(&vault).unwrap();
        assert!(after_install.installed);
        assert!(after_install.enabled);
        assert!(!after_install.modified);

        assert_eq!(
            install(&vault, false).unwrap(),
            InstallOutcome::AlreadyCurrent
        );

        assert_eq!(remove(&vault, false).unwrap(), RemoveOutcome::Removed);
        let after_remove = status(&vault).unwrap();
        assert!(!after_remove.installed);
        assert!(!after_remove.enabled);

        assert_eq!(remove(&vault, false).unwrap(), RemoveOutcome::NotInstalled);
        std::fs::remove_dir_all(&vault).unwrap();
    }

    #[test]
    fn test_install_refuses_to_clobber_hand_edited_snippet() {
        let vault = temp_vault("hand-edited");
        install(&vault, false).unwrap();
        std::fs::write(snippet_path(&vault), "mark { background: hotpink }").unwrap();

        assert_eq!(
            install(&vault, false).unwrap(),
            InstallOutcome::RefusedModified
        );
        assert_eq!(
            std::fs::read_to_string(snippet_path(&vault)).unwrap(),
            "mark { background: hotpink }"
        );

        assert_eq!(install(&vault, true).unwrap(), InstallOutcome::Updated);
        std::fs::remove_dir_all(&vault).unwrap();
    }

    #[test]
    fn test_remove_refuses_to_delete_hand_edited_snippet() {
        let vault = temp_vault("hand-edited-remove");
        install(&vault, false).unwrap();
        std::fs::write(snippet_path(&vault), "mark { background: hotpink }").unwrap();

        assert_eq!(
            remove(&vault, false).unwrap(),
            RemoveOutcome::RefusedModified
        );
        assert!(snippet_path(&vault).exists());

        assert_eq!(remove(&vault, true).unwrap(), RemoveOutcome::Removed);
        assert!(!snippet_path(&vault).exists());
        std::fs::remove_dir_all(&vault).unwrap();
    }

    #[test]
    fn test_install_preserves_unrelated_appearance_settings() {
        let vault = temp_vault("preserve-appearance");
        std::fs::write(
            appearance_path(&vault),
            r#"{"theme":"obsidian","enabledCssSnippets":["task-fixes"]}"#,
        )
        .unwrap();

        install(&vault, false).unwrap();
        let text = std::fs::read_to_string(appearance_path(&vault)).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["theme"], "obsidian");
        let names = parsed[ENABLED_KEY].as_array().unwrap();
        assert!(names.iter().any(|n| n == "task-fixes"));
        assert!(names.iter().any(is_snippet_name));

        remove(&vault, false).unwrap();
        let text = std::fs::read_to_string(appearance_path(&vault)).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["theme"], "obsidian");
        let names = parsed[ENABLED_KEY].as_array().unwrap();
        assert!(names.iter().any(|n| n == "task-fixes"));
        assert!(!names.iter().any(is_snippet_name));

        std::fs::remove_dir_all(&vault).unwrap();
    }

    #[test]
    fn test_resolve_vault_rejects_a_non_vault_path() {
        let dir = std::env::temp_dir().join("flint-snippet-test-not-a-vault");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(resolve_vault(Some(&dir), &dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
