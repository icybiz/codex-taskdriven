use super::*;

#[test]
fn parses_plain_transcript_path() {
    let cli = Cli::try_parse_from([
        "codex",
        "--plain-transcript",
        "/tmp/codex-plain-transcript.txt",
    ])
    .expect("--plain-transcript should parse");

    assert_eq!(
        cli.plain_transcript.as_deref(),
        Some(std::path::Path::new("/tmp/codex-plain-transcript.txt"))
    );
}
