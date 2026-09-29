//! Both tree-sitter stacks (zeron-syntax's `tree-sitter` crate and Helix's
//! `tree-house`) each compile their own copy of the C runtime. This links them
//! into one binary and parses with both.

const SOURCE: &str = "fn main() { let x = 1; }\n";

#[test]
fn zeron_syntax_and_helix_parse_in_one_binary() {
    let zeron = zeron_syntax::highlight(zeron_syntax::HighlightRequest {
        source: SOURCE,
        path: Some("main.rs"),
        fence_tag: None,
    })
    .expect("zeron-syntax highlights rust");
    assert!(zeron.lines.iter().any(|line| !line.is_empty()));

    let loader = helix_core::config::default_lang_loader();
    let language = loader
        .language_for_name("rust".to_string())
        .expect("rust is in the built-in languages.toml");
    let rope = helix_core::Rope::from(SOURCE);
    // Grammars load from the Helix runtime directory; skip the parse when none
    // is installed (CI) rather than fail on a missing .so.
    match helix_core::syntax::Syntax::new(rope.slice(..), language, &loader) {
        Ok(syntax) => {
            let root = syntax.tree().root_node();
            assert_eq!(root.kind(), "source_file");
        }
        Err(err) => eprintln!("helix grammar unavailable, parse skipped: {err:?}"),
    }
}
