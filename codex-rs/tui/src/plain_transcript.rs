use chrono::SecondsFormat;
use chrono::Utc;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub(crate) enum PlainTranscriptRole {
    User,
    CodexCommentary,
    CodexPartial,
    CodexFinal,
    CodexPlan,
    Codex,
}

impl PlainTranscriptRole {
    fn label(self) -> &'static str {
        match self {
            Self::User => "USER",
            Self::CodexCommentary => "CODEX COMMENTARY",
            Self::CodexPartial => "CODEX PARTIAL",
            Self::CodexFinal => "CODEX FINAL",
            Self::CodexPlan => "CODEX PLAN",
            Self::Codex => "CODEX",
        }
    }
}

pub(crate) struct PlainTranscript {
    path: PathBuf,
    file: File,
    current_thread_id: Option<String>,
}

impl PlainTranscript {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut options = OpenOptions::new();
        // Read access is also requested because Windows requires it for
        // `File::lock` on an append handle.
        options.read(true).create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // `O_NONBLOCK` prevents a misconfigured FIFO/device path from
            // hanging startup before the metadata validation below rejects it.
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(path)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            use std::os::unix::fs::PermissionsExt;
            let metadata = file.metadata()?;
            // SAFETY: `geteuid` has no preconditions and does not dereference pointers.
            let effective_uid = unsafe { libc::geteuid() };
            if !metadata.is_file() || metadata.uid() != effective_uid || metadata.nlink() != 1 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "plain transcript must be a regular, singly linked file owned by this user",
                ));
            }
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o600);
            file.set_permissions(permissions)?;
        }

        Ok(Self {
            path: path.to_path_buf(),
            file,
            current_thread_id: None,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn begin_session(&mut self, thread_id: &str, cwd: &Path) -> io::Result<()> {
        if self.current_thread_id.as_deref() == Some(thread_id) {
            return Ok(());
        }
        self.current_thread_id = Some(thread_id.to_string());

        let timestamp = timestamp();
        let cwd = sanitize_plain_text(&cwd.to_string_lossy());
        let record = format!(
            "\n===== CODEX SESSION =====\n\
             time: {timestamp}\n\
             version: {} hardened {}\n\
             thread_id: {thread_id}\n\
             cwd: {cwd}\n\
             resume_in_current_tui: /resume {thread_id}\n\
             resume_in_new_process: codex resume {thread_id}\n\
             ===== SESSION CONTINUES =====\n",
            env!("CARGO_PKG_VERSION"),
            crate::version::HARDENED_BUILD_REVISION
        );
        self.write_record(&record)
    }

    pub(crate) fn append(&mut self, role: PlainTranscriptRole, text: &str) -> io::Result<()> {
        let text = sanitize_plain_text(text);
        if text.trim().is_empty() {
            return Ok(());
        }
        let timestamp = timestamp();
        let label = role.label();
        let record = format!("\n----- {label} {timestamp} -----\n{text}\n");
        self.write_record(&record)
    }

    fn write_record(&mut self, record: &str) -> io::Result<()> {
        // All configured Codex processes for a user may share one transcript.
        // Keep each record contiguous even when two instances append at once.
        self.file.lock()?;
        let write_result = (|| {
            self.file.write_all(record.as_bytes())?;
            self.file.flush()?;
            self.file.sync_data()
        })();
        let unlock_result = self.file.unlock();
        write_result.and(unlock_result)
    }
}

pub(crate) fn safe_display_path(path: &Path) -> String {
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

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn sanitize_plain_text(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let bytes = normalized.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            0x1b => {
                index += 1;
                match bytes.get(index).copied() {
                    Some(b'[') => {
                        index += 1;
                        while index < bytes.len() {
                            let byte = bytes[index];
                            index += 1;
                            if (0x40..=0x7e).contains(&byte) {
                                break;
                            }
                        }
                    }
                    Some(b']') => {
                        index += 1;
                        while index < bytes.len() {
                            if bytes[index] == 0x07 {
                                index += 1;
                                break;
                            }
                            if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
                                index += 2;
                                break;
                            }
                            index += 1;
                        }
                    }
                    Some(_) => index += 1,
                    None => {}
                }
            }
            byte if byte < 0x20 && !matches!(byte, b'\n' | b'\t') => index += 1,
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8(output)
        .unwrap_or_default()
        .chars()
        .filter(|character| matches!(character, '\n' | '\t') || !character.is_control())
        .collect()
}

#[cfg(test)]
#[path = "plain_transcript_tests.rs"]
mod tests;
