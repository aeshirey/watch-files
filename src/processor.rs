use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

/// Handles processing within a thread.
///
/// Processing is handled in a loop that is conceptually:
/// ```text
/// while `queue` is not None:
///    acquire lock on `queue`
///    if an input is available:
///        remove input from queue
///        drop lock
///        process input
///            on success T, acquire lock on `successes` and insert T
///            on error E, acquire lock on `errors` and insert E
/// ```
pub(crate) struct Processor<T, E> {
    /// The queue of files to process
    pub queue: Arc<Mutex<Option<VecDeque<PathBuf>>>>,

    /// The map of successful inputs to their results
    pub successes: Arc<Mutex<HashMap<PathBuf, T>>>,

    /// The map of errored inputs to their errors
    pub errors: Arc<Mutex<HashMap<PathBuf, E>>>,

    /// The user-provided callback that turns the input [PathBuf] into either a success T or error E
    pub callback: Box<dyn Fn(PathBuf) -> Result<T, E>>,

    /// Whether messages will be written to stdout/stderr.
    pub verbose: bool,

    /// Whether input files should be deleted upon successful processing (ie, if the callback returns Ok(T))
    pub delete_on_completion: bool,
}

impl<T, E> Processor<T, E>
where
    T: Send,
    E: Send,
{
    pub fn process(self) {
        loop {
            // Acquire a lock and check if there's anything to process. If there is an item,
            // this block will drop the lock so other threads have access.
            let input = {
                let mut lock = self.queue.lock().unwrap();

                let queue = match lock.as_mut() {
                    Some(q) => q,
                    None => return, // None signals that we need to stop processing
                };

                match queue.pop_front() {
                    Some(p) => p,
                    None => {
                        // queue is empty but processing hasn't stopped.
                        // Drop the lock before sleeping so other threads have a chance to access.
                        drop(lock);
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                }
            };

            // We now have a file to process.
            match (self.callback)(input.clone()) {
                Ok(t) => match self.successes.lock() {
                    Ok(mut l) => {
                        if self.delete_on_completion {
                            match (std::fs::remove_file(&input), self.verbose) {
                                (Ok(()), true) => {
                                    println!("Processed and deleted {}.", input.display())
                                }
                                (Err(e), true) => {
                                    eprintln!(
                                        "Processed but failed to delete {}: {e:?}",
                                        input.display()
                                    )
                                }
                                _ => {}
                            }
                        }

                        l.insert(input, t);
                    }
                    Err(_) => eprintln!("Unable to save {} to successes", input.display()),
                },
                Err(e) => match self.errors.lock() {
                    Ok(mut l) => {
                        l.insert(input, e);
                    }
                    Err(_) => eprintln!("Unable to save {} to errors", input.display()),
                },
            }

            // This thread is done processing or attempting to process one time. Wait a bit to let
            // other threads get their turn.
            thread::sleep(Duration::from_millis(500));
        }
    }
}
