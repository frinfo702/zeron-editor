//! Scripted key tours rendered offscreen, shared by the IDE fixtures.
//!
//! ZERON_IDE_SHOTS=<dir> enables the tour; ZERON_IDE_TOUR holds
//! space-separated gpui keystrokes with `|` between shots. shot-0.png is the
//! untouched window. The app quits when the tour ends.

/// Start the tour if ZERON_IDE_SHOTS is set.
pub fn run_from_env(window: gpui::AnyWindowHandle, cx: &mut gpui::App) {
    let Some(dir) = std::env::var_os("ZERON_IDE_SHOTS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    cx.spawn(async move |cx| {
        if let Err(err) = tour(window, &dir, cx).await {
            eprintln!("tour failed: {err:#}");
        }
        let _ = cx.update(|cx| cx.quit());
    })
    .detach();
}

/// Scripted keys (`ZERON_IDE_TOUR`, space-separated gpui keystrokes, `|`
/// between shots) with a PNG after each group: shot-0.png is the untouched
/// editor.
async fn tour(
    window: gpui::AnyWindowHandle,
    dir: &std::path::Path,
    cx: &mut gpui::AsyncApp,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let script = std::env::var("ZERON_IDE_TOUR").unwrap_or_default();
    let groups: Vec<&str> = std::iter::once("").chain(script.split('|')).collect();
    for (ix, group) in groups.iter().enumerate() {
        for key in group.split_whitespace() {
            window.update(cx, |_, w, cx| {
                let keystroke = gpui::Keystroke::parse(key).unwrap();
                w.dispatch_event(
                    gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                        keystroke,
                        is_held: false,
                        prefer_character_input: false,
                    }),
                    cx,
                );
            })?;
            cx.background_executor()
                .timer(std::time::Duration::from_millis(40))
                .await;
        }
        // Let Helix render and the frame reach the view.
        cx.background_executor()
            .timer(std::time::Duration::from_millis(if ix == 0 { 2500 } else { 700 }))
            .await;
        window.update(cx, |_, w, cx| -> anyhow::Result<()> {
            w.refresh();
            w.draw(cx).clear();
            w.render_to_image()?.save(dir.join(format!("shot-{ix}.png")))?;
            Ok(())
        })??;
    }
    Ok(())
}
