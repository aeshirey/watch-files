use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime},
};

use crate::{processor::Processor, FileResults, FileStatus, StopCondition};

/// Monitors for new files according to the specified glob, processing them with
/// a user-provided closure.
///
/// Processing is done single-threaded with the `watch` method or multi-threaded with
/// the `watch_threaded` method.
pub struct Watcher<F> {
    glob: String,

    /// The closure to call when a file has matured
    callback: F,

    /// The duration between each check for new files. Default is 1 second.
    ///
    /// This globs files from the filesystem and compares them to files previously seen.
    check_interval: Duration,

    /// Whether files should be deleted from disk after they're processed. Default is `false`.
    delete_on_completion: bool,

    /// How long after a file is no longer updated until we consider it to be completed. Default is
    /// 5 seconds.
    mature_after: Duration,

    /// Specifies whether output messages should be written to stdout/stderr. Default is `false`.
    verbose: bool,
}

impl<F, T, E> Watcher<F>
where
    F: Fn(PathBuf) -> Result<T, E>,
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

    /// Sets the minimum duration in seconds used for checking for new files to be processed or
    /// existing files that haven't yet been completed.
    ///
    /// Note that this is the _minimum_ duration; due to processing time for other files,
    /// the actual time may exceed this.
    pub fn check_duration_secs(mut self, secs: f64) -> Self {
        let duration = Duration::from_secs_f64(secs);
        self.check_interval = duration;
        self
    }

    /// Specifies that after files have been processed, they should be deleted. Default is false.
    pub fn delete_on_completion(mut self, delete: bool) -> Self {
        self.delete_on_completion = delete;
        self
    }

    /// Specifies whether to print verbose output to stdout.
    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Sets the [Duration] after which a file is considered to be ready for processing by the callback.
    pub fn maturation(mut self, duration: Duration) -> Self {
        self.mature_after = duration;
        self
    }

    /// Sets the duration in seconds after which a file is considered to be ready for processing by the callback.
    pub fn maturation_secs(mut self, secs: f64) -> Self {
        let duration = Duration::from_secs_f64(secs);
        self.mature_after = duration;
        self
    }

    /// Performs single-threaded monitoring of files, stopping when the [StopCondition].
    ///
    /// # Panics
    /// On invalid glob.
    pub fn watch(&self, condition: StopCondition) -> FileResults<T, E> {
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
                match crate::modification_time(&file) {
                    Err(e) if self.verbose => {
                        // Couldn't get metadata->modified time, so we can't track it.
                        eprintln!("Couldn't get metadata for {}: {e:?}", file.display());
                        files_seen.insert(file, FileStatus::Skipped(e));
                    }
                    Err(e) => {
                        files_seen.insert(file, FileStatus::Skipped(e));
                    }
                    Ok(modtime) => {
                        let entry = files_seen
                            .entry(file.clone())
                            .or_insert(FileStatus::Seen(modtime));

                        if let FileStatus::Seen(last_seen) = entry {
                            // The file was previously seen; update its last seen time (which may or may not be
                            // different than what was previously set).
                            *last_seen = modtime;

                            if modtime > newest_file {
                                newest_file = modtime;
                            }

                            // This file hasn't yet been processed
                            let Ok(d) = last_seen.elapsed() else { continue };

                            // Able to calculate the Duration from the Systemtime
                            if d >= self.mature_after {
                                // The last modified date is old enough for us to consider this file completed.
                                *entry = match (self.callback)(file.clone()) {
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
                                        FileStatus::Processed(t)
                                    }
                                    Ok(t) => FileStatus::Processed(t),
                                    Err(e) => FileStatus::Errored(e),
                                };
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
                        .filter(|f| matches!(f, FileStatus::Processed(_)))
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
                                println!("Processing halted: {d:?} elapsed since a new file has been seen.");
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
        let mut skipped = HashMap::new();

        for (path, status) in files_seen {
            match status {
                FileStatus::Seen(_) => not_processed.push(path),
                FileStatus::Processing => unreachable!(), // not used when single-threaded
                FileStatus::Processed(t) => {
                    completed.insert(path, t);
                }
                FileStatus::Errored(e) => {
                    errored.insert(path, e);
                }
                FileStatus::Skipped(e) => {
                    skipped.insert(path, e);
                }
            }
        }

        FileResults {
            completed,
            not_processed,
            errored,
            skipped,
        }
    }
}

impl<F, T, E> Watcher<F>
where
    F: Fn(PathBuf) -> Result<T, E>,
    F: Clone,
    T: Send + 'static,
    E: Send + 'static,
    F: Send + 'static,
{
    pub fn watch_threaded(
        &mut self,
        condition: StopCondition,
        num_threads: usize,
    ) -> FileResults<T, E> {
        let queue = Arc::new(Mutex::new(Some(VecDeque::new())));
        let successes = Arc::new(Mutex::new(HashMap::new()));
        let errors = Arc::new(Mutex::new(HashMap::new()));

        // start the threads
        let threads = (0..num_threads)
            .map(|_| {
                let queue = queue.clone();
                let successes = successes.clone();
                let errors = errors.clone();
                let callback = Box::new(self.callback.clone());
                let verbose = self.verbose;
                let delete_on_completion = self.delete_on_completion;

                thread::spawn(move || {
                    Processor {
                        queue,
                        successes,
                        errors,
                        callback,
                        verbose,
                        delete_on_completion,
                    }
                    .process()
                })
            })
            .collect::<Vec<_>>();

        let mut files_seen = HashMap::<PathBuf, FileStatus<T, E>>::new();

        let start_time = Instant::now();
        let mut newest_file = SystemTime::now();

        loop {
            // Look for inputs that need to be processed
            for file in glob::glob(&self.glob)
                .expect("Couldn't glob files")
                .flatten()
            {
                match crate::modification_time(&file) {
                    Err(e) => {
                        // Couldn't get metadata->modified time, so we can't track it.
                        if self.verbose {
                            eprintln!("Couldn't get metadata for {}: {e:?}", file.display());
                        }

                        files_seen.insert(file, FileStatus::Skipped(e));
                    }
                    Ok(modtime) => {
                        let entry = files_seen
                            .entry(file.clone())
                            .or_insert(FileStatus::Seen(modtime));

                        if let FileStatus::Seen(last_seen) = entry {
                            // The file was previously seen; update its last seen time (which may or may not be
                            // different than what was previously set).
                            *last_seen = modtime;

                            newest_file = newest_file.max(modtime);

                            // This file hasn't yet been processed
                            let Ok(d) = last_seen.elapsed() else { continue };

                            // Able to calculate the Duration from the Systemtime
                            if d >= self.mature_after {
                                *entry = FileStatus::Processing;
                                // The last modified date is old enough for us to consider this file completed.
                                let mut l = queue.lock().unwrap();

                                // Safe to unwrap because we only set the queue to None after stop condition is met.
                                let q = l.as_mut().unwrap();
                                q.push_back(file.clone());
                            }
                        }
                    }
                }
            }

            match condition {
                StopCondition::Once => break,
                StopCondition::FilesFound(n) => {
                    // how many files have been successfully processed?
                    if successes.lock().unwrap().len() >= n {
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

            // Sleep a bit to let the workers access the queue
            thread::sleep(self.check_interval);
        }

        // now that we've broken out of the loop, signal to the threads that we're done, then join them
        {
            if self.verbose {
                println!("Signaling threads to stop...");
            }

            {
                let mut q = queue.lock().unwrap();
                *q = None;
            }

            if self.verbose {
                println!("Waiting for threads to stop...");
            }

            for thread in threads {
                thread.join().ok();
            }
        }

        // Get the results from the various Arcs
        let completed = match Arc::try_unwrap(successes) {
            Ok(l) => l.into_inner().unwrap(),
            Err(_) => panic!("Unable to unwrap sole 'success'"),
        };

        let errored = match Arc::try_unwrap(errors) {
            Ok(l) => l.into_inner().unwrap(),
            Err(_) => panic!("Unable to unwrap sole 'error'"),
        };

        let mut not_processed = Vec::new();
        let mut skipped = HashMap::new();

        for (path, status) in files_seen {
            match status {
                FileStatus::Skipped(e) => {
                    skipped.insert(path, e);
                }
                FileStatus::Seen(_) => {
                    not_processed.push(path);
                }
                FileStatus::Processing => {} // these should appear as completed/error
                FileStatus::Errored(_) | FileStatus::Processed(_) => unreachable!(),
            }
        }

        FileResults {
            completed,
            not_processed,
            errored,
            skipped,
        }
    }
}
