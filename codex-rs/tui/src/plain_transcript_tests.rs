use super::*;

#[test]
fn transcript_is_append_only_plain_text_with_resume_metadata() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("nested").join("session.txt");

    {
        let mut transcript = PlainTranscript::open(&path)?;
        transcript.begin_session("thread-123", Path::new("/workspace"))?;
        transcript.append(PlainTranscriptRole::User, "hello\u{1b}[31m red\u{1b}[0m")?;
        transcript.append(PlainTranscriptRole::CodexFinal, "answer")?;
    }
    {
        let mut transcript = PlainTranscript::open(&path)?;
        transcript.begin_session("thread-456", Path::new("/workspace"))?;
        transcript.append(PlainTranscriptRole::CodexCommentary, "continued")?;
    }

    let text = std::fs::read_to_string(&path)?;
    assert!(text.contains("thread_id: thread-123"));
    assert!(text.contains("resume_in_current_tui: /resume thread-123"));
    assert!(text.contains("resume_in_new_process: codex resume thread-123"));
    assert!(text.contains("hello red"));
    assert!(text.contains("CODEX FINAL"));
    assert!(text.contains("thread_id: thread-456"));
    assert!(text.contains("continued"));
    assert!(!text.contains('\u{1b}'));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
    }

    Ok(())
}

#[cfg(unix)]
#[test]
fn transcript_rejects_symlink_target() -> io::Result<()> {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir()?;
    let target = temp.path().join("target.txt");
    let link = temp.path().join("transcript.txt");
    std::fs::write(&target, "must remain untouched")?;
    symlink(&target, &link)?;

    assert!(PlainTranscript::open(&link).is_err());
    assert_eq!(std::fs::read_to_string(target)?, "must remain untouched");
    Ok(())
}

#[cfg(unix)]
#[test]
fn transcript_rejects_hardlinked_file() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let target = temp.path().join("target.txt");
    let link = temp.path().join("transcript.txt");
    std::fs::write(&target, "must remain untouched")?;
    std::fs::hard_link(&target, &link)?;

    let error = PlainTranscript::open(&link)
        .err()
        .ok_or_else(|| io::Error::other("hardlinked transcript was accepted"))?;
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(std::fs::read_to_string(target)?, "must remain untouched");
    Ok(())
}

#[test]
fn transcript_path_display_replaces_terminal_controls() {
    assert_eq!(
        safe_display_path(Path::new("/tmp/transcript\n\u{1b}.txt")),
        "/tmp/transcript\u{fffd}\u{fffd}.txt"
    );
}

#[test]
fn transcript_removes_c0_del_and_c1_controls() {
    assert_eq!(sanitize_plain_text("a\0b\u{7f}c\u{9b}d\n\t"), "abcd\n\t");
}

#[test]
fn transcript_handles_use_an_exclusive_cross_process_lock() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("shared.txt");
    let first = PlainTranscript::open(&path)?;
    let second = PlainTranscript::open(&path)?;

    first.file.lock()?;
    assert!(second.file.try_lock().is_err());
    first.file.unlock()?;
    second.file.try_lock().map_err(io::Error::other)?;
    second.file.unlock()?;
    Ok(())
}
