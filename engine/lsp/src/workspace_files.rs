//! Injected workspace disk access. The CLI owns discovery and watching; the LSP
//! only consumes file payloads through this trait.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One on-disk `.lemma` file supplied by the host (CLI).
#[derive(Debug, Clone)]
pub struct DiskLemmaFile {
    pub path: PathBuf,
    pub text: String,
}

/// Keeps a filesystem watch alive until dropped.
pub struct WatchGuard {
    _keep_alive: Box<dyn Send + Sync>,
}

impl WatchGuard {
    pub fn from_keep_alive(keep_alive: Box<dyn Send + Sync>) -> Self {
        Self {
            _keep_alive: keep_alive,
        }
    }
}

/// Failure loading or watching workspace files from the host.
#[derive(Debug)]
pub struct WorkspaceFilesError {
    message: String,
}

impl WorkspaceFilesError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for WorkspaceFilesError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for WorkspaceFilesError {}

/// Host-provided disk access for native LSP (implemented by the CLI).
pub trait WorkspaceFiles: Send + Sync {
    fn load(&self, root: &Path) -> std::result::Result<Vec<DiskLemmaFile>, WorkspaceFilesError>;

    fn watch(
        &self,
        root: PathBuf,
        on_change: Arc<
            dyn Fn(std::result::Result<Vec<DiskLemmaFile>, WorkspaceFilesError>) + Send + Sync,
        >,
    ) -> std::result::Result<WatchGuard, WorkspaceFilesError>;
}
