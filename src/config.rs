//! Your settings: a small TOML file, plus environment variables that win
//! over it.
//!
//! Why a file in your home folder and not a `.env` in the project? `am` is
//! installed globally and runs from any folder, so a `.env` next to the code
//! would never be found. `~/.config/am/config.toml` is the usual place for a
//! command-line tool's settings, it's outside the repository (your playlist
//! names stay out of git), and moving to another Mac is just copying one
//! file. See `config.example.toml` for every setting.
//!
//! Order, last wins: built-in defaults → the file → `AM_*` environment
//! variables.

use serde::Deserialize;
use std::path::PathBuf;

/// The example file, built into the program so `am --init-config` can write
/// it. `include_str!` reads the file at *compile* time.
pub const EXAMPLE: &str = include_str!("../config.example.toml");

/// Rust note: `#[derive(Deserialize)]` (from the serde library) writes the
/// code that turns TOML text into this struct. `#[serde(default)]` fills in
/// any missing field from `Default` below, and `deny_unknown_fields` turns a
/// typo like `favourites` into an error instead of silently ignoring it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Playlist names pinned to the top of the Playlists tab, in order.
    pub favorites: Vec<String>,
    pub show_art: bool,
    pub seek_seconds: u32,
    pub volume_step: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            favorites: Vec::new(),
            show_art: true,
            seek_seconds: 5,
            volume_step: 5,
        }
    }
}

/// What `load` found: the settings, where it looked, and anything wrong
/// (shown on the bottom line, never fatal: a broken file means defaults).
#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    pub path: Option<PathBuf>,
    pub file_found: bool,
    pub problems: Vec<String>,
}

/// Where the settings file is: `$AM_CONFIG` if set, else
/// `$XDG_CONFIG_HOME/am/config.toml`, else `~/.config/am/config.toml`.
pub fn path() -> Option<PathBuf> {
    path_from(|name| std::env::var(name).ok())
}

/// `path`, with the environment passed in so tests can fake it.
fn path_from(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    // `filter` drops empty values: `AM_CONFIG=` counts as unset.
    let get = |name: &str| env(name).filter(|v| !v.is_empty());
    if let Some(p) = get("AM_CONFIG") {
        return Some(PathBuf::from(p));
    }
    let base = match get("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(get("HOME")?).join(".config"),
    };
    Some(base.join("am").join("config.toml"))
}

/// Read the file (if any) and the environment.
pub fn load() -> Loaded {
    let path = path();
    let text = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
    let file_found = text.is_some();
    let (config, problems) = build(text.as_deref(), |name| std::env::var(name).ok());
    Loaded {
        config,
        path,
        file_found,
        problems,
    }
}

/// The pure part of `load`: file text + environment → settings + problems.
/// No disk or real environment, so it's easy to test.
fn build(file: Option<&str>, env: impl Fn(&str) -> Option<String>) -> (Config, Vec<String>) {
    let mut problems = Vec::new();
    let mut config = match file.map(toml::from_str::<Config>) {
        None => Config::default(),
        Some(Ok(c)) => c,
        Some(Err(e)) => {
            // `e.message()` is the short reason, e.g. "unknown field `x`".
            problems.push(format!("Settings file ignored: {}", e.message()));
            Config::default()
        }
    };

    if let Some(v) = env("AM_FAVORITES") {
        config.favorites = v
            .split(',')
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
    }
    if let Some(v) = env("AM_SHOW_ART") {
        match v.trim().to_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => config.show_art = true,
            "0" | "false" | "no" | "off" => config.show_art = false,
            _ => problems.push(format!("AM_SHOW_ART={v} isn't true/false")),
        }
    }
    for (name, field) in [
        ("AM_SEEK_SECONDS", &mut config.seek_seconds),
        ("AM_VOLUME_STEP", &mut config.volume_step),
    ] {
        if let Some(v) = env(name) {
            match v.trim().parse() {
                Ok(n) => *field = n,
                Err(_) => problems.push(format!("{name}={v} isn't a whole number")),
            }
        }
    }

    // Keep numbers sensible rather than refusing to start.
    let clamp = |value: &mut u32, min, max, what: &str, problems: &mut Vec<String>| {
        if !(min..=max).contains(value) {
            problems.push(format!(
                "{what} must be {min}–{max}; using {}",
                (*value).clamp(min, max)
            ));
            *value = (*value).clamp(min, max);
        }
    };
    clamp(
        &mut config.seek_seconds,
        1,
        60,
        "seek_seconds",
        &mut problems,
    );
    clamp(&mut config.volume_step, 1, 25, "volume_step", &mut problems);

    (config, problems)
}

/// `am --config`: say where settings come from and what's in use.
pub fn print() {
    let loaded = load();
    match &loaded.path {
        Some(p) if loaded.file_found => println!("Settings file: {}", p.display()),
        Some(p) => println!(
            "Settings file: {} (not created yet; `am --init-config` makes one)",
            p.display()
        ),
        None => println!("Settings file: none (HOME isn't set)"),
    }
    let c = &loaded.config;
    println!("  favorites    = {:?}", c.favorites);
    println!("  show_art     = {}", c.show_art);
    println!("  seek_seconds = {}", c.seek_seconds);
    println!("  volume_step  = {}", c.volume_step);
    for p in &loaded.problems {
        println!("Problem: {p}");
    }
}

/// `am --init-config`: write the example file, unless one already exists
/// (we never overwrite your settings).
pub fn init() -> anyhow::Result<()> {
    let path = path().ok_or_else(|| anyhow::anyhow!("HOME isn't set"))?;
    if path.exists() {
        println!("Already exists, left alone: {}", path.display());
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, EXAMPLE)?;
    println!("Created {}", path.display());
    println!("Edit it, then restart am.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake environment from a list of (name, value) pairs.
    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn no_file_and_no_env_gives_defaults() {
        let (c, problems) = build(None, env(&[]));
        assert_eq!(c, Config::default());
        assert!(problems.is_empty());
    }

    #[test]
    fn the_example_file_parses() {
        let (c, problems) = build(Some(EXAMPLE), env(&[]));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(c.favorites, ["Morning Run", "Focus"]);
    }

    #[test]
    fn env_wins_over_the_file() {
        let file = "favorites = [\"a\"]\nseek_seconds = 10\n";
        let (c, problems) = build(
            Some(file),
            env(&[("AM_FAVORITES", " x , y,, "), ("AM_SHOW_ART", "off")]),
        );
        assert!(problems.is_empty());
        assert_eq!(c.favorites, ["x", "y"]);
        assert!(!c.show_art);
        assert_eq!(
            c.seek_seconds, 10,
            "file value kept when env doesn't set it"
        );
    }

    #[test]
    fn mistakes_are_reported_not_fatal() {
        let (c, problems) = build(Some("favourites = [\"typo\"]"), env(&[]));
        assert_eq!(c, Config::default());
        assert!(problems[0].contains("favourites"), "{problems:?}");

        let (c, problems) = build(Some("volume_step = 99"), env(&[("AM_SEEK_SECONDS", "ten")]));
        assert_eq!(c.volume_step, 25);
        assert_eq!(problems.len(), 2);
    }

    #[test]
    fn where_the_file_lives() {
        let p = |pairs: &[(&str, &str)]| path_from(env(pairs)).map(|p| p.display().to_string());
        assert_eq!(p(&[("HOME", "/h")]).unwrap(), "/h/.config/am/config.toml");
        assert_eq!(
            p(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "/x")]).unwrap(),
            "/x/am/config.toml"
        );
        assert_eq!(
            p(&[("HOME", "/h"), ("AM_CONFIG", "/c.toml")]).unwrap(),
            "/c.toml"
        );
        assert_eq!(p(&[]), None);
    }
}
