# rlm — The Context Broker

**Stop feeding entire files to your AI. Start querying your codebase.**

[![License: Source Available](https://img.shields.io/badge/License-Source%20Available-lightgrey.svg)](#license)
[![Status: Beta](https://img.shields.io/badge/Status-Beta.svg)]()

> Based on [Recursive Language Models](https://alexzhang13.github.io/blog/2025/rlm/) research (MIT/Stanford, 2025)

---

## About This Project

This is a proof of concept / MVP to validate and make practical use of the ideas presented in the [Recursive Language Models](https://alexzhang13.github.io/blog/2025/rlm/) research (MIT/Stanford, 2025). The paper makes compelling claims about progressive disclosure and query-based retrieval for code understanding — this tool is my attempt to see if those ideas hold up in real-world usage.

It's also my first public project. I'm making the source code available so you can see for yourself what's going on under the hood — no data collection, no shady business, just the code doing what it says.

Honestly, I don't know yet where this project is headed. I want to see how it's received and how people use it before deciding on the next steps. That's why I'm keeping my licensing options open for now. Maybe it'll become fully open source someday, maybe it'll stay as it is — we'll see.

Feedback is welcome. Pull requests are not, at least for now.

---

## The Problem

When AI agents work with code, they typically do this:

```
Agent: "I need to understand this codebase"
→ Reads file1.rs (500 tokens)
→ Reads file2.rs (800 tokens)
→ Reads file3.rs (600 tokens)
→ ...
→ Context window fills up
→ Earlier files get "forgotten" (Context Rot)
→ Agent makes mistakes or asks to re-read files
```

**The result:** Thousands of tokens wasted, context rot, slower responses, higher costs.

## The Solution

rlm treats your codebase like a database, not a pile of files.

```
Agent: "I need to understand this codebase"
→ rlm map (~200 tokens) — sees project structure and purpose of each file
→ rlm refs Config (~50 tokens) — finds all usages of Config
→ rlm read src/config.rs --symbol load (~100 tokens) — reads only the relevant function
→ Done. Total: ~350 tokens instead of thousands.
```

**The principle:** Never load what you don't need. Query, don't dump.

---

## How It Works

### Progressive Disclosure

Instead of reading entire files, rlm lets you zoom in progressively:

```
┌─────────────────────────────────────────────────────────────────┐
│  CHEAPEST                                           MOST DETAIL │
│                                                                 │
│  peek ──→ map ──→ grep ──→ search ──→ read symbol ──→ read file │
│  ~50 tok  ~200    ~100     variable    ~100-500       full file │
│                                                                 │
│  Structure  Overview  Pattern  Full-text  One function  Last    │
│  only       + purpose matches  search     or struct     resort  │
└─────────────────────────────────────────────────────────────────┘
```

Most tasks can be completed without ever reading a full file.

### Context as Environment Variable

Traditional approach:
```python
# Load everything into context, hope for the best
context = read("file1.rs") + read("file2.rs") + read("file3.rs")
llm.generate(context + prompt)
```

rlm approach:
```python
# Query what you need, when you need it
structure = rlm.map()           # What files exist and why?
usages = rlm.refs("Config")     # Where is Config used?
code = rlm.read("config.rs", symbol="load")  # Just this function
llm.generate(structure + usages + code + prompt)
```

The codebase stays outside the context window, queryable on demand.

### Surgical Editing

Traditional AI editing:
```
Agent: *reads 500-line file*
Agent: *rewrites entire file with one small change*
→ Risk of unintended changes
→ 1000+ tokens for input + output
```

rlm editing:
```bash
rlm replace src/lib.rs --symbol helper --code "fn helper(x: i32) -> i32 { x * 3 }"
```
- AST-based: finds the exact node to replace
- Syntax Guard: validates the change compiles before writing
- Minimal: only the changed code goes through the LLM

---

## Quick Start

### Installation

```bash
# Build from source (requires Rust 1.75+)
cargo build --release

# Add to PATH
export PATH="$PWD/target/release:$PATH"
```

### Index Your Project

```bash
cd your-project
rlm index .
```

> **Note:** Indexing respects `.gitignore` — files and directories listed there are automatically skipped.
> Hidden files (starting with `.`) and common build directories (`node_modules/`, `target/`, etc.) are also excluded.

### Explore

```bash
# Get oriented (~200 tokens)
rlm map

# Find where something is used
rlm refs MyStruct

# Read just the function you need
rlm read src/main.rs --symbol main

# Search across the codebase
rlm search "error handling"
```

---

## Setup for AI Agents

rlm is designed to be used by AI agents, not manually. There are two ways to integrate it:

### Option A: MCP Server (Recommended)

The Model Context Protocol (MCP) gives the agent native access to rlm tools.

```bash
# Find where rlm is installed
which rlm
# Example output: /home/user/projects/rlm/target/release/rlm

# Register with Claude Code (use absolute path)
claude mcp add rlm -- /home/user/projects/rlm/target/release/rlm mcp

# Or if you just built it:
claude mcp add rlm -- "$(pwd)/target/release/rlm" mcp

# Verify it's registered
claude mcp list
```

> **Note:** Use the absolute path to the `rlm` binary. MCP servers run as separate processes and may not have access to your shell's PATH.

That's it. The agent now has direct access to all rlm commands as native tools.

**What the agent sees:** All rlm tools with descriptions and parameters are automatically exposed via MCP. The agent can discover available tools and understands the minified JSON output format (r=results, k=kind, n=name, etc.) through tool descriptions.

### Option B: CLI via CLAUDE.md

If you prefer CLI mode, add instructions to your project's `CLAUDE.md`:

```markdown
## rlm Available

This project is indexed with rlm. Use Bash commands for efficient code exploration.

### Quick Reference
- `rlm help` — list all commands
- `rlm help <command>` — detailed help for a command

### Workflow: Start Cheap, Zoom In
1. `rlm peek` — structure only (~50 tokens)
2. `rlm map` — project overview (~200 tokens)
3. `rlm refs <symbol>` — find usages
4. `rlm read <path> --symbol <n>` — read one function
5. `rlm read <path>` — full file (last resort)

### Editing
- `rlm replace <path> --symbol <n> --code "<new>" --preview` — preview
- `rlm replace <path> --symbol <n> --code "<new>"` — apply

### Output Format (Minified JSON)
| Key | Meaning |
|-----|---------|
| `r` | results (array) |
| `k` | kind (fn, struct, class, enum, trait, etc.) |
| `n` | name / identifier |
| `l` | lines [start, end] or single line number |
| `c` | content (code) or count |
| `s` | symbol name |
| `f` | file path |
| `t` | token estimate `{"in": N, "out": N}` |
| `q` | quality warning — if `fallback_recommended: true`, use `read --lines` or `grep` |
```

### Which to Choose?

| Mode | Pros | Cons |
|------|------|------|
| **MCP** | Native integration, no prompting needed | Requires MCP support in agent |
| **CLI** | Works with any agent | Agent must be instructed via CLAUDE.md |

For Claude Code, MCP is recommended. For other agents or simpler setups, CLI works well.

---

## Commands

> **Important:** Most commands (`tree`, `map`, `search`, `refs`, etc.) only operate on
> **indexed files**. If a file wasn't indexed (unsupported extension, excluded by gitignore),
> it won't appear in results. Use `rlm files` to see all files regardless of index status.

### Project Overview

| Command | Use When |
|---------|----------|
| `rlm peek` | Quick structure check (~50 tokens) |
| `rlm map` | Full overview with descriptions (~200 tokens) |
| `rlm tree` | File tree with symbol counts |
| `rlm files` | See ALL files including those with unsupported extensions |
| `rlm files --skipped-only` | Find files that were skipped during indexing |
| `rlm peek` / `rlm map` | See indexed files with their symbols |

### Search

| Command | Use When |
|---------|----------|
| `rlm grep <pattern>` | Find exact text matches (regex supported) |
| `rlm search <query>` | Full-text semantic search |
| `rlm refs <symbol>` | Find all references to a symbol |

### Reading Code

| Command | Use When |
|---------|----------|
| `rlm read <path> --symbol <n>` | Read one function/struct/class |
| `rlm read <path> --lines 10-50` | Read specific line range |
| `rlm read <path>` | Read entire file (use sparingly) |

### Code Intelligence

| Command | Use When |
|---------|----------|
| `rlm callgraph <symbol>` | See what a function calls |
| `rlm impact <symbol>` | See what would break if you change something |
| `rlm deps <path>` | See file dependencies |

### Editing

| Command | Use When |
|---------|----------|
| `rlm replace <path> --symbol <n> --code "<new>"` | Replace a function/struct |
| `rlm replace ... --preview` | Preview the change first |

### Maintenance

| Command | Use When |
|---------|----------|
| `rlm index .` | Initial indexing or full re-index |
| `rlm reindex` | Update index after changes |
| `rlm stats` | See index statistics |
| `rlm quality` | Check for parse quality issues |

---

## Output Format

All output is minified JSON to minimize token consumption:

```json
{"r":[{"id":1,"k":"fn","n":"main","l":[1,5],"c":"fn main() {...}"}],"t":{"in":0,"out":45}}
```

| Key | Meaning |
|-----|---------|
| `r` | results |
| `k` | kind (fn, struct, enum, trait, etc.) |
| `n` | name |
| `l` | lines [start, end] |
| `c` | content |
| `s` | symbol |
| `t` | token estimate `{"in": N, "out": N}` |
| `f` | file path |
| `sig` | signature |
| `dc` | doc comment (`///`, `/**`, docstrings) |
| `at` | attributes/decorators (`#[derive]`, `@Override`) |
| `q` | parse quality warning (see below) |

Example with quality warning:
```json
{"r":[...],"t":{"in":100,"out":50},"q":{"fallback_recommended":true,"el":[15,23],"m":"File has 2 parse errors. Consider using read/grep for affected lines."}}
```

---

## Parse Quality & Fallback

rlm uses tree-sitter for AST-based parsing. Tree-sitter grammars may not support the latest language features. When a file contains unsupported syntax, rlm still indexes it but marks the result with a quality warning.

### How It Works

1. During indexing, each file's parse result is checked for tree-sitter ERROR nodes
2. Files with errors get a `parse_quality` value stored in the database
3. Query responses include a `q` field when quality issues are detected

### Quality Levels

| Level | Meaning |
|-------|---------|
| `complete` | No parse errors, all AST nodes resolved correctly |
| `partial` | Some ERROR nodes found; most of the file was parsed successfully |
| `failed` | Majority of the file could not be parsed |

### Decision Tree for Agents

```
Response received
├── No "q" field → AST data is reliable, use normally
└── "q" field present
    ├── fallback_recommended: false → Minor issues, AST data mostly reliable
    └── fallback_recommended: true
        ├── For reading code → use `rlm read <path> --lines X-Y`
        ├── For searching → use `rlm grep <pattern>`
        └── For refs/callgraph/impact → results may be incomplete
```

### Checking Quality via CLI

```bash
# See files with parse quality issues in stats
rlm stats

# Inspect detailed quality issues
rlm quality --summary      # Summary statistics
rlm quality --unknown-only # Only issues without test coverage
rlm quality --all          # All logged issues
```

---

## Supported Languages

| Language | Parser | Extensions | Chunks Extracted |
|----------|--------|------------|------------------|
| **Rust** | tree-sitter | `.rs` | fn, struct, enum, impl, mod, trait |
| **Go** | tree-sitter | `.go` | func, type, interface, struct |
| **Java** | tree-sitter | `.java` | class, interface, method, enum |
| **C#** | tree-sitter | `.cs` | class, struct, interface, method, enum |
| **Python** | tree-sitter | `.py`, `.pyi` | class, def, async def |
| **PHP** | tree-sitter | `.php` | class, function, interface, trait |
| **JavaScript** | tree-sitter | `.js`, `.jsx` | function, class, arrow, export |
| **TypeScript** | tree-sitter | `.ts` | interface, type, enum, namespace |
| **TSX** | tree-sitter | `.tsx` | JSX components + TS features |
| **HTML** | tree-sitter | `.html`, `.htm` | element IDs, script/style blocks |
| **CSS** | tree-sitter | `.css` | rules, media queries, keyframes |
| **YAML** | serde | `.yaml`, `.yml` | top-level keys, nested objects |
| **TOML** | serde | `.toml` | tables, arrays of tables |
| **JSON** | serde | `.json` | semantic keys (scripts, deps) |
| **Markdown** | structural | `.md` | headings as sections |
| **PDF** | pdf-extract | `.pdf` | pages as chunks |
| bash, sql, xml, c, cpp | plaintext | various | FTS-searchable |

---

## Integration with Other Tools

### Other AI Agents

rlm works with any agent that can execute shell commands or connect via MCP (stdio transport). See [Setup for AI Agents](#setup-for-ai-agents) for details.

### IDE Integration

rlm can complement your IDE's built-in features:

```bash
# Use rlm for cross-file analysis that IDEs struggle with
rlm impact Config    # What breaks if I change this?
rlm callgraph main   # Full call tree across modules
```

### CI/CD Pipelines

```bash
# Check for parse quality issues before merge
rlm quality --summary --exit-code

# Generate codebase overview for documentation
rlm map > docs/architecture.json
```

---

## Benchmarks

*Coming soon: Comparative analysis of token consumption and accuracy on real-world tasks.*

Preliminary results suggest 60-80% token reduction on typical code exploration tasks, with improved accuracy due to reduced context rot.

---

## The Research

rlm is inspired by the [Recursive Language Models](https://alexzhang13.github.io/blog/2025/rlm/) paper (MIT/Stanford, 2025), which demonstrated that:

1. **Progressive disclosure** beats full-context loading for code understanding
2. **AST-aware chunking** preserves semantic boundaries better than line-based splits
3. **Query-based retrieval** reduces context pollution and improves task accuracy

We've adapted these principles into a practical tool for everyday use with AI coding assistants.

---

## Roadmap

- [x] Core indexing and search
- [x] AST-based code intelligence (refs, callgraph, impact)
- [x] Surgical editing with Syntax Guard
- [x] MCP server integration
- [x] Parse quality detection and fallback recommendations
- [x] Configuration file support
- [x] Extended language support (JS, TS, HTML, CSS, YAML, TOML, JSON)
- [ ] Benchmark suite with published results
- [ ] Language Server Protocol (LSP) integration
- [ ] Web UI for visualization
- [ ] More languages (C++, Ruby, Kotlin)

---

## Philosophy

rlm is part of a larger vision: making powerful tools accessible to everyone, not just experts.

The same principle applies to AI coding assistants: they shouldn't require you to understand tokenization, context windows, or prompt engineering. They should just work efficiently.

> *"The best interface is no interface."* — Golden Krishna

---

## License

This project uses a **Source Available License**.

You are free to use the software (including binaries) and inspect the source code. However, modification, redistribution, and creating derivative works are not permitted.

See [LICENSE](LICENSE) for the full terms.

---

## Acknowledgments

- [Recursive Language Models](https://alexzhang13.github.io/blog/2025/rlm/) research (MIT/Stanford)
- [tree-sitter](https://tree-sitter.github.io/) for robust parsing
- The Rust community for excellent tooling
