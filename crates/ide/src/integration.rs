//! Registration with the Zeron shell ([`zeron_ui::ide`]).

use std::{
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};

use gpui::{AnyView, App, AppContext as _, Entity, FocusHandle, Focusable as _, Window};
use zeron_ui::ide::{
    self, GrammarStatus, IdeConfigFiles, IdeEditor, IdeRequest, IdeServices, IdeSettings,
};

use crate::{
    dirs::IdeDirs,
    view::{EditorOptions, EditorStatus, HelixEditor},
};

/// Install Helix's directory layout under `data_dir` and make IDE mode
/// available to the shell. Call once at boot, before any window opens.
pub fn init(data_dir: &std::path::Path, cx: &mut App) {
    let dirs = IdeDirs::for_data_dir(data_dir);
    dirs.install();
    seed_config_files(&dirs);
    let services = Rc::new(Services {
        dirs: dirs.clone(),
        installing: Arc::new(Mutex::new(None)),
    });
    let factory_dirs = dirs.clone();
    ide::register(
        Rc::new(move |request: IdeRequest, window: &mut Window, cx: &mut App| {
            let dirs = factory_dirs.clone();
            let editor = cx.new(|cx| {
                HelixEditor::new(
                    EditorOptions {
                        dirs,
                        workspace: request.workspace,
                        files: Vec::new(),
                        settings: request.settings,
                    },
                    window,
                    cx,
                )
            });
            Rc::new(Handle(editor)) as Rc<dyn IdeEditor>
        }),
        services,
        cx,
    );
}

struct Handle(Entity<HelixEditor>);

impl IdeEditor for Handle {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.0.focus_handle(cx)
    }

    fn open(&self, path: PathBuf, cx: &mut App) {
        self.0.read(cx).open(path);
    }

    fn apply_settings(&self, settings: &IdeSettings, cx: &mut App) {
        let settings = settings.clone();
        self.0
            .update(cx, |editor, cx| editor.apply_settings(settings, cx));
    }

    fn is_closed(&self, cx: &App) -> bool {
        matches!(self.0.read(cx).status(), EditorStatus::Exited(_))
    }
}

struct Services {
    dirs: IdeDirs,
    /// `Some` while an install runs: `Ok(())` / `Err` once it finished and the
    /// status has not been read since.
    installing: Arc<Mutex<Option<Option<Result<(), String>>>>>,
}

impl IdeServices for Services {
    fn config_files(&self) -> IdeConfigFiles {
        IdeConfigFiles {
            config: self.dirs.config_file(),
            languages: self.dirs.languages_file(),
            dir: self.dirs.config.clone(),
        }
    }

    fn grammar_status(&self) -> GrammarStatus {
        match &*self.installing.lock().unwrap() {
            Some(None) => GrammarStatus::Installing,
            Some(Some(Err(err))) => GrammarStatus::Failed(err.clone()),
            Some(Some(Ok(()))) | None => match count_grammars(&self.dirs.grammars_dir()) {
                0 => GrammarStatus::Unknown,
                n => GrammarStatus::Installed(n),
            },
        }
    }

    fn install_grammars(&self, done: Box<dyn FnOnce(&mut App)>, cx: &mut App) {
        {
            let mut state = self.installing.lock().unwrap();
            if matches!(*state, Some(None)) {
                return;
            }
            *state = Some(None);
        }
        let state = self.installing.clone();
        let task = cx.background_spawn(async move {
            // Both steps shell out to git and a C compiler and take minutes on
            // a cold cache; they report per-grammar failures in the error.
            let result = helix_loader::grammar::fetch_grammars()
                .and_then(|()| helix_loader::grammar::build_grammars(None))
                .map_err(|err| format!("{err:#}"));
            *state.lock().unwrap() = Some(Some(result));
        });
        cx.spawn(async move |cx| {
            task.await;
            let _ = cx.update(|cx| done(cx));
        })
        .detach();
    }
}

fn count_grammars(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    matches!(
                        entry.path().extension().and_then(|ext| ext.to_str()),
                        Some("so" | "dylib" | "dll")
                    )
                })
                .count()
        })
        .unwrap_or(0)
}

/// Create commented starter files so "Open config.toml" always has
/// something to open. Existing files are never touched.
fn seed_config_files(dirs: &IdeDirs) {
    let seed = |path: PathBuf, body: &str| {
        if !path.exists() {
            let _ = std::fs::write(path, body);
        }
    };
    seed(
        dirs.config_file(),
        "# Helix config for Zeron's IDE mode — https://docs.helix-editor.com/configuration.html\n\
         # Settings here override Zeron's Settings → Editor page.\n\
         #\n\
         # theme = \"onedark\"   # unset: follow Zeron's theme\n\
         #\n\
         # [editor]\n\
         # rulers = [100]\n\
         #\n\
         # [keys.normal]\n\
         # C-s = \":write\"\n",
    );
    seed(
        dirs.languages_file(),
        "# Helix languages for Zeron's IDE mode — https://docs.helix-editor.com/languages.html\n\
         # Merged over Helix's built-in languages.toml.\n\
         #\n\
         # [[language]]\n\
         # name = \"rust\"\n\
         # auto-format = true\n",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_starter_files_once() {
        let data = tempfile::tempdir().unwrap();
        let dirs = IdeDirs::for_data_dir(data.path());
        std::fs::create_dir_all(&dirs.config).unwrap();
        seed_config_files(&dirs);
        let config = std::fs::read_to_string(dirs.config_file()).unwrap();
        assert!(config.starts_with("# Helix config"));
        // Seeds are all comments: they parse as empty configs.
        assert!(toml::from_str::<toml::Value>(&config).is_ok());
        let languages = std::fs::read_to_string(dirs.languages_file()).unwrap();
        assert!(toml::from_str::<toml::Value>(&languages).is_ok());

        std::fs::write(dirs.config_file(), "theme = \"onedark\"\n").unwrap();
        seed_config_files(&dirs);
        assert_eq!(
            std::fs::read_to_string(dirs.config_file()).unwrap(),
            "theme = \"onedark\"\n"
        );
    }

    #[test]
    fn counts_compiled_grammars_only() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(count_grammars(dir.path()), 0);
        for name in ["rust.so", "toml.dylib", "sources"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        assert_eq!(count_grammars(dir.path()), 2);
    }
}
