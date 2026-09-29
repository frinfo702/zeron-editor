//! Standard mode's selection rules against a real Helix. Own test binary:
//! one Helix Application per process.

use std::time::{Duration, Instant};

use zeron_ide::{
    dirs::IdeDirs,
    host::{Event, HelixHost, HostOptions},
    keymap::build_config,
    standard,
};
use zeron_ui::ide::{IdeKeymap, IdeSettings};

fn text(host: &HelixHost) -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    host.call(move |app| {
        let doc = app.editor.documents.values().next().unwrap();
        let _ = tx.send(doc.text().to_string());
    });
    rx.recv_timeout(Duration::from_secs(20)).unwrap()
}

#[test]
fn typing_and_backspace_replace_the_selection() {
    let data = tempfile::tempdir().unwrap();
    IdeDirs::for_data_dir(data.path()).install();
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("a.txt");
    std::fs::write(&file, "hello world\n").unwrap();

    let settings = IdeSettings {
        keymap: IdeKeymap::Standard,
        ..IdeSettings::default()
    };
    let host = HelixHost::spawn(HostOptions {
        workspace: workspace.path().to_path_buf(),
        files: vec![file],
        config: build_config(&settings, None, None).unwrap(),
        host_config: None,
        on_frame: Box::new(|| {}),
        on_exit: Box::new(|_| {}),
    })
    .unwrap();
    host.send(Event::Resize(80, 24));
    host.call(|app| app.editor.mode = helix_view::document::Mode::Insert);
    let key = |source: &str| {
        let key = source.parse().unwrap();
        host.call(move |app| standard::apply_key(app, key));
    };

    // Shift+Right ×5 selects "hello"; typing replaces it.
    for _ in 0..5 {
        key("S-right");
    }
    key("H");
    key("i");
    let deadline = Instant::now() + Duration::from_secs(20);
    while text(&host) != "Hi world\n" {
        assert!(Instant::now() < deadline, "got {:?}", text(&host));
    }

    // With a selection, Backspace removes just the selection.
    key("S-right");
    key("S-right");
    key("S-right");
    key("backspace");
    assert_eq!(text(&host), "Hirld\n");
    // Without one, it deletes a character as usual.
    key("backspace");
    assert_eq!(text(&host), "Hrld\n");
}
