# watch-files
Monitor the creation of files and process them when complete.

This crate aims to make it easy to create a process that will keep an eye on files as they are created and, via the [`Metadata::modified` time](https://doc.rust-lang.org/std/fs/struct.Metadata.html#method.modified), when it was last updated. Based on a specified maturation period, it will run a specified closure to process the completed files.

For example, the following will process all text files seen at least 10 seconds after they have stopped being updated. After the closure is run on a file, it will be deleted from disk, and only the first 10 files are processed:

```rust
let results = Watcher::new("*.txt", |path| {
        // do something with the input file
        todo!()
    })
    .maturation(Duration::from_secs_f64(10))
    .delete_on_completion(true)
    .watch(StopCondition::FilesFound(10));
```

## TODO
- [ ] Add multithreading support
