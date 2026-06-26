# 🐴 The Clipboard Centaur

**A fast, deterministic, and zero-compute local file patcher for AI coding workflows.**

The Clipboard Centaur bridges the gap between Cloud LLMs (like ChatGPT or Claude Web) and your local codebase. Instead of manually copying and pasting code back into your IDE, you simply copy the AI's output, and the Centaur instantly patches your local files with mathematical precision. 

Best of all? It costs **$0**, uses **zero local GPU**, and features an intelligent local LLM fallback if the patch gets messy.

---

## ⚡ Features

- **Blazing Fast (Zero-Compute):** Written in Rust, the tool parses `<<<<<<< SEARCH` and `>>>>>>> REPLACE` blocks and applies them instantaneously using exact and fuzzy-string matching.
- **Direct Clipboard Integration:** Parses AI output directly from your OS clipboard (`--clipboard`). No terminal pasting required.
- **Smart Local LLM Fallback (`--llm auto`):** If a patch fails (e.g., ambiguous matches), the Centaur will dynamically assess your system's available RAM and securely boot up a local Ollama model (like `deepseek-coder` or `qwen2.5-coder`) to surgically fix the file in the background.
- **Deep Safety Nets:** Built-in protection against directory traversal attacks, ambiguous match failures, and OS file locks.

---

## 🚀 Installation

Ensure you have [Rust installed](https://rustup.rs/), then clone and build the optimized binary:

```bash
git clone https://github.com/DevT02/The-Clipboard-Centaur.git
cd The-Clipboard-Centaur
cargo build --release
```

**Pro-Tip:** Add the `target\release\` directory to your Windows/Linux `PATH` so you can just type `centaur` from anywhere!

*(Optional) If you want the `--llm auto` fallback feature to work, ensure you have [Ollama installed](https://ollama.com/) on your machine.*

---

## 📖 The Daily Workflow

### Step 1: The Robust AI Prompt
Start your conversation with ChatGPT (or put this in your Custom Instructions / System Prompt). This guarantees the AI outputs the correct, parsable format.

```text
You are an expert developer assisting with a local codebase. 

When you suggest modifications, refactors, or fixes, you must ONLY output the changes using specific Search/Replace blocks. Do not output the entire file.

FORMAT:
For each change, you must provide a block like this:

File: <relative file path>
<<<<<<< SEARCH
<exact lines to replace, including surrounding context>
=======
<the new lines to insert>
>>>>>>> REPLACE

RULES:
1. The SEARCH block MUST perfectly match the existing code in the file, character for character, including whitespace and indentation.
2. Include a few lines of context before and after the change in the SEARCH block to ensure it is uniquely matched.
3. You can output multiple blocks for multiple changes.
```

### Step 2: Ask the AI
Paste your code into the chat and ask it to make a change (e.g., *"Add a login button"*). 

### Step 3: Run the Centaur
When the AI generates the blocks, simply copy its output. Open your terminal inside your project directory and run:

```bash
centaur -c --llm auto
```
The Centaur will instantly read your clipboard, locate the files, and apply the exact diffs. 

---

## ⚙️ CLI Usage

```text
Usage: centaur [OPTIONS]

Options:
  -c, --clipboard      Read the patch text directly from the OS clipboard.
  -f, --file <FILE>    Read the patch text from a specific text file.
  -l, --llm <LLM>      Fallback to a local LLM via Ollama if the patch fails. Use 'auto' to automatically select the best model based on available RAM.
  -h, --help           Print help menu.
  -V, --version        Print version.
```
