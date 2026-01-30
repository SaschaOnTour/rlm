# Sample Documentation

This is a sample markdown file for testing the heading-based parser.

## Installation

To install the project:

```bash
cargo install rlm-cli
```

## Usage

### Basic Commands

Run the indexer:

```bash
rlm index .
```

### Advanced Commands

Search for symbols:

```bash
rlm search "Config"
```

## Configuration

The configuration file lives at `.rlm/config.toml`.

### Options

| Option | Default | Description |
|--------|---------|-------------|
| auto_index | true | Auto-index on first query |
| strict_mode | true | Enable syntax guard |

## FAQ

**Q: Does it support TypeScript?**
A: Not yet in the MVP. Planned for v2.

**Q: Where is the index stored?**
A: In `.rlm/index.db` relative to the project root.
