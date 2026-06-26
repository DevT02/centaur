# The Clipboard Centaur

A dead-simple, deterministic file patcher for AI coding workflows. 

It connects ChatGPT Web directly to your local file system using your clipboard. You send your codebase to ChatGPT, it spits out code diffs, and Centaur applies those diffs to your local files instantly. 

If a patch fails due to ChatGPT hallucinating the context, Centaur automatically spins up a local, offline LLM via Ollama to intelligently resolve the merge conflict for free.

## Installation

You need [Rust](https://rustup.rs/) installed.

```sh
git clone https://github.com/DevT02/The-Clipboard-Centaur.git
cd The-Clipboard-Centaur
cargo install --path .
```
This globally installs the `centaur` binary to your PATH.

*(Optional: Install [Ollama](https://ollama.com/) if you want the local LLM fallback).*

## The Workflow

### 1. Feed your codebase to ChatGPT
Use the `--pack` command to bundle your project into a format ChatGPT easily understands. It strictly respects your `.gitignore`.

```sh
centaur --pack src/ Cargo.toml
```
- If your project is small, it instantly copies the bundle to your clipboard.
- If your project is massive, it automatically chunks it into files (e.g. `centaur_context_part1.txt`) so you can seamlessly drag-and-drop them into the ChatGPT UI.

### 2. The System Prompt
Paste your codebase into ChatGPT along with your feature request, and **include this strict prompt**:

```text
Output modifications ONLY using this exact Search/Replace block format. Do not output full files.

File: <path>
<<<<<<< SEARCH
<exact lines to replace, with context>
=======
<new lines>
>>>>>>> REPLACE
```

### 3. Apply the AI's Code
When ChatGPT responds with the diffs, simply highlight them and press `Ctrl+C` (Copy). Then, in your terminal, run:

```sh
centaur -c --llm auto
```
- `-c`: Reads the diffs directly from your clipboard and patches the files.
- `--llm auto`: If the deterministic patcher fails (e.g., ChatGPT messed up the indentation in the `SEARCH` block), Centaur will scan your system's available RAM and automatically trigger an intelligent, offline model (like DeepSeek or Qwen) to fix the file locally.
