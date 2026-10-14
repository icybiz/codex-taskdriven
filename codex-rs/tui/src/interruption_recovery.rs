//! Persistent run markers for detecting Codex sessions interrupted by a crash or reboot.

use codex_protocol::ThreadId;
use std::io;
use std::path::Path;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use chrono::Utc;
    use serde::Deserialize;
    use serde::Serialize;
    use std::fs::File;
    use std::fs::OpenOptions;
    use std::io::Read;
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use uuid::Uuid;

    const MARKER_SCHEMA_VERSION: u32 = 1;
    const MARKER_DIRECTORY: &str = "recovery-runs";
    const MAX_MARKER_BYTES: u64 = 64 * 1024;

    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct RunMarker {
        schema_version: u32,
        pid: u32,
        process_start_ticks: u64,
        boot_id: String,
        started_unix_ms: i64,
        updated_unix_ms: i64,
        cwd: PathBuf,
        thread_id: Option<String>,
        codex_version: String,
    }

    #[derive(Clone, Debug)]
    struct RunIdentity {
        pid: u32,
        process_start_ticks: u64,
        boot_id: String,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum InterruptionKind {
        Reboot,
        ProcessEnded,
    }

    #[derive(Clone, Debug)]
    struct InterruptedMarker {
        path: PathBuf,
        marker: RunMarker,
        kind: InterruptionKind,
    }

    /// The marker is deliberately not removed in `Drop`: panics and forced
    /// termination must leave evidence for the next process. Call
    /// `finish_clean` only after a controlled TUI exit.
    pub(crate) struct RecoveryRun {
        directory: Option<PathBuf>,
        marker_path: Option<PathBuf>,
        marker: Option<RunMarker>,
        interrupted: Vec<InterruptedMarker>,
    }

    impl RecoveryRun {
        pub(crate) fn begin(
            codex_home: &Path,
            cwd: &Path,
            startup_resume_thread_id: Option<ThreadId>,
        ) -> io::Result<(Self, Option<String>)> {
            let identity = current_identity()?;
            let directory = codex_home.join(MARKER_DIRECTORY);
            ensure_private_directory(&directory)?;
            let mut interrupted = scan_interrupted_markers(&directory, &identity, &|pid| {
                read_process_start_ticks(pid)
            })?;

            let notice_candidate = select_notice_candidate(&interrupted, cwd);
            let startup_resume_thread_id = startup_resume_thread_id.map(|id| id.to_string());
            let notice = notice_candidate
                .filter(|candidate| {
                    startup_resume_thread_id.as_deref() != candidate.marker.thread_id.as_deref()
                })
                .map(|candidate| recovery_notice(candidate, cwd));

            let now = Utc::now().timestamp_millis();
            let marker = RunMarker {
                schema_version: MARKER_SCHEMA_VERSION,
                pid: identity.pid,
                process_start_ticks: identity.process_start_ticks,
                boot_id: identity.boot_id.clone(),
                started_unix_ms: now,
                updated_unix_ms: now,
                cwd: cwd.to_path_buf(),
                thread_id: None,
                codex_version: env!("CARGO_PKG_VERSION").to_string(),
            };
            let marker_path = directory.join(format!(
                "run-{}-{}-{}.json",
                safe_path_component(&identity.boot_id),
                identity.pid,
                identity.process_start_ticks
            ));
            persist_marker(&directory, &marker_path, &marker)?;

            interrupted.retain(|candidate| candidate.path != marker_path);
            Ok((
                Self {
                    directory: Some(directory),
                    marker_path: Some(marker_path),
                    marker: Some(marker),
                    interrupted,
                },
                notice,
            ))
        }

        pub(crate) fn disabled() -> Self {
            Self {
                directory: None,
                marker_path: None,
                marker: None,
                interrupted: Vec::new(),
            }
        }

        pub(crate) fn update_session(&mut self, thread_id: ThreadId, cwd: &Path) -> io::Result<()> {
            let thread_id = thread_id.to_string();
            let (Some(directory), Some(marker_path), Some(current_marker)) = (
                self.directory.as_ref(),
                self.marker_path.as_ref(),
                self.marker.as_ref(),
            ) else {
                return Ok(());
            };

            let mut marker = current_marker.clone();
            marker.thread_id = Some(thread_id.clone());
            marker.cwd = cwd.to_path_buf();
            marker.updated_unix_ms = Utc::now().timestamp_millis();
            persist_marker(directory, marker_path, &marker)?;
            self.marker = Some(marker);

            let mut removed_any = false;
            self.interrupted.retain(|candidate| {
                let matches = candidate.marker.thread_id.as_deref() == Some(thread_id.as_str());
                if matches {
                    match std::fs::remove_file(&candidate.path) {
                        Ok(()) => removed_any = true,
                        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                path = %candidate.path.display(),
                                "failed to retire resumed interruption marker"
                            );
                            return true;
                        }
                    }
                }
                !matches
            });
            if removed_any {
                sync_directory(directory)?;
            }
            Ok(())
        }

        pub(crate) fn finish_clean(&mut self) -> io::Result<()> {
            let (Some(directory), Some(marker_path)) =
                (self.directory.as_ref(), self.marker_path.take())
            else {
                return Ok(());
            };
            match std::fs::remove_file(&marker_path) {
                Ok(()) => sync_directory(directory)?,
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            self.marker = None;
            Ok(())
        }
    }

    fn current_identity() -> io::Result<RunIdentity> {
        let pid = std::process::id();
        let process_start_ticks = read_process_start_ticks(pid).ok_or_else(|| {
            io::Error::other("could not read this Codex process start time from /proc")
        })?;
        let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
            .trim()
            .to_string();
        if boot_id.is_empty() {
            return Err(io::Error::other("Linux boot ID is empty"));
        }
        Ok(RunIdentity {
            pid,
            process_start_ticks,
            boot_id,
        })
    }

    fn read_process_start_ticks(pid: u32) -> Option<u64> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after_command = stat.rsplit_once(')')?.1;
        after_command.split_whitespace().nth(19)?.parse().ok()
    }

    fn scan_interrupted_markers(
        directory: &Path,
        identity: &RunIdentity,
        process_start_ticks: &dyn Fn(u32) -> Option<u64>,
    ) -> io::Result<Vec<InterruptedMarker>> {
        let mut interrupted = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::warn!(error = %err, "failed to inspect a recovery marker entry");
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let marker = match read_marker(&path) {
                Ok(marker) => marker,
                Err(err) => {
                    tracing::warn!(error = %err, path = %path.display(), "ignored invalid recovery marker");
                    continue;
                }
            };
            if marker.schema_version != MARKER_SCHEMA_VERSION {
                continue;
            }
            if marker_is_active(&marker, identity, process_start_ticks) {
                continue;
            }
            let valid_thread_id = marker
                .thread_id
                .as_deref()
                .is_some_and(|value| Uuid::parse_str(value).is_ok());
            if !valid_thread_id || !marker.cwd.is_absolute() {
                if let Err(err) = std::fs::remove_file(&path)
                    && err.kind() != io::ErrorKind::NotFound
                {
                    tracing::warn!(error = %err, path = %path.display(), "failed to prune unusable recovery marker");
                }
                continue;
            }
            let kind = if marker.boot_id == identity.boot_id {
                InterruptionKind::ProcessEnded
            } else {
                InterruptionKind::Reboot
            };
            interrupted.push(InterruptedMarker { path, marker, kind });
        }
        interrupted.sort_by_key(|candidate| candidate.marker.updated_unix_ms);
        Ok(interrupted)
    }

    fn marker_is_active(
        marker: &RunMarker,
        identity: &RunIdentity,
        process_start_ticks: &dyn Fn(u32) -> Option<u64>,
    ) -> bool {
        marker.boot_id == identity.boot_id
            && process_start_ticks(marker.pid) == Some(marker.process_start_ticks)
    }

    fn select_notice_candidate<'a>(
        interrupted: &'a [InterruptedMarker],
        cwd: &Path,
    ) -> Option<&'a InterruptedMarker> {
        interrupted
            .iter()
            .rev()
            .find(|candidate| candidate.marker.cwd == cwd)
            .or_else(|| {
                (interrupted.len() == 1)
                    .then(|| interrupted.first())
                    .flatten()
            })
    }

    fn recovery_notice(candidate: &InterruptedMarker, current_cwd: &Path) -> String {
        let thread_id = candidate.marker.thread_id.as_deref().unwrap_or_default();
        let reason = match candidate.kind {
            InterruptionKind::Reboot => "system reboot while Codex was running",
            InterruptionKind::ProcessEnded => "Codex exited without a clean shutdown",
        };
        let interrupted_cwd = display_path(&candidate.marker.cwd);
        let cwd_note = if candidate.marker.cwd == current_cwd {
            format!("in working directory `{interrupted_cwd}`")
        } else {
            format!("for working directory `{interrupted_cwd}`")
        };
        format!(
            "Unexpected Codex interruption detected ({reason}) {cwd_note}. Native session `{thread_id}` can be resumed. Enter `/resume {thread_id}` to continue it in this running Codex instance; `/resume` opens the session picker instead."
        )
    }

    fn ensure_private_directory(directory: &Path) -> io::Result<()> {
        match std::fs::symlink_metadata(directory) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(io::Error::other(format!(
                        "recovery path is not a private directory: {}",
                        directory.display()
                    )));
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                std::fs::create_dir_all(directory)?;
            }
            Err(err) => return Err(err),
        }
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        let metadata = std::fs::symlink_metadata(directory)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.uid() != effective_uid()
            || metadata.mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "recovery directory is not private and owned by this user: {}",
                    directory.display()
                ),
            ));
        }
        Ok(())
    }

    fn read_marker(path: &Path) -> io::Result<RunMarker> {
        let metadata = std::fs::symlink_metadata(path)?;
        if !marker_metadata_is_safe(&metadata) {
            return Err(io::Error::other("unsafe recovery marker metadata"));
        }
        let mut options = OpenOptions::new();
        options.read(true).custom_flags(libc::O_NOFOLLOW);
        let file = options.open(path)?;
        let opened_metadata = file.metadata()?;
        if !marker_metadata_is_safe(&opened_metadata)
            || opened_metadata.dev() != metadata.dev()
            || opened_metadata.ino() != metadata.ino()
        {
            return Err(io::Error::other("recovery marker changed while opening"));
        }
        let mut data = String::new();
        file.take(MAX_MARKER_BYTES + 1).read_to_string(&mut data)?;
        if data.len() as u64 > MAX_MARKER_BYTES {
            return Err(io::Error::other("recovery marker exceeds size limit"));
        }
        serde_json::from_str(&data).map_err(io::Error::other)
    }

    fn persist_marker(directory: &Path, path: &Path, marker: &RunMarker) -> io::Result<()> {
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| io::Error::other("invalid recovery marker path"))?;
        let temp_path = directory.join(format!(
            ".{file_name}.tmp-{}-{}",
            std::process::id(),
            Utc::now().timestamp_millis()
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temp_path)?;
            serde_json::to_writer(&mut file, marker).map_err(io::Error::other)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            std::fs::rename(&temp_path, path)?;
            sync_directory(directory)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        result
    }

    fn sync_directory(directory: &Path) -> io::Result<()> {
        File::open(directory)?.sync_all()
    }

    fn safe_path_component(value: &str) -> String {
        value
            .chars()
            .filter(|character| character.is_ascii_hexdigit() || *character == '-')
            .collect()
    }

    fn display_path(path: &Path) -> String {
        path.to_string_lossy()
            .chars()
            .map(|character| {
                if character.is_control() {
                    '\u{fffd}'
                } else {
                    character
                }
            })
            .collect()
    }

    fn marker_metadata_is_safe(metadata: &std::fs::Metadata) -> bool {
        !metadata.file_type().is_symlink()
            && metadata.is_file()
            && metadata.uid() == effective_uid()
            && metadata.mode() & 0o077 == 0
            && metadata.nlink() == 1
            && metadata.len() <= MAX_MARKER_BYTES
    }

    fn effective_uid() -> u32 {
        // SAFETY: `geteuid` has no preconditions and does not dereference pointers.
        unsafe { libc::geteuid() }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn marker(boot_id: &str, pid: u32, start: u64, cwd: &Path) -> RunMarker {
            RunMarker {
                schema_version: MARKER_SCHEMA_VERSION,
                pid,
                process_start_ticks: start,
                boot_id: boot_id.to_string(),
                started_unix_ms: 1,
                updated_unix_ms: 2,
                cwd: cwd.to_path_buf(),
                thread_id: Some("019fc937-e0c0-7013-b8de-8f5721c8f027".to_string()),
                codex_version: "test".to_string(),
            }
        }

        #[test]
        fn active_marker_requires_same_boot_and_exact_process_start() {
            let cwd = Path::new("/workspace");
            let identity = RunIdentity {
                pid: 99,
                process_start_ticks: 900,
                boot_id: "boot-new".to_string(),
            };
            let live = marker("boot-new", 42, 123, cwd);
            assert!(marker_is_active(&live, &identity, &|pid| {
                (pid == 42).then_some(123)
            }));
            let rebooted = marker("boot-old", 42, 123, cwd);
            assert!(!marker_is_active(&rebooted, &identity, &|_| Some(123)));
            assert!(!marker_is_active(&live, &identity, &|_| Some(124)));
        }

        #[test]
        fn notice_contains_exact_in_tui_resume_command() {
            let cwd = Path::new("/workspace");
            let candidate = InterruptedMarker {
                path: PathBuf::from("marker.json"),
                marker: marker("boot-old", 42, 123, cwd),
                kind: InterruptionKind::Reboot,
            };
            let notice = recovery_notice(&candidate, cwd);
            assert!(notice.contains("/resume 019fc937-e0c0-7013-b8de-8f5721c8f027"));
            assert!(notice.contains("system reboot"));
        }

        #[test]
        fn notice_replaces_terminal_control_characters_in_path() {
            let cwd = Path::new("/workspace\n\u{1b}[31m");
            let candidate = InterruptedMarker {
                path: PathBuf::from("marker.json"),
                marker: marker("boot-old", 42, 123, cwd),
                kind: InterruptionKind::Reboot,
            };
            let notice = recovery_notice(&candidate, cwd);
            assert!(!notice.contains('\n'));
            assert!(!notice.contains('\u{1b}'));
            assert!(notice.contains("/workspace\u{fffd}\u{fffd}[31m"));
        }

        #[test]
        fn scan_ignores_live_marker_and_returns_rebooted_marker() -> io::Result<()> {
            let temp = tempfile::tempdir()?;
            let directory = temp.path();
            let cwd = Path::new("/workspace");
            let live_path = directory.join("run-live.json");
            let stale_path = directory.join("run-stale.json");
            persist_marker(directory, &live_path, &marker("boot-new", 42, 123, cwd))?;
            persist_marker(directory, &stale_path, &marker("boot-old", 43, 456, cwd))?;
            let identity = RunIdentity {
                pid: 99,
                process_start_ticks: 900,
                boot_id: "boot-new".to_string(),
            };

            let interrupted =
                scan_interrupted_markers(directory, &identity, &|pid| (pid == 42).then_some(123))?;
            assert_eq!(interrupted.len(), 1);
            assert_eq!(interrupted[0].path, stale_path);
            assert_eq!(interrupted[0].kind, InterruptionKind::Reboot);
            Ok(())
        }

        #[test]
        fn clean_finish_removes_only_current_marker() -> io::Result<()> {
            let temp = tempfile::tempdir()?;
            let current_path = temp.path().join("run-current.json");
            let stale_path = temp.path().join("run-stale.json");
            let cwd = Path::new("/workspace");
            let current = marker("boot-new", 42, 123, cwd);
            persist_marker(temp.path(), &current_path, &current)?;
            persist_marker(temp.path(), &stale_path, &marker("boot-old", 43, 456, cwd))?;
            let mut recovery = RecoveryRun {
                directory: Some(temp.path().to_path_buf()),
                marker_path: Some(current_path.clone()),
                marker: Some(current),
                interrupted: Vec::new(),
            };

            recovery.finish_clean()?;
            assert!(!current_path.exists());
            assert!(stale_path.exists());
            Ok(())
        }

        #[test]
        fn begin_update_and_clean_finish_round_trip() -> io::Result<()> {
            let temp = tempfile::tempdir()?;
            let cwd = Path::new("/workspace");
            let (mut recovery, notice) = RecoveryRun::begin(temp.path(), cwd, None)?;
            assert!(notice.is_none());
            let marker_path = recovery
                .marker_path
                .clone()
                .ok_or_else(|| io::Error::other("begin did not create a marker"))?;
            assert_eq!(std::fs::metadata(&marker_path)?.mode() & 0o777, 0o600);

            let thread_id = ThreadId::new();
            let thread_id_string = thread_id.to_string();
            recovery.update_session(thread_id, cwd)?;
            let persisted = read_marker(&marker_path)?;
            assert_eq!(
                persisted.thread_id.as_deref(),
                Some(thread_id_string.as_str())
            );
            assert_eq!(persisted.cwd, cwd);

            recovery.finish_clean()?;
            assert!(!marker_path.exists());
            Ok(())
        }

        #[test]
        fn explicit_startup_resume_suppresses_notice_and_retires_stale_marker() -> io::Result<()> {
            let temp = tempfile::tempdir()?;
            let cwd = Path::new("/workspace");
            let directory = temp.path().join(MARKER_DIRECTORY);
            ensure_private_directory(&directory)?;
            let stale_path = directory.join("run-stale.json");
            let thread_id = ThreadId::new();
            let mut stale = marker("definitely-an-older-boot", 43, 456, cwd);
            stale.thread_id = Some(thread_id.to_string());
            persist_marker(&directory, &stale_path, &stale)?;

            let (mut recovery, notice) = RecoveryRun::begin(temp.path(), cwd, Some(thread_id))?;
            assert!(notice.is_none());
            recovery.update_session(thread_id, cwd)?;
            assert!(!stale_path.exists());
            recovery.finish_clean()?;
            Ok(())
        }

        #[test]
        fn read_marker_rejects_group_readable_file() -> io::Result<()> {
            let temp = tempfile::tempdir()?;
            let path = temp.path().join("run-readable.json");
            persist_marker(
                temp.path(),
                &path,
                &marker("boot-old", 43, 456, Path::new("/workspace")),
            )?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))?;

            let error = read_marker(&path).expect_err("group-readable marker must be rejected");
            assert_eq!(error.kind(), io::ErrorKind::Other);
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) use linux::RecoveryRun;

#[cfg(not(target_os = "linux"))]
pub(crate) struct RecoveryRun;

#[cfg(not(target_os = "linux"))]
impl RecoveryRun {
    pub(crate) fn begin(
        _codex_home: &Path,
        _cwd: &Path,
        _startup_resume_thread_id: Option<ThreadId>,
    ) -> io::Result<(Self, Option<String>)> {
        Ok((Self, None))
    }

    pub(crate) fn disabled() -> Self {
        Self
    }

    pub(crate) fn update_session(&mut self, _thread_id: ThreadId, _cwd: &Path) -> io::Result<()> {
        Ok(())
    }

    pub(crate) fn finish_clean(&mut self) -> io::Result<()> {
        Ok(())
    }
}
