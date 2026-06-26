use std::env;
use std::io::{self, Read};
use chimera::{parse_blocks, apply_block, ApplyResult};

fn main() {
    println!("Paste the output from ChatGPT below, then press Ctrl+D (Unix) or Ctrl+Z then Enter (Windows) to execute:\n");

    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        eprintln!("Failed to read from stdin: {}", e);
        return;
    }

    let blocks = parse_blocks(&input);
    if blocks.is_empty() {
        println!("No valid Search/Replace blocks found in the input.");
        return;
    }

    let current_dir = env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    for block in blocks {
        match apply_block(&current_dir, &block) {
            ApplyResult::Created(path) => println!("Created new file: {}", path),
            ApplyResult::Updated(path) => println!("Successfully updated: {}", path),
            ApplyResult::MatchNotFound(path) => println!("ERROR: Could not find exact match for SEARCH block in {}", path),
            ApplyResult::IoError(path, err) => println!("ERROR: IO Error for file {}: {}", path, err),
            ApplyResult::SecurityError(err) => println!("ERROR: Security violation: {}", err),
        }
    }
}
