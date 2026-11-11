//! Every page, group and row of Settings, so the palette can find and reveal them.

use serde::{Deserialize, Serialize};

/// A page of the settings, in menu order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSection {
    #[default]
    General,
    Appearance,
    Requests,
    HistoryAndData,
    Ai,
    Shortcuts,
    About,
}

impl SettingsSection {
    pub const ALL: [Self; 7] = [
        Self::General,
        Self::Appearance,
        Self::Requests,
        Self::HistoryAndData,
        Self::Ai,
        Self::Shortcuts,
        Self::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Requests => "Requests",
            Self::HistoryAndData => "History & data",
            Self::Ai => "AI",
            Self::Shortcuts => "Shortcuts",
            Self::About => "About",
        }
    }

    /// Prefixes the ids of its settings.
    #[cfg(test)]
    fn key(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Requests => "requests",
            Self::HistoryAndData => "history",
            Self::Ai => "ai",
            Self::Shortcuts => "shortcuts",
            Self::About => "about",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::General => "How the app starts and when it asks before discarding changes.",
            Self::Appearance => "Theme, fonts and how responses are displayed.",
            Self::Requests => "Defaults for sending requests: timeout, redirects, TLS and proxy.",
            Self::HistoryAndData => "How long history is kept and where your data is stored.",
            Self::Ai => {
                "Optional AI features, their provider, and the MCP server for AI assistants."
            }
            Self::Shortcuts => "Keyboard shortcuts for common actions.",
            Self::About => "Version, licences and links.",
        }
    }

    /// Other words people search for the page by.
    pub fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::General => &["startup", "session", "preferences"],
            Self::Appearance => &["theme", "colour", "color", "font", "dark", "light"],
            Self::Requests => &["timeout", "proxy", "tls", "redirects", "network"],
            Self::HistoryAndData => &["retention", "storage", "folder", "privacy"],
            Self::Ai => &["assistant", "mcp", "claude", "llm", "model"],
            Self::Shortcuts => &["keyboard", "keybindings", "hotkeys"],
            Self::About => &["version", "licences", "licenses", "credits"],
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// The section before (`forward == false`) or after this one, stopping at the ends.
    pub fn neighbor(self, forward: bool) -> Self {
        let index = self.index();
        let index = if forward {
            (index + 1).min(Self::ALL.len() - 1)
        } else {
            index.saturating_sub(1)
        };
        Self::ALL[index]
    }
}

/// A titled group of rows on a settings page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsGroup {
    Startup,
    Editing,
    Interface,
    Editors,
    Connection,
    Redirects,
    Headers,
    History,
    Data,
    Ai,
    Provider,
    Features,
    McpServer,
    McpPermissions,
    McpClientSetup,
    ShortcutsRequests,
    ShortcutsNavigation,
    ShortcutsPanels,
    ShortcutsApp,
    Credits,
}

impl SettingsGroup {
    pub const ALL: [Self; 20] = [
        Self::Startup,
        Self::Editing,
        Self::Interface,
        Self::Editors,
        Self::Connection,
        Self::Redirects,
        Self::Headers,
        Self::History,
        Self::Data,
        Self::Ai,
        Self::Provider,
        Self::Features,
        Self::McpServer,
        Self::McpPermissions,
        Self::McpClientSetup,
        Self::ShortcutsRequests,
        Self::ShortcutsNavigation,
        Self::ShortcutsPanels,
        Self::ShortcutsApp,
        Self::Credits,
    ];

    /// The element id of its group box.
    pub fn id(self) -> &'static str {
        match self {
            Self::Startup => "general-startup",
            Self::Editing => "general-editing",
            Self::Interface => "appearance-interface",
            Self::Editors => "appearance-editors",
            Self::Connection => "requests-connection",
            Self::Redirects => "requests-redirects",
            Self::Headers => "requests-headers",
            Self::History => "history",
            Self::Data => "data",
            Self::Ai => "ai-general",
            Self::Provider => "ai-provider",
            Self::Features => "ai-features",
            Self::McpServer => "mcp-server",
            Self::McpPermissions => "mcp-permissions",
            Self::McpClientSetup => "mcp-client-setup",
            Self::ShortcutsRequests => "shortcuts-requests",
            Self::ShortcutsNavigation => "shortcuts-navigation",
            Self::ShortcutsPanels => "shortcuts-panels",
            Self::ShortcutsApp => "shortcuts-app",
            Self::Credits => "about-credits",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Startup => "Startup",
            Self::Editing => "Editing",
            Self::Interface => "Interface",
            Self::Editors => "Editors",
            Self::Connection => "Connection",
            Self::Redirects => "Redirects",
            Self::Headers => "Headers",
            Self::History => "History",
            Self::Data => "Data",
            Self::Ai => "AI",
            Self::Provider => "Provider",
            Self::Features => "Features",
            Self::McpServer => "MCP server",
            Self::McpPermissions => "MCP permissions",
            Self::McpClientSetup => "Client setup",
            Self::ShortcutsRequests => "Requests",
            Self::ShortcutsNavigation => "Navigation",
            Self::ShortcutsPanels => "Panels",
            Self::ShortcutsApp => "App",
            Self::Credits => "Open source",
        }
    }

    pub fn section(self) -> SettingsSection {
        use SettingsSection as S;
        match self {
            Self::Startup | Self::Editing => S::General,
            Self::Interface | Self::Editors => S::Appearance,
            Self::Connection | Self::Redirects | Self::Headers => S::Requests,
            Self::History | Self::Data => S::HistoryAndData,
            Self::Ai
            | Self::Provider
            | Self::Features
            | Self::McpServer
            | Self::McpPermissions
            | Self::McpClientSetup => S::Ai,
            Self::ShortcutsRequests
            | Self::ShortcutsNavigation
            | Self::ShortcutsPanels
            | Self::ShortcutsApp => S::Shortcuts,
            Self::Credits => S::About,
        }
    }
}

/// A row on a settings page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Setting {
    RestoreSession,
    CheckForUpdates,
    ConfirmDiscard,
    Theme,
    Accent,
    UiFontSize,
    EditorFont,
    EditorFontSize,
    WrapResponses,
    Timeout,
    VerifyTls,
    Proxy,
    ProxyUrl,
    FollowRedirects,
    MaxRedirects,
    UserAgent,
    HistoryRetention,
    ClearHistory,
    DataFolder,
    AiEnabled,
    AiProvider,
    AiApiKey,
    AiModel,
    ExplainResponses,
    GenerateRequests,
    McpEnabled,
    McpPort,
    McpUrl,
    McpToken,
    McpReadCollections,
    McpSendRequests,
    McpAskBeforeSending,
    McpEditCollections,
    McpReadHistory,
    McpClientConfig,
    Shortcut(Shortcut),
}

struct Info {
    id: &'static str,
    group: SettingsGroup,
    label: &'static str,
    keywords: &'static [&'static str],
}

const fn info(
    id: &'static str,
    group: SettingsGroup,
    label: &'static str,
    keywords: &'static [&'static str],
) -> Info {
    Info {
        id,
        group,
        label,
        keywords,
    }
}

impl Setting {
    /// The rows other than shortcuts, in page order.
    pub const ROWS: [Self; 35] = [
        Self::RestoreSession,
        Self::CheckForUpdates,
        Self::ConfirmDiscard,
        Self::Theme,
        Self::Accent,
        Self::UiFontSize,
        Self::EditorFont,
        Self::EditorFontSize,
        Self::WrapResponses,
        Self::Timeout,
        Self::VerifyTls,
        Self::Proxy,
        Self::ProxyUrl,
        Self::FollowRedirects,
        Self::MaxRedirects,
        Self::UserAgent,
        Self::HistoryRetention,
        Self::ClearHistory,
        Self::DataFolder,
        Self::AiEnabled,
        Self::AiProvider,
        Self::AiApiKey,
        Self::AiModel,
        Self::ExplainResponses,
        Self::GenerateRequests,
        Self::McpEnabled,
        Self::McpPort,
        Self::McpUrl,
        Self::McpToken,
        Self::McpReadCollections,
        Self::McpSendRequests,
        Self::McpAskBeforeSending,
        Self::McpEditCollections,
        Self::McpReadHistory,
        Self::McpClientConfig,
    ];

    /// Every row, shortcuts last, in page order.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::ROWS
            .into_iter()
            .chain(Shortcut::all().map(Self::Shortcut))
    }

    fn info(self) -> Info {
        use SettingsGroup as G;
        match self {
            Self::RestoreSession => info(
                "general.restore-session",
                G::Startup,
                "Restore last session",
                &["startup", "reopen", "launch"],
            ),
            Self::CheckForUpdates => info(
                "general.check-for-updates",
                G::Startup,
                "Check for updates",
                &["version", "upgrade", "startup"],
            ),
            Self::ConfirmDiscard => info(
                "general.confirm-discard",
                G::Editing,
                "Ask before discarding changes",
                &["unsaved", "confirm", "prompt", "close"],
            ),
            Self::Theme => info(
                "appearance.theme",
                G::Interface,
                "Theme",
                &["dark", "light", "mode", "system"],
            ),
            Self::Accent => info(
                "appearance.accent",
                G::Interface,
                "Accent colour",
                &["color", "theme", "highlight"],
            ),
            Self::UiFontSize => info(
                "appearance.font-size",
                G::Interface,
                "Font size",
                &["text", "zoom", "interface", "scale"],
            ),
            Self::EditorFont => info(
                "appearance.editor-font",
                G::Editors,
                "Font",
                &["editor", "monospace", "typeface", "family"],
            ),
            Self::EditorFontSize => info(
                "appearance.editor-font-size",
                G::Editors,
                "Font size",
                &["editor", "monospace", "text"],
            ),
            Self::WrapResponses => info(
                "appearance.wrap-responses",
                G::Editors,
                "Wrap long lines in responses",
                &["wrapping", "word", "body"],
            ),
            Self::Timeout => info(
                "requests.timeout",
                G::Connection,
                "Timeout",
                &["request", "seconds", "slow", "wait"],
            ),
            Self::VerifyTls => info(
                "requests.verify-tls",
                G::Connection,
                "Verify TLS certificates",
                &["ssl", "https", "insecure", "security"],
            ),
            Self::Proxy => info(
                "requests.proxy",
                G::Connection,
                "Proxy",
                &["network", "http", "socks"],
            ),
            Self::ProxyUrl => info(
                "requests.proxy-url",
                G::Connection,
                "Proxy URL",
                &["network", "address", "host"],
            ),
            Self::FollowRedirects => info(
                "requests.follow-redirects",
                G::Redirects,
                "Follow redirects",
                &["location", "301", "302"],
            ),
            Self::MaxRedirects => info(
                "requests.max-redirects",
                G::Redirects,
                "Maximum redirects",
                &["limit", "hops"],
            ),
            Self::UserAgent => info(
                "requests.user-agent",
                G::Headers,
                "User-Agent",
                &["header", "browser", "client"],
            ),
            Self::HistoryRetention => info(
                "history.retention",
                G::History,
                "Keep history for",
                &["days", "retention", "prune", "delete"],
            ),
            Self::ClearHistory => info(
                "history.clear",
                G::History,
                "Clear history",
                &["delete", "remove", "erase"],
            ),
            Self::DataFolder => info(
                "history.data-folder",
                G::Data,
                "Data folder",
                &["storage", "files", "location", "directory", "finder"],
            ),
            Self::AiEnabled => info(
                "ai.enabled",
                G::Ai,
                "Enable AI features",
                &["assistant", "llm", "claude"],
            ),
            Self::AiProvider => info(
                "ai.provider",
                G::Provider,
                "Provider",
                &["ai", "anthropic", "llm"],
            ),
            Self::AiApiKey => info(
                "ai.api-key",
                G::Provider,
                "API key",
                &["ai", "token", "secret", "anthropic"],
            ),
            Self::AiModel => info("ai.model", G::Provider, "Model", &["ai", "claude", "llm"]),
            Self::ExplainResponses => info(
                "ai.explain-responses",
                G::Features,
                "Explain responses",
                &["ai", "summarize", "errors"],
            ),
            Self::GenerateRequests => info(
                "ai.generate-requests",
                G::Features,
                "Generate requests",
                &["ai", "create", "describe"],
            ),
            Self::McpEnabled => info(
                "ai.mcp-enabled",
                G::McpServer,
                "Enable MCP server",
                &["assistant", "claude code", "ai"],
            ),
            Self::McpPort => info(
                "ai.mcp-port",
                G::McpServer,
                "Port",
                &["mcp", "server", "localhost"],
            ),
            Self::McpUrl => info(
                "ai.mcp-url",
                G::McpServer,
                "URL",
                &["mcp", "server", "address"],
            ),
            Self::McpToken => info(
                "ai.mcp-token",
                G::McpServer,
                "Access token",
                &["mcp", "secret", "password", "auth"],
            ),
            Self::McpReadCollections => info(
                "ai.mcp-read-collections",
                G::McpPermissions,
                "Read collections and environments",
                &["mcp", "permission", "access"],
            ),
            Self::McpSendRequests => info(
                "ai.mcp-send-requests",
                G::McpPermissions,
                "Send requests",
                &["mcp", "permission"],
            ),
            Self::McpAskBeforeSending => info(
                "ai.mcp-ask-before-sending",
                G::McpPermissions,
                "Ask before sending",
                &["mcp", "confirm", "permission"],
            ),
            Self::McpEditCollections => info(
                "ai.mcp-edit-collections",
                G::McpPermissions,
                "Edit collections",
                &["mcp", "permission", "write"],
            ),
            Self::McpReadHistory => info(
                "ai.mcp-read-history",
                G::McpPermissions,
                "Read history",
                &["mcp", "permission"],
            ),
            Self::McpClientConfig => info(
                "ai.mcp-client-config",
                G::McpClientSetup,
                "Client configuration",
                &["mcp", "json", "claude code", "setup", "snippet"],
            ),
            Self::Shortcut(shortcut) => info(
                shortcut.id(),
                shortcut.group(),
                shortcut.label(),
                SHORTCUT_KEYWORDS,
            ),
        }
    }

    /// Stable, e.g. "requests.timeout".
    pub fn id(self) -> &'static str {
        self.info().id
    }

    /// The row's label.
    pub fn label(self) -> &'static str {
        self.info().label
    }

    /// Other words people search for it by.
    pub fn keywords(self) -> &'static [&'static str] {
        self.info().keywords
    }

    pub fn group(self) -> SettingsGroup {
        self.info().group
    }

    pub fn section(self) -> SettingsSection {
        self.group().section()
    }

    /// Where the row is, e.g. "Requests › Connection"; the group is left
    /// out when it repeats the page's title.
    pub fn location(self) -> String {
        let section = self.section().title();
        let group = self.group().title();
        if section == group {
            section.to_string()
        } else {
            format!("{section} › {group}")
        }
    }
}

const SHORTCUT_KEYWORDS: &[&str] = &["shortcut", "keyboard", "keybinding", "hotkey"];

/// A row of Settings › Shortcuts, named after the action it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shortcut {
    NewRequest,
    Save,
    SaveAs,
    SendRequest,
    FormatJsonBody,
    Back,
    Forward,
    CommandPalette,
    QuickOpen,
    FocusUrl,
    ToggleSidebar,
    Library,
    History,
    Tests,
    SendHistoryEntry,
    DeleteHistoryEntry,
    Settings,
    Hide,
    HideOthers,
    Quit,
}

impl Shortcut {
    /// In page order, including ones for other platforms.
    pub const ALL: [Self; 20] = [
        Self::NewRequest,
        Self::Save,
        Self::SaveAs,
        Self::SendRequest,
        Self::FormatJsonBody,
        Self::Back,
        Self::Forward,
        Self::CommandPalette,
        Self::QuickOpen,
        Self::FocusUrl,
        Self::ToggleSidebar,
        Self::Library,
        Self::History,
        Self::Tests,
        Self::SendHistoryEntry,
        Self::DeleteHistoryEntry,
        Self::Settings,
        Self::Hide,
        Self::HideOthers,
        Self::Quit,
    ];

    /// The ones this platform has, in page order.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(|s| s.on_this_platform())
    }

    fn on_this_platform(self) -> bool {
        !matches!(self, Self::Hide | Self::HideOthers) || cfg!(target_os = "macos")
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NewRequest => "New request",
            Self::Save => "Save",
            Self::SaveAs => "Save as",
            Self::SendRequest => "Send request",
            Self::FormatJsonBody => "Format JSON body",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::CommandPalette => "Command palette",
            Self::QuickOpen => "Quick open",
            Self::FocusUrl => "Focus URL",
            Self::ToggleSidebar => "Toggle sidebar",
            Self::Library => "Library",
            Self::History => "History",
            Self::Tests => "Tests",
            Self::SendHistoryEntry => "Send history entry again (in History)",
            Self::DeleteHistoryEntry => "Delete history entry",
            Self::Settings => "Settings",
            Self::Hide => "Hide",
            Self::HideOthers => "Hide others",
            Self::Quit => "Quit",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::NewRequest => "shortcuts.new-request",
            Self::Save => "shortcuts.save",
            Self::SaveAs => "shortcuts.save-as",
            Self::SendRequest => "shortcuts.send-request",
            Self::FormatJsonBody => "shortcuts.format-json-body",
            Self::Back => "shortcuts.back",
            Self::Forward => "shortcuts.forward",
            Self::CommandPalette => "shortcuts.command-palette",
            Self::QuickOpen => "shortcuts.quick-open",
            Self::FocusUrl => "shortcuts.focus-url",
            Self::ToggleSidebar => "shortcuts.toggle-sidebar",
            Self::Library => "shortcuts.library",
            Self::History => "shortcuts.history",
            Self::Tests => "shortcuts.tests",
            Self::SendHistoryEntry => "shortcuts.send-history-entry",
            Self::DeleteHistoryEntry => "shortcuts.delete-history-entry",
            Self::Settings => "shortcuts.settings",
            Self::Hide => "shortcuts.hide",
            Self::HideOthers => "shortcuts.hide-others",
            Self::Quit => "shortcuts.quit",
        }
    }

    pub fn group(self) -> SettingsGroup {
        use SettingsGroup as G;
        match self {
            Self::NewRequest
            | Self::Save
            | Self::SaveAs
            | Self::SendRequest
            | Self::FormatJsonBody => G::ShortcutsRequests,
            Self::Back
            | Self::Forward
            | Self::CommandPalette
            | Self::QuickOpen
            | Self::FocusUrl => G::ShortcutsNavigation,
            Self::ToggleSidebar
            | Self::Library
            | Self::History
            | Self::Tests
            | Self::SendHistoryEntry
            | Self::DeleteHistoryEntry => G::ShortcutsPanels,
            Self::Settings | Self::Hide | Self::HideOthers | Self::Quit => G::ShortcutsApp,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn opens_on_general() {
        assert_eq!(SettingsSection::default(), SettingsSection::General);
        assert_eq!(SettingsSection::ALL[0], SettingsSection::General);
    }

    #[test]
    fn menu_order_matches_the_titles() {
        let titles: Vec<_> = SettingsSection::ALL.iter().map(|s| s.title()).collect();
        assert_eq!(
            titles,
            [
                "General",
                "Appearance",
                "Requests",
                "History & data",
                "AI",
                "Shortcuts",
                "About"
            ]
        );
    }

    #[test]
    fn neighbor_stops_at_the_ends() {
        use SettingsSection::*;
        assert_eq!(General.neighbor(false), General);
        assert_eq!(General.neighbor(true), Appearance);
        assert_eq!(About.neighbor(true), About);
        assert_eq!(About.neighbor(false), Shortcuts);
    }

    #[test]
    fn ids_are_unique() {
        let all: Vec<Setting> = Setting::all().collect();
        let ids: HashSet<_> = all.iter().map(|s| s.id()).collect();
        assert_eq!(ids.len(), all.len());
        let shortcut_ids: HashSet<_> = Shortcut::ALL.iter().map(|s| s.id()).collect();
        assert_eq!(shortcut_ids.len(), Shortcut::ALL.len());
    }

    #[test]
    fn ids_start_with_their_section() {
        for setting in Setting::all() {
            let prefix = format!("{}.", setting.section().key());
            assert!(setting.id().starts_with(&prefix), "{setting:?}");
        }
    }

    #[test]
    fn rows_list_every_setting_once() {
        let rows: HashSet<_> = Setting::ROWS.into_iter().collect();
        assert_eq!(rows.len(), Setting::ROWS.len());
        assert!(
            !Setting::ROWS
                .iter()
                .any(|s| matches!(s, Setting::Shortcut(_)))
        );
    }

    #[test]
    fn every_page_but_about_has_settings() {
        for section in SettingsSection::ALL {
            let count = Setting::all().filter(|s| s.section() == section).count();
            if section == SettingsSection::About {
                assert_eq!(count, 0);
            } else {
                assert!(count > 0, "{section:?} has no settings");
            }
        }
    }

    #[test]
    fn rows_are_in_page_order() {
        let sections: Vec<usize> = Setting::all().map(|s| s.section().index()).collect();
        assert!(sections.is_sorted());
    }

    #[test]
    fn every_group_but_credits_has_settings() {
        for group in SettingsGroup::ALL {
            let used = Setting::all().any(|s| s.group() == group);
            assert_eq!(used, group != SettingsGroup::Credits, "{group:?}");
        }
    }

    #[test]
    fn group_ids_are_unique() {
        let ids: HashSet<_> = SettingsGroup::ALL.iter().map(|g| g.id()).collect();
        assert_eq!(ids.len(), SettingsGroup::ALL.len());
    }

    #[test]
    fn keywords_are_lowercase_and_not_words_of_the_label() {
        let pages = SettingsSection::ALL
            .into_iter()
            .map(|s| (s.title(), s.keywords()));
        let rows = Setting::all().map(|s| (s.label(), s.keywords()));
        for (label, keywords) in pages.chain(rows) {
            let label = label.to_lowercase();
            for keyword in keywords {
                assert_eq!(*keyword, keyword.to_lowercase(), "{label}");
                let repeated = label
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|word| word == *keyword);
                assert!(!repeated, "{label} repeats {keyword:?}");
            }
        }
    }

    #[test]
    fn location_names_the_page_and_group() {
        assert_eq!(Setting::Timeout.location(), "Requests › Connection");
        assert_eq!(Setting::AiEnabled.location(), "AI");
        assert_eq!(
            Setting::Shortcut(Shortcut::Save).location(),
            "Shortcuts › Requests"
        );
    }

    #[test]
    fn hide_shortcuts_are_mac_only() {
        let hide = Shortcut::all().any(|s| s == Shortcut::Hide);
        assert_eq!(hide, cfg!(target_os = "macos"));
    }
}
