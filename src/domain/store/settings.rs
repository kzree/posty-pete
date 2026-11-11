use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use anyhow::Context as _;

use crate::domain::settings::Settings;

/// The settings in a single JSON file.
pub struct SettingsStore {
    path: PathBuf,
    /// The file couldn't be read, so it's moved aside before the first save.
    backup_pending: bool,
}

impl SettingsStore {
    /// Opens `path` and loads it. A missing file loads as defaults, and so
    /// does an unreadable one, which is kept until the first save.
    pub fn open(path: PathBuf) -> (Self, Settings) {
        let (settings, backup_pending) = match read(&path) {
            Ok(settings) => (settings.unwrap_or_default(), false),
            Err(error) => {
                tracing::warn!("using default settings: {error:#}");
                (Settings::default(), true)
            }
        };
        let store = Self {
            path,
            backup_pending,
        };
        (store, settings)
    }

    pub fn save(&mut self, settings: &Settings) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        if self.backup_pending {
            self.back_up()?;
            self.backup_pending = false;
        }
        super::write_json_atomically(&self.path, settings)
    }

    fn back_up(&self) -> anyhow::Result<()> {
        let backup = backup_path(&self.path);
        match fs::rename(&self.path, &backup) {
            Ok(()) => {
                tracing::info!("kept the unreadable settings in {}", backup.display());
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e)
                .with_context(|| format!("moving {} to {}", self.path.display(), backup.display())),
        }
    }
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

/// `Ok(None)` if there is no file yet.
fn read(path: &Path) -> anyhow::Result<Option<Settings>> {
    let json = match fs::read(path) {
        Ok(json) => json,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let settings =
        serde_json::from_slice(&json).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(settings))
}

/// Hands out saves that can run on any thread, in any order: a save never
/// lands over a newer one.
pub struct SettingsWriter {
    shared: Arc<Mutex<Shared>>,
    next: u64,
}

struct Shared {
    store: SettingsStore,
    /// The newest save that ran, whether or not it succeeded.
    attempted: u64,
    /// The newest save that succeeded.
    written: u64,
}

impl SettingsWriter {
    pub fn new(store: SettingsStore) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                store,
                attempted: 0,
                written: 0,
            })),
            next: 0,
        }
    }

    /// A save of `settings`. It does nothing if a newer save already ran,
    /// even one that failed.
    pub fn job(
        &mut self,
        settings: Settings,
    ) -> impl FnOnce() -> anyhow::Result<()> + Send + 'static {
        self.next += 1;
        let (seq, shared) = (self.next, self.shared.clone());
        move || {
            let mut shared = shared.lock().unwrap_or_else(PoisonError::into_inner);
            if shared.attempted >= seq {
                return Ok(());
            }
            shared.attempted = seq;
            shared.store.save(&settings)?;
            shared.written = seq;
            Ok(())
        }
    }

    /// Whether the newest save handed out hasn't run yet or failed.
    pub fn has_pending(&self) -> bool {
        let shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        shared.written < self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        settings::{AppearanceSettings, ProxyMode, ThemePreference},
        store::test_support::{TempDir, file_names},
    };

    fn file(dir: &TempDir) -> PathBuf {
        dir.path().join("settings.json")
    }

    fn with_theme(theme: ThemePreference) -> Settings {
        Settings {
            appearance: AppearanceSettings {
                theme,
                ..AppearanceSettings::default()
            },
            ..Settings::default()
        }
    }

    fn write_file(dir: &TempDir, contents: &str) {
        fs::create_dir_all(dir.path()).unwrap();
        fs::write(file(dir), contents).unwrap();
    }

    #[test]
    fn save_then_open_round_trips() {
        let dir = TempDir::new();
        let (mut store, _) = SettingsStore::open(file(&dir));
        store.save(&with_theme(ThemePreference::Light)).unwrap();

        let (_, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, with_theme(ThemePreference::Light));
        assert_eq!(file_names(dir.path()), ["settings.json"]);
    }

    #[test]
    fn request_settings_survive_a_save() {
        let dir = TempDir::new();
        let (mut store, _) = SettingsStore::open(file(&dir));
        let mut settings = Settings::default();
        settings.requests.timeout_secs = 0;
        settings.requests.proxy = ProxyMode::None;
        settings.requests.user_agent = "posty-pete/1".into();
        store.save(&settings).unwrap();

        let (_, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, settings);
    }

    #[test]
    fn theme_is_stored_lowercase() {
        let dir = TempDir::new();
        let (mut store, _) = SettingsStore::open(file(&dir));
        store.save(&with_theme(ThemePreference::System)).unwrap();
        let json = fs::read_to_string(file(&dir)).unwrap();
        assert!(json.contains(r#""theme": "system""#), "{json}");
    }

    #[test]
    fn missing_file_loads_defaults_with_dark_theme() {
        let dir = TempDir::new();
        let (_, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, Settings::default());
        assert_eq!(loaded.appearance.theme, ThemePreference::Dark);
    }

    #[test]
    fn missing_and_unknown_fields_are_tolerated() {
        let dir = TempDir::new();
        write_file(&dir, r#"{ "appearance": { "font": "Mono" }, "future": 1 }"#);
        let (_, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, Settings::default());

        write_file(&dir, r#"{ "appearance": { "theme": "light" } }"#);
        let (_, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, with_theme(ThemePreference::Light));
    }

    #[test]
    fn broken_file_loads_defaults_and_is_kept_until_the_first_save() {
        let dir = TempDir::new();
        write_file(&dir, "{ not json");
        let (mut store, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, Settings::default());
        assert_eq!(fs::read_to_string(file(&dir)).unwrap(), "{ not json");

        store.save(&with_theme(ThemePreference::Light)).unwrap();
        let backup = dir.path().join("settings.json.bak");
        assert_eq!(fs::read_to_string(backup).unwrap(), "{ not json");
        let (_, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, with_theme(ThemePreference::Light));
    }

    #[test]
    fn backup_is_made_only_once() {
        let dir = TempDir::new();
        write_file(&dir, "{ not json");
        let (mut store, _) = SettingsStore::open(file(&dir));
        store.save(&with_theme(ThemePreference::Light)).unwrap();
        store.save(&with_theme(ThemePreference::System)).unwrap();

        let backup = dir.path().join("settings.json.bak");
        assert_eq!(fs::read_to_string(backup).unwrap(), "{ not json");
    }

    #[test]
    fn invalid_theme_counts_as_broken() {
        let dir = TempDir::new();
        write_file(&dir, r#"{ "appearance": { "theme": "purple" } }"#);
        let (mut store, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, Settings::default());
        store.save(&Settings::default()).unwrap();
        assert_eq!(
            file_names(dir.path()),
            ["settings.json", "settings.json.bak"]
        );
    }

    #[test]
    fn older_save_never_lands_over_a_newer_one() {
        let dir = TempDir::new();
        let (store, _) = SettingsStore::open(file(&dir));
        let mut writer = SettingsWriter::new(store);
        let older = writer.job(with_theme(ThemePreference::Light));
        let newer = writer.job(with_theme(ThemePreference::System));

        newer().unwrap();
        older().unwrap();

        let (_, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, with_theme(ThemePreference::System));
    }

    #[test]
    fn pending_until_the_newest_save_ran() {
        let dir = TempDir::new();
        let (store, _) = SettingsStore::open(file(&dir));
        let mut writer = SettingsWriter::new(store);
        assert!(!writer.has_pending());

        let older = writer.job(with_theme(ThemePreference::Light));
        let newer = writer.job(with_theme(ThemePreference::System));
        older().unwrap();
        assert!(writer.has_pending());
        newer().unwrap();
        assert!(!writer.has_pending());
    }

    #[test]
    fn saves_in_order_all_land() {
        let dir = TempDir::new();
        let (store, _) = SettingsStore::open(file(&dir));
        let mut writer = SettingsWriter::new(store);
        writer.job(with_theme(ThemePreference::Light))().unwrap();
        let (_, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, with_theme(ThemePreference::Light));

        writer.job(with_theme(ThemePreference::Dark))().unwrap();
        let (_, loaded) = SettingsStore::open(file(&dir));
        assert_eq!(loaded, with_theme(ThemePreference::Dark));
    }

    /// A path whose parent is a file, so saving fails until `unblock`.
    fn blocked_file(dir: &TempDir) -> PathBuf {
        fs::create_dir_all(dir.path()).unwrap();
        fs::write(dir.path().join("blocker"), "").unwrap();
        dir.path().join("blocker").join("settings.json")
    }

    fn unblock(dir: &TempDir) {
        fs::remove_file(dir.path().join("blocker")).unwrap();
    }

    #[test]
    fn failed_save_stays_pending_until_a_later_save_lands() {
        let dir = TempDir::new();
        let path = blocked_file(&dir);
        let (store, _) = SettingsStore::open(path.clone());
        let mut writer = SettingsWriter::new(store);

        assert!(writer.job(with_theme(ThemePreference::Light))().is_err());
        assert!(writer.has_pending());

        unblock(&dir);
        writer.job(with_theme(ThemePreference::System))().unwrap();
        assert!(!writer.has_pending());
        let (_, loaded) = SettingsStore::open(path);
        assert_eq!(loaded, with_theme(ThemePreference::System));
    }

    #[test]
    fn older_save_skips_after_a_newer_one_failed() {
        let dir = TempDir::new();
        let path = blocked_file(&dir);
        let (store, _) = SettingsStore::open(path.clone());
        let mut writer = SettingsWriter::new(store);
        let older = writer.job(with_theme(ThemePreference::Light));
        let newer = writer.job(with_theme(ThemePreference::System));

        assert!(newer().is_err());
        unblock(&dir);
        older().unwrap();
        assert!(!path.exists());
        assert!(writer.has_pending());

        writer.job(with_theme(ThemePreference::Dark))().unwrap();
        assert!(!writer.has_pending());
        let (_, loaded) = SettingsStore::open(path);
        assert_eq!(loaded, with_theme(ThemePreference::Dark));
    }
}
