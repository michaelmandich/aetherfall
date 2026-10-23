//! What the multiplayer client remembers between runs: the last server
//! and username. A small RON file in the user's config folder
//! (`%APPDATA%\aetherfall\multiplayer.ron` on Windows,
//! `$XDG_CONFIG_HOME/aetherfall/multiplayer.ron` or
//! `~/.config/aetherfall/multiplayer.ron` elsewhere).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientConfig {
    /// `host:port` of the last server joined.
    #[serde(default)]
    pub server: String,
    /// The last username logged in with.
    #[serde(default)]
    pub username: String,
    /// The player's display name in casual rooms.
    #[serde(default)]
    pub name: String,
}

/// Where the config file lives, or `None` with no home to put it in.
pub fn config_path() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(dir.join("aetherfall").join("multiplayer.ron"))
}

impl ClientConfig {
    pub fn load() -> ClientConfig {
        Self::load_from(config_path().as_deref())
    }

    pub fn load_from(path: Option<&std::path::Path>) -> ClientConfig {
        let Some(path) = path else { return ClientConfig::default() };
        let Ok(text) = std::fs::read_to_string(path) else { return ClientConfig::default() };
        ron::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(path) = config_path() {
            let _ = self.save_to(&path);
        }
    }

    pub fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_and_tolerates_missing_or_bad_files() {
        let dir = std::env::temp_dir().join(format!("af_net_config_{}", std::process::id()));
        let path = dir.join("sub").join("multiplayer.ron");
        let c = ClientConfig { server: "pi.local:7777".into(), username: "Fin".into(), name: "Fin".into() };
        c.save_to(&path).unwrap();
        assert_eq!(ClientConfig::load_from(Some(&path)), c);
        std::fs::write(&path, "not ron at all").unwrap();
        assert_eq!(ClientConfig::load_from(Some(&path)), ClientConfig::default());
        std::fs::write(&path, "(server: \"a:1\")").unwrap();
        assert_eq!(ClientConfig::load_from(Some(&path)).server, "a:1", "missing fields default");
        assert_eq!(ClientConfig::load_from(Some(&dir.join("nothing.ron"))), ClientConfig::default());
        assert_eq!(ClientConfig::load_from(None), ClientConfig::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
