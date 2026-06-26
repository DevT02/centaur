# the-clipboard-centaur

A stupid simple, deterministic file patcher for AI coding workflows. 

You use ChatGPT Web. You paste your code. ChatGPT spits out diffs. You copy the diffs. You run `centaur -c`. It parses the diffs and patches your local files instantly. No API keys, no monthly subscriptions, no convoluted IDE plugins. 

If ChatGPT hallucinates the diff context and the patch fails, Centaur uses your available RAM to spin up a local Ollama model in the background, feeds it the broken patch, and fixes the file locally.

## Build

You need Rust. 

```sh
git clone https://github.com/DevT02/The-Clipboard-Centaur.git
cd The-Clipboard-Centaur
cargo build --release
```
Throw `target/release/centaur` into your PATH. 

*(Optional: Install Ollama if you want the local LLM fallback feature).*

## Usage

Start your ChatGPT session with this system prompt to force it into outputting clean blocks:

```text
Output modifications using this exact Search/Replace block format. Do not output full files.

File: <path>
<<<<<<< SEARCH
<exact lines to replace, with context>
=======
<new lines>
>>>>>>> REPLACE
```

When it replies, copy the text and run:

```sh
centaur -c --llm auto
```
It reads your clipboard, patches the files, and exits. If it fails, `--llm auto` triggers a local Ollama model (sized dynamically to your available RAM) to resolve the merge conflict.

### Packing Context

If you need to feed your project to ChatGPT, use the pack command. It respects `.gitignore`. You can pass multiple folders or files.

```sh
centaur --pack src/ utils/ config.toml
```

If the output is massive, it will copy to your clipboard, but be aware of ChatGPT's context limits.
