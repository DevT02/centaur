use arboard::Clipboard;
use clap::Parser;
use std::env;
use std::fs;
use std::io::{self, Read};
use the_clipboard_centaur::{apply_block, parse_blocks, ApplyResult};

#[derive(Parser, Debug)]
#[command(
    name = "The Clipboard Centaur",
    version,
    about = "Applies LLM-generated search/replace blocks directly to local files.",
    long_about = "A fast, deterministic, and consumer-friendly CLI that parses search/replace blocks (e.g. from ChatGPT) and applies them locally with deep safety nets."
)]
struct Args {
    /// Read the patch text directly from the OS clipboard instead of standard input.
    #[arg(short, long)]
    clipboard: bool,

    /// Read the patch text from a specific file instead of standard input.
    #[arg(short, long)]
    file: Option<String>,
}

fn main() {
    let args = Args::parse();

    let input = if args.clipboard {
        println!("Reading patch instructions from the clipboard...");
        match Clipboard::new() {
            Ok(mut cb) => match cb.get_text() {
                Ok(text) => text,
                Err(e) => {
                    eprintln!("Failed to read text from clipboard: {}", e);
                    return;
                }
            },
            Err(e) => {
                eprintln!("Failed to initialize clipboard: {}", e);
                return;
            }
        }
    } else if let Some(path) = args.file {
        println!("Reading patch instructions from {}...", path);
        match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("Failed to read file {}: {}", path, e);
                return;
            }
        }
    } else {
        println!("Paste the output from ChatGPT below, then press Ctrl+D (Unix) or Ctrl+Z then Enter (Windows) to execute:\n");
        let mut text = String::new();
        if let Err(e) = io::stdin().read_to_string(&mut text) {
            eprintln!("Failed to read from stdin: {}", e);
            return;
        }
        text
    };

    let blocks = parse_blocks(&input);
    if blocks.is_empty() {
        println!("No valid Search/Replace blocks found in the input.");
        return;
    }

    let current_dir = env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    for block in blocks {
        match apply_block(&current_dir, &block) {
            ApplyResult::Created(path) => println!("✅ Created new file: {}", path),
            ApplyResult::Updated(path) => println!("✅ Successfully updated: {}", path),
            ApplyResult::AmbiguousMatch(path) => {
                println!("❌ ERROR: Search block matches multiple locations in {}. Please be more specific.", path)
            }
            ApplyResult::MatchNotFound(path) => {
                println!("❌ ERROR: Could not find exact match for SEARCH block in {}", path)
            }
            ApplyResult::IoError(path, err) => {
                println!("❌ ERROR: IO Error for file {}: {}", path, err)
            }
            ApplyResult::SecurityError(err) => {
                println!("🚫 ERROR: Security violation: {}", err)
            }
        }
    }
}
