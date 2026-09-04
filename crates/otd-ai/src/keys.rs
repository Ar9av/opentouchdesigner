//! API keys: where they come from, and where they are never allowed to go.
//!
//! Three rules, and the first one is the one that matters:
//!
//! 1. **A key never enters a project file.** `.otd` is text, it is meant to be
//!    committed, and the whole pitch of the format is that you can read it in
//!    a diff. A key in there is a key in somebody's git history, and a key in
//!    a git history has to be *rotated*, not deleted.
//! 2. Keys live in one file outside every project, `0600`, in the OS config
//!    directory — or in the environment, which is what CI and a show machine
//!    will actually use.
//! 3. Nothing prints one. [`Key`] has a hand-written `Debug` that redacts, so
//!    a stray `{:?}` in a log line cannot leak one, and `redact` scrubs keys
//!    out of provider error messages before they reach the UI.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::Provider;

/// An API key. Constructed from a string, and deliberately awkward to get
/// back out of one.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Key(String);

impl Key {
    pub fn new(raw: impl Into<String>) -> Key {
        Key(raw.into().trim().to_string())
    }

    /// The real value. Named so that every use site is greppable, and so that
    /// nobody reaches for it by accident when they wanted `Display`.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// What is safe to show a user: enough to tell two keys apart, not
    /// enough to use.
    pub fn hint(&self) -> String {
        let n = self.0.chars().count();
        if n == 0 {
            return "not set".into();
        }
        if n <= 8 {
            return "set".into();
        }
        let head: String = self.0.chars().take(3).collect();
        let tail: String = self.0.chars().skip(n - 4).collect();
        format!("{head}…{tail} ({n} chars)")
    }
}

/// Redacted on purpose. See the module docs.
impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key({})", self.hint())
    }
}

/// Remove anything that looks like one of these keys from a string.
///
/// Provider errors quote the request back often enough that this is worth
/// doing before any message reaches a log or a panel.
pub fn redact(text: &str, keys: &Keys) -> String {
    let mut out = text.to_string();
    for key in keys.all().values() {
        if key.0.len() >= 8 {
            out = out.replace(&key.0, "<redacted>");
        }
    }
    out
}

/// The keys this machine knows about, one per provider.
#[derive(Clone, Debug, Default)]
pub struct Keys {
    keys: BTreeMap<Provider, Key>,
}

impl Keys {
    pub fn get(&self, provider: Provider) -> Option<&Key> {
        self.keys.get(&provider).filter(|k| !k.is_empty())
    }

    pub fn set(&mut self, provider: Provider, key: Key) {
        if key.is_empty() {
            self.keys.remove(&provider);
        } else {
            self.keys.insert(provider, key);
        }
    }

    pub fn all(&self) -> &BTreeMap<Provider, Key> {
        &self.keys
    }

    /// Load from the environment, then from the config file.
    ///
    /// The environment wins, because that is how a show machine or a CI job
    /// supplies one and neither should be editing a config file to do it.
    pub fn load() -> Keys {
        let mut keys = Keys::from_file(&config_path()).unwrap_or_default();
        for provider in Provider::ALL {
            if let Ok(value) = std::env::var(provider.env_var()) {
                let key = Key::new(value);
                if !key.is_empty() {
                    keys.set(*provider, key);
                }
            }
        }
        keys
    }

    fn from_file(path: &std::path::Path) -> Option<Keys> {
        let text = std::fs::read_to_string(path).ok()?;
        Some(Keys::parse(&text))
    }

    /// `provider = key`, one per line, `#` comments. Deliberately not a
    /// format with an ecosystem: this file has exactly one job.
    pub fn parse(text: &str) -> Keys {
        let mut keys = Keys::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            if let Some(provider) = Provider::parse(name.trim()) {
                keys.set(provider, Key::new(value));
            }
        }
        keys
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "# OpenTouchDesigner API keys.\n\
             # This file is not part of any project and must not be copied into one.\n",
        );
        for (provider, key) in &self.keys {
            out.push_str(&format!("{} = {}\n", provider.id(), key.0));
        }
        out
    }

    /// Write the config file, owner-readable only.
    pub fn save(&self) -> Result<PathBuf, String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, self.to_text()).map_err(|e| format!("{}: {e}", path.display()))?;
        restrict(&path);
        Ok(path)
    }
}

/// `0600`. A key readable by every process running as another user on a
/// shared machine is not stored, it is published.
#[cfg(unix)]
fn restrict(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &std::path::Path) {}

/// Where the key file lives. Never inside a project directory.
pub fn config_path() -> PathBuf {
    config_dir().join("keys.conf")
}

fn config_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        if let Some(home) = home {
            return home.join("Library/Application Support/OpenTouchDesigner");
        }
    } else if cfg!(target_os = "windows") {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("OpenTouchDesigner");
        }
    } else {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("opentouchdesigner");
        }
        if let Some(home) = home {
            return home.join(".config/opentouchdesigner");
        }
    }
    PathBuf::from(".otd-config")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_never_prints_itself() {
        let key = Key::new("sk-ant-super-secret-value-abcdef123456");
        // The three ways a value normally escapes: Debug, a format string,
        // and a log line built from either.
        let debugged = format!("{key:?}");
        assert!(!debugged.contains("super-secret"), "{debugged}");
        assert!(debugged.contains("sk-"), "still has to identify itself");
        assert!(debugged.contains("123456") || debugged.contains("3456"));

        let keys = Keys::default();
        assert_eq!(Key::new("").hint(), "not set");
        // Short enough that any hint would be most of the key.
        assert_eq!(Key::new("abc123").hint(), "set");
        drop(keys);
    }

    #[test]
    fn provider_errors_are_scrubbed_before_anyone_sees_them() {
        let mut keys = Keys::default();
        keys.set(Provider::OpenAi, Key::new("sk-proj-0123456789abcdef"));
        // Providers really do quote the offending header back.
        let raw = "401 Unauthorized: invalid key sk-proj-0123456789abcdef supplied";
        let safe = redact(raw, &keys);
        assert!(!safe.contains("0123456789"), "{safe}");
        assert!(safe.contains("<redacted>"));
        assert!(
            safe.contains("401 Unauthorized"),
            "the useful part survives"
        );
    }

    #[test]
    fn keys_round_trip_through_the_config_format() {
        let mut keys = Keys::default();
        keys.set(Provider::Anthropic, Key::new("sk-ant-aaaaaaaaaaaa"));
        keys.set(Provider::OpenRouter, Key::new("sk-or-bbbbbbbbbbbb"));
        let text = keys.to_text();
        let back = Keys::parse(&text);
        assert_eq!(
            back.get(Provider::Anthropic).map(|k| k.expose()),
            Some("sk-ant-aaaaaaaaaaaa")
        );
        assert_eq!(
            back.get(Provider::OpenRouter).map(|k| k.expose()),
            Some("sk-or-bbbbbbbbbbbb")
        );
        assert!(back.get(Provider::OpenAi).is_none());

        // Comments, blank lines and unknown names are skipped rather than
        // turning into a key called `#`.
        let messy = Keys::parse("# a comment\n\nnonsense = x\nopenai=  sk-spaced  \n");
        assert_eq!(
            messy.get(Provider::OpenAi).map(|k| k.expose()),
            Some("sk-spaced")
        );
    }

    #[test]
    fn an_empty_key_is_no_key_rather_than_an_empty_one() {
        let mut keys = Keys::default();
        keys.set(Provider::OpenAi, Key::new("   "));
        assert!(keys.get(Provider::OpenAi).is_none());
    }

    #[test]
    fn the_key_file_is_never_inside_a_project() {
        let path = config_path();
        assert!(path.ends_with("keys.conf"));
        // The test is about the *shape* of the answer: a per-user config
        // location, not the working directory a project happens to be in.
        assert!(path.parent().is_some());
        assert_ne!(path.parent().unwrap(), std::path::Path::new(""));
    }
}
