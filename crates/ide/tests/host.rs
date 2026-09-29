//! Drives a real headless Helix through the host channels: resize, type in
//! insert mode, `:w`, and read the file back.

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

#[test]
fn edits_and_saves_a_file_through_the_host() {
    let data = tempfile::tempdir().unwrap();
    IdeDirs::for_data_dir(data.path()).install();

    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("notes.txt");
    std::fs::write(&file, "world\n").unwrap();

    let (exit_tx, exit_rx) = std::sync::mpsc::channel();
    let host = HelixHost::spawn(HostOptions {
        workspace: workspace.path().to_path_buf(),
        files: vec![file.clone()],
        config: Default::default(),
        host_config: None,
        on_frame: Box::new(|| {}),
        on_exit: Box::new(move |err| {
            let _ = exit_tx.send(err.map(|err| err.to_string()));
        }),
    })
    .unwrap();

    host.send(Event::Resize(80, 24));
    let mut last = None;
    wait_until("first frame", || {
        last = host.take_frame().or(last.take());
        last.is_some()
    });
    let frame = last.unwrap();
    assert_eq!(frame.buffer.area.width, 80);
    let screen: String = frame.buffer.content.iter().map(|c| c.symbol.as_str()).collect();
    assert!(screen.contains("world"), "document not painted");

    for k in ["i", "h", "e", "l", "l", "o", "space", "esc", ":", "w", "ret"] {
        host.send(key(k));
    }
    wait_until("file written", || {
        std::fs::read_to_string(&file).unwrap() == "hello world\n"
    });

    // `:q` on the last view keeps Helix alive with a scratch buffer: Helix
    // state is process-global, so a host never lets its Application exit.
    host.send(key(":"));
    host.send(key("q"));
    host.send(key("ret"));
    let mut screen = String::new();
    wait_until("scratch buffer", || {
        if let Some(frame) = host.take_frame() {
            screen = frame.buffer.content.iter().map(|c| c.symbol.as_str()).collect();
        }
        screen.contains("[scratch]")
    });
    assert!(exit_rx.try_recv().is_err(), "helix exited on :q");
    // Dropping the host ends the loop; the exit hook reports a clean close.
    drop(host);
    let exit = exit_rx.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_eq!(exit, None);
    // The process-wide cwd is the host's, not the workspace Helix opened.
    assert_ne!(std::env::current_dir().unwrap(), workspace.path());
}
