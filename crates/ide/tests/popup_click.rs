//! A docs popup drawn by the host closes when the user clicks elsewhere,
//! instead of following the cursor to the click. (Its own test binary:
//! Helix allows one `Application` per process.)

use std::time::{Duration, Instant};

use helix_view::input::{MouseButton, MouseEvent, MouseEventKind};
use helix_view::keyboard::KeyModifiers;
use zeron_ide::{
    dirs::IdeDirs,
    host::{Event, HelixHost, HostOptions},
};

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_click_elsewhere_dismisses_a_docs_popup() {
    let data = tempfile::tempdir().unwrap();
    IdeDirs::for_data_dir(data.path()).install();
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("notes.txt");
    std::fs::write(&file, "one\ntwo\nthree\nfour\nfive\n").unwrap();

    let host = HelixHost::spawn(HostOptions {
        workspace: workspace.path().to_path_buf(),
        files: vec![file],
        config: Default::default(),
        host_config: None,
        on_frame: Box::new(|| {}),
        on_exit: Box::new(|_| {}),
    })
    .unwrap();
    host.send(Event::Resize(80, 24));

    host.call(|app| {
        let markdown = helix_term::ui::Markdown::new(
            "Docs for `one`.".to_string(),
            app.editor.syn_loader.clone(),
        );
        app.compositor
            .push(Box::new(helix_term::ui::Popup::new("hover", markdown)));
    });
    let mut docs = 0;
    wait_until("the popup to show", || {
        if let Some(frame) = host.take_frame() {
            docs = frame.docs.len();
        }
        docs == 1
    });

    // A click on the fourth line, clear of the popup.
    host.send(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }));
    host.send(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 2,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }));
    wait_until("the popup to close", || {
        if let Some(frame) = host.take_frame() {
            docs = frame.docs.len();
        }
        docs == 0
    });
}
