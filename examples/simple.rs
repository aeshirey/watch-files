use std::{io::Write, thread, time::Duration};
use watch_files::{FileResults, StopCondition, Watcher};

pub fn main() {
    // Start creating files
    let thread1 = thread::spawn(create_files);

    // Watch for them to be created and process them as they become ready
    let FileResults {
        completed,
        not_processed,
        errored,
    } = Watcher::new("simple_*.txt", |path| {
        std::fs::read_to_string(path).map(|s| s.len())
    })
    .maturation(Duration::from_secs_f64(1.1))
    .delete_on_completion(true)
    .verbose(true)
    .watch(StopCondition::FilesFound(10));

    thread1.join().ok();

    println!("Found files: {completed:?}");
    assert_eq!(100, completed.values().sum::<usize>());

    assert_eq!(not_processed.len(), 0, "No unprocessed files");
    assert_eq!(errored.len(), 0, "No errors");
}

fn create_files() {
    fn create_file(number: u8, length: usize) {
        let filename = format!("simple_{number}.txt");
        println!("Creating {filename}");
        let mut f = std::fs::File::create(&filename).expect("Couldn't create file");

        for _ in 0..length {
            f.write(b".").ok();
        }
    }

    for i in 1..=10 {
        create_file(i, 10);
        thread::sleep(Duration::from_secs(4));
    }
}
