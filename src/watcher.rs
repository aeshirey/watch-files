use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use crate::{FileResults, FileStatus, StopCondition};

pub struct Watcher<F> {
    glob: String,

    /// The closure to call when a file has matured
    callback: F,

    /// The duration between each check for new files.
    ///
    /// This globs files from the filesystem and compares them to files previously seen.
    check_interval: Duration,

    /// Whether files should be deleted from disk after they're processed. Default is `false`.
    delete_on_completion: bool,

    /// How long after a file is no longer updated until we consider it to be completed
    mature_after: Duration,

    ///
    verbose: bool,
}

impl<F, T, E> Watcher<F>
where
    F: Fn(&Path) -> Result<T, E>,
{
    pub fn new<U: ToString>(glob: U, callback: F) -> Self {
        Watcher {
            glob: glob.to_string(),
            callback,
            check_interval: Duration::from_secs(1),
            delete_on_completion: false,
            mature_after: Duration::from_secs(5),
            verbose: false,
        }
    }

    /// Sets the minimum [Duration] used for checking for new files to be processed or
    /// existing files that haven't yet been completed.
    ///
    /// Note that this is the _minimum_ duration; due to processing time for other files,
    /// the actual time may exceed this.
    pub fn check_duration(mut self, duration: Duration) -> Self {
        self.check_interval = duration;
        self
    }

    pub fn delete_on_completion(mut self, delete: bool) -> Self {
        self.delete_on_completion = delete;
        self
    }

    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn maturation(mut self, duration: Duration) -> Self {
        self.mature_after = duration;
        self
    }

    pub fn watch(&mut self, condition: StopCondition) -> FileResults<T, E>
    where
        E: From<std::io::Error>,
    {
        let mut files_seen = HashMap::<PathBuf, FileStatus<T, E>>::new();

        let start_time = Instant::now();
        let mut newest_file = SystemTime::now();

        loop {
            // Check all files
            let iteration_start = Instant::now();

            for file in glob::glob(&self.glob)
                .expect("Couldn't glob files")
                .flatten()
            {
                match modification_time(&file) {
                    Err(e) => {
                        // Couldn't get metadata->modified time, so we can't track it.
                        if self.verbose {
                            eprintln!("Couldn't get metadata for {}: {e:?}", file.display());
                        }

                        files_seen.insert(file, FileStatus::Error(e.into()));
                    }
                    Ok(current_systime) => {
                        let entry = files_seen
                            .entry(file.clone())
                            .or_insert_with(|| FileStatus::Processing(current_systime));

                        if let FileStatus::Processing(last_seen) = entry {
                            // The file was previously seen; update its last seen time (which may or may not be
                            // different than what was previously set).
                            newest_file = current_systime;
                            *last_seen = current_systime;

                            // This file hasn't yet been processed
                            if let Ok(d) = last_seen.elapsed() {
                                // Able to calculate the Duration from the Systemtime
                                if d >= self.mature_after {
                                    // The last modified date is old enough for us to consider this file completed.
                                    *entry = match (self.callback)(&file) {
                                        Ok(t) if self.delete_on_completion => {
                                            match (std::fs::remove_file(&file), self.verbose) {
                                                (Ok(_), true) => println!(
                                                    "Processed and deleted {}.",
                                                    file.display()
                                                ),
                                                (Err(e), true) => {
                                                    eprintln!(
                                                        "Processed but failed to delete {}: {e:?}",
                                                        file.display()
                                                    )
                                                }
                                                _ => {}
                                            }
                                            FileStatus::ProcessingCompleted(t)
                                        }
                                        Ok(t) => FileStatus::ProcessingCompleted(t),
                                        Err(e) => FileStatus::Error(e),
                                    };
                                }
                            }
                        }
                    }
                }
            }

            match condition {
                StopCondition::Once => break,
                StopCondition::FilesFound(n) => {
                    if files_seen
                        .values()
                        .filter(|f| matches!(f, FileStatus::ProcessingCompleted(_)))
                        .count()
                        >= n
                    {
                        if self.verbose {
                            println!(
                                "Processing halted: {n} files have been successfully processed."
                            )
                        }

                        break;
                    }
                }
                StopCondition::Elapsed(d) => {
                    if d > start_time.elapsed() {
                        if self.verbose {
                            println!("Processing halted: {d:?} elapsed since processing started.");
                        }
                        break;
                    }
                }
                StopCondition::NoNewFilesSince(d) => {
                    if let Ok(newest) = newest_file.elapsed() {
                        if newest >= d {
                            if self.verbose {
                                println!(
                                "Processing halted: {d:?} elapsed since a new file has been seen."
                            );
                            }

                            break;
                        }
                    }
                }
            }

            let iteration_elapsed = iteration_start.elapsed();

            if self.check_interval > iteration_elapsed {
                std::thread::sleep(self.check_interval - iteration_elapsed);
            }
        }

        let mut completed = HashMap::new();
        let mut not_processed = Vec::new();
        let mut errored = HashMap::new();

        for (path, status) in files_seen {
            match status {
                FileStatus::ProcessingCompleted(t) => {
                    completed.insert(path, t);
                }
                FileStatus::Processing(_) => not_processed.push(path),
                FileStatus::Error(e) => {
                    errored.insert(path, e);
                }
            }
        }

        FileResults {
            completed,
            not_processed,
            errored,
        }
    }
}

/// Result flattening [is unstable](https://github.com/rust-lang/rust/issues/70142),
/// so this function simplifies getting the system time from a file
fn modification_time(path: &Path) -> Result<SystemTime, std::io::Error> {
    let metadata = path.metadata()?;
    let modified = metadata.modified()?;
    Ok(modified)
}
