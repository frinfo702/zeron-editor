//! Helix state is process-global, so each Application gets its own test
//! binary; this one checks the host's config layering survives a reload.

use std::time::{Duration, Instant};

use zeron_ide::{
    dirs::IdeDirs,
    host::{Event, HelixHost, HostOptions},
};

fn key(source: &str) -> Event {
    Event::Key(source.parse().unwrap())
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `:config-reload` must rebuild through the host's layering: a key the host
/// layer binds (here `Cmd-s`, as Zeron's GUI layer does) survives the reload,
/// and so does the host theme.
#[test]
fn config_reload_keeps_host_layers() {
    use helix_term::application::headless::HostConfig;
    use zeron_ide::keymap::build_config;
    use zeron_ui::ide::IdeSettings;

    let data = tempfile::tempdir().unwrap();
    IdeDirs::for_data_dir(data.path()).install();
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("a.txt");
    std::fs::write(&file, "one\n").unwrap();

    let settings = IdeSettings::default();
    let config = build_config(&settings, None, None).unwrap();
    let (exit_tx, exit_rx) = std::sync::mpsc::channel();
    let host = HelixHost::spawn(HostOptions {
        workspace: workspace.path().to_path_buf(),
        files: vec![file.clone()],
        config,
        host_config: Some(HostConfig {
            load: Box::new(move || build_config(&settings, None, None)),
            default_theme: Box::new(zeron_ide::theme::helix_theme),
        }),
        on_frame: Box::new(|| {}),
        on_exit: Box::new(move |err| {
            let _ = exit_tx.send(err.map(|err| err.to_string()));
        }),
    })
    .unwrap();
    host.send(Event::Resize(80, 24));
    for k in [
        ":", "c", "o", "n", "f", "i", "g", "-", "r", "e", "l", "o", "a", "d", "ret",
    ] {
        host.send(key(k));
    }
    for k in ["i", "t", "w", "o", " ", "esc", "Cmd-s"] {
        host.send(key(if k == " " { "space" } else { k }));
    }
    wait_until("Cmd-s save after reload", || {
        std::fs::read_to_string(&file).unwrap() == "two one\n"
    });
    host.call(|app| {
        let theme = app.editor.theme.get("ui.selection");
        assert_eq!(
            theme.bg,
            Some(helix_view::graphics::Color::Indexed(
                zeron_ide::theme::Token::Selection.index()
            ))
        );
        app.editor.set_status("theme ok");
    });
    drop(host);
    assert_eq!(exit_rx.recv_timeout(Duration::from_secs(20)).unwrap(), None);
}
