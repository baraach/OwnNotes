// Appearance and behaviour choices, shared by every front end.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SortBy {
    Updated,
    Created,
    Title,
}

impl SortBy {
    pub fn label(self) -> &'static str {
        match self {
            SortBy::Updated => "Last edited",
            SortBy::Created => "Date created",
            SortBy::Title => "Title",
        }
    }
}

/// Appearance and behaviour choices from Settings (saved in settings.json).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemeMode,
    /// Index into `ui::ACCENTS`.
    pub accent: usize,
    pub show_counts: bool,
    pub compact_list: bool,
    pub sort: SortBy,
    pub editor_font_size: f32,
    /// Style Markdown (headings, bold, checklists) while typing.
    pub live_markdown: bool,
    /// Let the editor text use the whole width instead of a readable column.
    pub wide_editor: bool,
    pub show_sidebar: bool,
    /// The Info panel on the right (word counts, table of contents).
    pub show_info: bool,
    /// Typing "/" opens the insert menu.
    pub slash_menu: bool,
    /// The editor's font: "sans", "serif", "mono", "rounded", or the name of
    /// any font installed on this computer.
    pub editor_font: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: ThemeMode::System,
            accent: 0,
            show_counts: true,
            compact_list: false,
            sort: SortBy::Updated,
            editor_font_size: 15.0,
            live_markdown: true,
            wide_editor: false,
            show_sidebar: true,
            show_info: false,
            slash_menu: true,
            editor_font: "sans".to_string(),
        }
    }
}

