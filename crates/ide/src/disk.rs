//! Picking up edits made on disk while a buffer is open.
//!
//! Helix does not watch files: an agent editing a file in the background
//! leaves the IDE showing the old text. The view calls [`sync`] on the Helix
//! thread every so often while IDE mode is visible. A buffer without unsaved
//! changes reloads to the new content; one with unsaved changes keeps them
//! and gets a warning. Helix itself already refuses a plain `:w` over a newer
//! file.

use std::{collections::HashMap, path::PathBuf, time::SystemTime};

use helix_term::application::Application;
use helix_view::{DocumentId, ViewId};

/// Per-buffer memory of the disk versions already reported as conflicts, so
/// each external edit warns once.
#[derive(Default)]
pub struct DiskSync {
    warned: HashMap<PathBuf, SystemTime>,
}

/// What [`sync`] did, for tests and logs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub reloaded: Vec<PathBuf>,
    pub conflicted: Vec<PathBuf>,
}

pub fn sync(app: &mut Application, state: &mut DiskSync) -> SyncReport {
    let mut report = SyncReport::default();
    let changed: Vec<(DocumentId, PathBuf, SystemTime, bool)> = app
        .editor
        .documents()
        .filter_map(|doc| {
            let path = doc.path()?.clone();
            let mtime = std::fs::metadata(&path).ok()?.modified().ok()?;
            (mtime > doc.last_saved_time()).then(|| (doc.id(), path, mtime, doc.is_modified()))
        })
        .collect();
    if changed.is_empty() {
        state.warned.clear();
        return report;
    }

    let focused: ViewId = app.editor.tree.focus;
    let scrolloff = app.editor.config().scrolloff;
    for (doc_id, path, mtime, modified) in changed {
        if modified {
            if state.warned.get(&path) != Some(&mtime) {
                state.warned.insert(path.clone(), mtime);
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                app.editor.set_warning(format!(
                    "{name} changed on disk: :reload takes it (dropping your edits), :w! keeps yours"
                ));
                report.conflicted.push(path);
            }
            continue;
        }
        // Reload through a view that shows the document (as `:reload-all`).
        let view_id = {
            let doc = app.editor.documents.get_mut(&doc_id).unwrap();
            let view_id = doc.selections().keys().next().copied().unwrap_or(focused);
            doc.ensure_view_init(view_id);
            view_id
        };
        let view = app.editor.tree.get_mut(view_id);
        let doc = app.editor.documents.get_mut(&doc_id).unwrap();
        view.sync_changes(doc);
        if let Err(err) = doc.reload(view, &app.editor.diff_providers) {
            app.editor.set_error(format!("{}: {err}", path.display()));
            continue;
        }
        if view.doc == doc_id {
            view.ensure_cursor_in_view(doc, scrolloff);
        }
        app.editor
            .language_servers
            .file_event_handler
            .file_changed(path.clone());
        state.warned.remove(&path);
        report.reloaded.push(path);
    }
    report
}
