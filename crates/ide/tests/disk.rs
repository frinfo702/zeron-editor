//! Buffers pick up edits made on disk (an agent's), and keep unsaved work.
//! Own test binary: one Helix Application per process.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use zeron_ide::{
    dirs::IdeDirs,
    disk::{DiskSync, SyncReport},
    host::{Event, HelixHost, HostOptions},
};

fn on_helix<T: Send + 'static>(
    host: &HelixHost,
    f: impl FnOnce(&mut helix_term::application::Application) -> T + Send + 'static,
) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    host.call(move |app| {
        let _ = tx.send(f(app));
    });
    rx.recv_timeout(Duration::from_secs(20)).unwrap()
}

fn text_of(host: &HelixHost, path: PathBuf) -> String {
    on_helix(host, move |app| {
        let doc = app.editor.document_by_path(&path).unwrap();
        doc.text().to_string()
    })
}

#[test]
fn clean_buffers_reload_and_dirty_ones_warn() {
    let data = tempfile::tempdir().unwrap();
    IdeDirs::for_data_dir(data.path()).install();
    let workspace = tempfile::tempdir().unwrap();
    let clean = workspace.path().join("clean.txt");
    let dirty = workspace.path().join("dirty.txt");
    std::fs::write(&clean, "old\n").unwrap();
    std::fs::write(&dirty, "old\n").unwrap();

    let host = HelixHost::spawn(HostOptions {
        workspace: workspace.path().to_path_buf(),
        files: vec![clean.clone(), dirty.clone()],
        config: Default::default(),
        host_config: None,
        on_frame: Box::new(|| {}),
        on_exit: Box::new(|_| {}),
    })
    .unwrap();
    host.send(Event::Resize(80, 24));
    // Focus the second file and make an unsaved edit there.
    let focus = dirty.clone();
    on_helix(&host, move |app| {
        app.editor
            .open(&focus, helix_view::editor::Action::Replace)
            .unwrap();
    });
    for key in ["i", "x", "esc"] {
        host.send(Event::Key(key.parse().unwrap()));
    }
    assert_eq!(text_of(&host, dirty.clone()), "xold\n");

    // An agent rewrites both files.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&clean, "new from agent\n").unwrap();
    std::fs::write(&dirty, "new from agent\n").unwrap();

    let state = Arc::new(Mutex::new(DiskSync::default()));
    let run = |state: &Arc<Mutex<DiskSync>>| -> SyncReport {
        let state = state.clone();
        on_helix(&host, move |app| {
            zeron_ide::disk::sync(app, &mut state.lock().unwrap())
        })
    };
    let report = run(&state);
    assert_eq!(report.reloaded, vec![clean.clone()]);
    assert_eq!(report.conflicted, vec![dirty.clone()]);
    assert_eq!(text_of(&host, clean.clone()), "new from agent\n");
    assert_eq!(text_of(&host, dirty.clone()), "xold\n", "unsaved work kept");

    // Nothing new on disk: no reload, and the conflict warns only once.
    assert_eq!(run(&state), SyncReport::default());
}
