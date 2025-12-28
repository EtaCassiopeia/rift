# rift-tui

Interactive Terminal User Interface for [Rift HTTP Proxy](https://github.com/EtaCassiopeia/rift).

## Features

- **Imposter Management** - View, create, edit, and delete imposters
- **Stub Editor** - JSON editor with syntax highlighting and validation
- **Search & Filter** - Find imposters and stubs quickly
- **Import/Export** - Load and save imposter configurations
- **Curl Generation** - Generate curl commands for testing stubs
- **Metrics Dashboard** - View request counts and statistics
- **Vim-style Navigation** - Navigate with j/k keys

## Installation

### From Source

```bash
cargo install rift-tui
```

### Build from Repository

```bash
git clone https://github.com/EtaCassiopeia/rift.git
cd rift
cargo build --release --bin rift-tui
```

## Usage

```bash
# Connect to default admin URL (http://localhost:2525)
rift-tui

# Connect to a different admin URL
rift-tui --admin-url http://localhost:2525
```

## Keyboard Shortcuts

### Navigation

| Key | Action |
|:----|:-------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Enter` | Select / Drill down |
| `Esc` | Go back / Close |
| `Tab` | Switch panes |
| `/` | Search |
| `?` | Help |
| `q` | Quit |

### Imposter List

| Key | Action |
|:----|:-------|
| `n` | New imposter |
| `p` | New proxy imposter |
| `d` | Delete imposter |
| `t` | Toggle enable/disable |
| `i` / `I` | Import file / folder |
| `e` / `E` | Export file / folder |

### Stub Management

| Key | Action |
|:----|:-------|
| `a` | Add stub |
| `e` | Edit stub |
| `d` | Delete stub |
| `y` | Copy as curl |

### Editor

| Key | Action |
|:----|:-------|
| `Ctrl+S` | Save |
| `Ctrl+F` | Format JSON |
| `Ctrl+A` | Select all |
| `Ctrl+C/X/V` | Copy/Cut/Paste |
| `Esc` | Cancel |

## Documentation

Full documentation available at [etacassiopeia.github.io/rift/features/tui](https://etacassiopeia.github.io/rift/features/tui/).

## License

Apache-2.0 - see [LICENSE](../../LICENSE) for details.
