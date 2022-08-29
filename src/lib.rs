mod watcher;
pub use watcher::Watcher;

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
enum FileStatus<T,E> {
    ProcessingCompleted(T),
    Processing(SystemTime),
    Error(E),
}

pub struct FileResults<T, E> {
    /// Files successfully processed. The input path maps to the value returned
    /// by the closure.
    pub completed: HashMap<PathBuf, T>,

    /// A list of files that were not processed because the stop condition
    /// was hit before they could mature.
    pub not_processed: Vec<PathBuf>,

    /// Files that were not processed due to an error.
    /// 
    /// The user-specified closure can return `E` or the watcher 
    /// itself can return std::io::Error if metadata can't be fetched.
    pub errored: std::collections::HashMap<PathBuf, E>,
}
