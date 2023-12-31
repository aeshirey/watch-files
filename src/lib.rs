mod watcher;
pub use watcher::Watcher;

mod processor;

use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, SystemTime},
};

/// Specifies how a watcher will stop monitoring files
#[derive(Clone, Copy)]
pub enum StopCondition {
    /// Looks for matching files once, stopping execution immediately after processing all files
    Once,

    /// Continues watching until the specified number of files have been found
    FilesFound(usize),

    /// Continues watching until the specified duration of time has elapsed
    Elapsed(Duration),

    /// Continues watching until the specified duration has elapsed without a new file
    NoNewFilesSince(Duration),
}

#[derive(Debug)]
enum FileStatus<T, E> {
    /// Unable to process because a modification time couldn't be read.
    Skipped(std::io::Error),

    /// File has been seen with modification time but has not yet reached maturation.
    Seen(SystemTime),

    /// Used only in multithreaded as a way to signal that a thread has claimed an input.
    Processing,

    /// Successfully completed.
    Processed(T),

    /// Failed to process.
    Errored(E),
}

pub struct FileResults<T, E> {
    /// Files that were skipped because a modification time couldn't be read.
    pub skipped: HashMap<PathBuf, std::io::Error>,

    /// A list of files that were not processed because the stop condition
    /// was hit before they could mature.
    pub not_processed: Vec<PathBuf>,

    /// Files successfully processed with the user-specified closure as `Ok(T)`
    pub completed: HashMap<PathBuf, T>,

    /// Files that failed the user-specified processing with `Err(E)`
    pub errored: HashMap<PathBuf, E>,
}

/// Result flattening [is unstable](https://github.com/rust-lang/rust/issues/70142),
/// so this function simplifies getting the system time from a file
fn modification_time(path: &std::path::Path) -> Result<SystemTime, std::io::Error> {
    let metadata = path.metadata()?;
    let modified = metadata.modified()?;
    Ok(modified)
}
