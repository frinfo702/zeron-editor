//! Where IDE mode keeps Helix's files.
//!
//! Helix's own `config.toml` / `languages.toml` formats are used unchanged, but
//! they live under Zeron's data directory rather than `~/.config/helix`, so an
//! existing `hx` setup is never read or rewritten behind the user's back.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeDirs {
    /// Helix config dir: `config.toml`, `languages.toml`, `themes/`,
    /// `runtime/grammars/` (fetched and built on demand).
    pub config: PathBuf,
    /// Helix cache dir (logs, history).
    pub cache: PathBuf,
    /// Read-only runtime dirs that ship with Zeron (queries, themes, tutor),
    /// highest priority first.
    pub runtime: Vec<PathBuf>,
}

impl IdeDirs {
    pub fn for_data_dir(data_dir: &Path) -> Self {
        let root = data_dir.join("ide");
        Self {
            config: root.join("helix"),
            cache: root.join("cache"),
            runtime: bundled_runtime_dirs(),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    pub fn languages_file(&self) -> PathBuf {
        self.config.join("languages.toml")
    }

    pub fn log_file(&self) -> PathBuf {
        self.cache.join("helix.log")
    }

    /// Where grammars are fetched and compiled (`runtime/grammars` under the
    /// writable config dir, which Helix searches first).
    pub fn grammars_dir(&self) -> PathBuf {
        self.config.join("runtime").join("grammars")
    }

    /// Hand the layout to Helix. Process-wide and first-call-wins, like the
    /// Helix globals it sets, so call it once before any Helix path lookup.
    pub fn install(&self) {
        let _ = std::fs::create_dir_all(&self.config);
        let _ = std::fs::create_dir_all(&self.cache);
        helix_loader::initialize_host_dirs(
            self.config.clone(),
            self.cache.clone(),
            self.runtime.clone(),
        );
        helix_loader::initialize_config_file(Some(self.config_file()));
        helix_loader::initialize_log_file(Some(self.log_file()));
        helix_stdx::env::set_host_owns_process_cwd();
    }
}

/// Runtime dirs Zeron ships: next to the executable in a package (macOS
/// `Contents/Resources/helix-runtime`, elsewhere `helix-runtime/` beside the
/// binary), and the vendored tree in a source checkout.
fn bundled_runtime_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::canonicalize(exe).ok())
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        dirs.push(exe_dir.join("../Resources/helix-runtime"));
        dirs.push(exe_dir.join("helix-runtime"));
    }
    let source_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/helix/runtime");
    if source_tree.is_dir() {
        dirs.push(source_tree);
    }
    dirs.retain(|dir| dir.is_dir());
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_lives_under_the_zeron_data_dir() {
        let dirs = IdeDirs::for_data_dir(Path::new("/data"));
        assert_eq!(dirs.config_file(), Path::new("/data/ide/helix/config.toml"));
        assert_eq!(
            dirs.languages_file(),
            Path::new("/data/ide/helix/languages.toml")
        );
        assert_eq!(
            dirs.grammars_dir(),
            Path::new("/data/ide/helix/runtime/grammars")
        );
    }

    #[test]
    fn source_checkout_runtime_is_found() {
        let dirs = bundled_runtime_dirs();
        assert!(
            dirs.iter().any(|dir| dir.join("queries").is_dir()),
            "vendored runtime missing from {dirs:?}"
        );
    }
}
