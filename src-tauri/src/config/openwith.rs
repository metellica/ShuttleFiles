//! Which program opens text files.
//!
//! Windows associates one handler per extension, which means a user who
//! edits `.json` in one editor and `.rs` in another has to fix the
//! association file type by file type. The app keeps its own answer: one
//! program for the whole family of text extensions, applied to every
//! way of opening a file inside ShuttleFiles.

use serde::{Deserialize, Serialize};

use crate::config::{read_json, write_json};
use crate::error::{AppError, AppResult};

const FILE: &str = "open-with.json";

/// Extensions treated as text until the user edits the list. Deliberately
/// broad: anything a plain-text editor can display belongs here.
pub const DEFAULT_TEXT_EXTENSIONS: &[&str] = &[
    "txt", "text", "log", "md", "markdown", "rst", "adoc", "org", "tex", "csv", "tsv", "json",
    "jsonc", "json5", "yaml", "yml", "toml", "ini", "cfg", "conf", "properties", "env", "xml",
    "svg", "html", "htm", "css", "scss", "sass", "less", "js", "jsx", "mjs", "cjs", "ts", "tsx",
    "mts", "cts", "vue", "svelte", "py", "pyi", "rb", "go", "rs", "java", "kt", "kts", "c", "h",
    "cpp", "cc", "cxx", "hpp", "hxx", "cs", "php", "swift", "m", "mm", "scala", "groovy", "lua",
    "pl", "pm", "r", "dart", "sh", "bash", "zsh", "fish", "ps1", "psm1", "bat", "cmd", "sql",
    "graphql", "gql", "proto", "tf", "tfvars", "hcl", "gradle", "cmake", "mk", "diff", "patch",
    "srt", "vtt", "asm", "s", "v", "sv", "vhd", "editorconfig", "gitignore", "gitattributes",
    "dockerfile", "makefile", "lock",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenWithSettings {
    /// Program that opens text files. A full path, or a bare name Windows
    /// can resolve (`code`, `notepad`). Empty means the system default,
    /// which is also the fallback for every non-text file.
    #[serde(default)]
    pub text_editor: String,
    /// Lowercase extensions, without the leading dot.
    #[serde(default = "default_extensions")]
    pub text_extensions: Vec<String>,
}

fn default_extensions() -> Vec<String> {
    DEFAULT_TEXT_EXTENSIONS
        .iter()
        .map(|e| (*e).to_string())
        .collect()
}

impl Default for OpenWithSettings {
    fn default() -> Self {
        Self {
            text_editor: String::new(),
            text_extensions: default_extensions(),
        }
    }
}

impl OpenWithSettings {
    /// Lowercases, strips the dot users naturally type, and drops blanks
    /// and duplicates, so lookups can be a plain string comparison.
    fn normalise(mut self) -> Self {
        self.text_editor = self.text_editor.trim().to_string();

        let mut seen = Vec::with_capacity(self.text_extensions.len());
        for ext in self.text_extensions {
            let ext = ext.trim().trim_start_matches('.').to_lowercase();
            if !ext.is_empty() && !seen.contains(&ext) {
                seen.push(ext);
            }
        }
        self.text_extensions = seen;
        self
    }

    /// A program that names a file must actually be there; catching the
    /// typo here beats every later open failing silently.
    fn validate(&self) -> AppResult<()> {
        let editor = &self.text_editor;
        if editor.is_empty() || !editor.contains(['\\', '/']) {
            return Ok(());
        }
        let path = std::path::Path::new(editor);
        if path.is_file() {
            Ok(())
        } else {
            Err(AppError::Config(format!("No such program: {}", editor)))
        }
    }
}

pub fn load() -> AppResult<OpenWithSettings> {
    Ok(read_json::<OpenWithSettings>(FILE)?.normalise())
}

/// Returns the stored form, so the caller keeps exactly what was saved.
pub fn save(settings: OpenWithSettings) -> AppResult<OpenWithSettings> {
    let settings = settings.normalise();
    settings.validate()?;
    write_json(FILE, &settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_cleans_extensions() {
        let settings = OpenWithSettings {
            text_editor: "  notepad  ".into(),
            text_extensions: vec![".TXT".into(), "txt".into(), " md ".into(), "".into()],
        }
        .normalise();

        assert_eq!(settings.text_editor, "notepad");
        assert_eq!(settings.text_extensions, vec!["txt", "md"]);
    }

    #[test]
    fn bare_program_names_are_not_path_checked() {
        let settings = OpenWithSettings {
            text_editor: "code".into(),
            ..Default::default()
        };
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn missing_program_file_is_rejected() {
        let settings = OpenWithSettings {
            text_editor: "C:\\nope\\missing-editor.exe".into(),
            ..Default::default()
        };
        assert!(settings.validate().is_err());
    }
}
