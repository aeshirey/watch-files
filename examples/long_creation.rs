//! This example shows how files that take a long time to be created (eg, because
//! they're large files that take a long time to download) will be waited for

use std::{io::Write, thread, time::Duration};
use watch_files::{FileResults, StopCondition, Watcher};

pub fn main() {
    // Start creating files
    let thread1 = thread::spawn(|| create_file(1, 100));
    thread::sleep(Duration::from_secs(1));
    let thread2 = thread::spawn(|| create_file(2, 100));

    // Watch for them to be created and process them as they become ready
    let FileResults {
        completed,
        not_processed,
        errored,
    } = Watcher::new("long_creation_file_*.txt", |path| {
        std::fs::read_to_string(path).map(|s| s.len())
    })
    .maturation(Duration::from_secs(5))
    .delete_on_completion(true)
    .verbose(true)
    .watch(StopCondition::NoNewFilesSince(Duration::from_secs(10)));

    thread1.join().ok();
    thread2.join().ok();

    println!("Found files: {completed:?}");
    assert_eq!(200, completed.values().sum::<usize>());

    assert_eq!(not_processed.len(), 0, "No unprocessed files");
    assert_eq!(errored.len(), 0, "No errors");
}

fn create_file(number: u8, length: usize) {
    let filename = format!("long_creation_file_{number}.txt");
    println!("Creating {filename}");
    let mut f = std::fs::File::create(&filename).expect("Couldn't create file");

    for _ in 0..length {
        f.write(b".").ok();
        thread::sleep(Duration::from_secs(1));
    }
}
