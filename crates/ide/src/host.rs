//! The Helix thread.
//!
//! Helix runs its own tokio event loop (LSP, jobs, idle timers) and blocks on
//! it in places (`block_in_place` on save/quit), so it gets a dedicated thread
//! and runtime rather than sharing gpui's main thread. The UI talks to it over
//! two narrow channels:
//!
//! - **input** — [`Input`]s: key/mouse/paste/resize events, or a closure run
//!   against the [`Application`];
//! - **frames** — after every Helix render the host snapshots the result into
//!   a [`Frame`] and parks it in a latest-wins slot, then pings the UI. The UI
//!   paints whatever frame is newest, so a slow paint never queues frames.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use anyhow::Context as _;
use helix_term::{
    application::{
        Application,
        headless::{Frame as HelixFrame, HostConfig, Input},
    },
    args::Args,
    config::Config,
};
use helix_view::{document::Mode, graphics::CursorKind};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tui::buffer::Buffer;

pub use helix_view::input::Event;

/// An owned snapshot of one Helix render, safe to hand across threads.
#[derive(Debug, Clone)]
pub struct Frame {
    pub buffer: Buffer,
    pub cursor: Option<(u16, u16)>,
    pub cursor_kind: CursorKind,
    pub mode: Mode,
}

impl Frame {
    fn capture(frame: &HelixFrame<'_>) -> Self {
        Self {
            buffer: frame.buffer.clone(),
            cursor: frame.cursor,
            cursor_kind: frame.cursor_kind,
            mode: frame.editor.mode(),
        }
    }
}

#[derive(Default)]
struct FrameSlot {
    latest: Mutex<Option<Frame>>,
}

pub struct HostOptions {
    /// Folder Helix treats as its working directory (file picker root, LSP
    /// workspace).
    pub workspace: PathBuf,
    /// Files to open at startup, in order.
    pub files: Vec<PathBuf>,
    pub config: Config,
    /// Rebuilds the config on `:config-reload` and supplies the theme used
    /// while the config names none; `None` keeps Helix's own behavior.
    pub host_config: Option<HostConfig>,
    /// Called on the Helix thread whenever a new frame is ready. Must be cheap
    /// and must not block; it just wakes the UI.
    pub on_frame: Box<dyn Fn() + Send + Sync>,
    /// Called once when the Helix application exits (`:quit`) or fails.
    pub on_exit: Box<dyn FnOnce(Option<anyhow::Error>) + Send>,
}

/// Handle to a running Helix instance. Dropping it closes the input channel,
/// which ends the event loop and lets Helix flush writes and stop language
/// servers on its own thread; the thread is detached, never joined, so the UI
/// does not stall on a slow language server shutdown.
pub struct HelixHost {
    input: mpsc::UnboundedSender<Input>,
    frames: Arc<FrameSlot>,
    _thread: JoinHandle<()>,
}

impl HelixHost {
    pub fn spawn(options: HostOptions) -> anyhow::Result<Self> {
        let (input, input_rx) = mpsc::unbounded_channel();
        let frames = Arc::new(FrameSlot::default());
        let sink_frames = frames.clone();
        let thread = std::thread::Builder::new()
            .name("zeron-helix".into())
            .spawn(move || {
                let HostOptions {
                    workspace,
                    files,
                    config,
                    host_config,
                    on_frame,
                    on_exit,
                } = options;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(workspace, files, config, host_config, input_rx, move |frame| {
                        *sink_frames.latest.lock().unwrap() = Some(Frame::capture(&frame));
                        on_frame();
                    })
                }))
                .unwrap_or_else(|panic| {
                    let message = panic
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".into());
                    Err(anyhow::anyhow!("helix panicked: {message}"))
                });
                on_exit(result.err());
            })
            .context("spawn helix thread")?;
        Ok(Self {
            input,
            frames,
            _thread: thread,
        })
    }

    pub fn send(&self, event: Event) {
        let _ = self.input.send(Input::Event(event));
    }

    /// Run `call` on the Helix thread with the live application; a render
    /// follows automatically.
    pub fn call(&self, call: impl FnOnce(&mut Application) + Send + 'static) {
        let _ = self.input.send(Input::Call(Box::new(call)));
    }

    /// Take the newest frame, if one arrived since the last take.
    pub fn take_frame(&self) -> Option<Frame> {
        self.frames.latest.lock().unwrap().take()
    }
}

fn run(
    workspace: PathBuf,
    files: Vec<PathBuf>,
    config: Config,
    host_config: Option<HostConfig>,
    input: mpsc::UnboundedReceiver<Input>,
    sink: impl FnMut(HelixFrame<'_>) + Send + 'static,
) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("zeron-helix-rt")
        .enable_all()
        .build()
        .context("build helix runtime")?;
    runtime.block_on(async move {
        helix_stdx::env::set_current_working_dir(&workspace)
            .with_context(|| format!("open workspace {}", workspace.display()))?;
        let lang_loader = helix_core::config::user_lang_loader().unwrap_or_else(|err| {
            tracing::warn!("languages.toml: {err}; using the built-in language config");
            helix_core::config::default_lang_loader()
        });
        let mut args = Args::default();
        args.working_directory = Some(workspace);
        for file in files {
            // Helix builds each file's initial selection from these; an empty
            // list is an empty selection, which it asserts against.
            args.files
                .insert(file, vec![helix_core::Position::default()]);
        }
        let themed = config.theme.is_some();
        let mut app = Application::new(args, config, lang_loader).context("start helix")?;
        if let Some(host) = host_config {
            if !themed {
                app.editor.set_theme((host.default_theme)());
            }
            app.set_host_config(host);
        }
        app.set_frame_sink(Box::new(sink));
        let mut input = UnboundedReceiverStream::new(input);
        app.run_headless(&mut input).await;
        for err in app.close().await {
            tracing::warn!("helix shutdown: {err}");
        }
        Ok(())
    })
}
