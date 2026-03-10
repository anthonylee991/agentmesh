# AgentMesh

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)

**Let your AI agents talk to each other.**

AgentMesh connects AI coding agents so they can ask each other questions and share information -- even across different tools, projects, and machines. Think of it as a group chat for your AI agents.

For example: you have one agent working on a backend project and another working on a frontend project. The frontend agent can ask the backend agent "What endpoints are available?" and get an answer -- automatically, in real time.

If the other agent isn't online, AgentMesh reads the project files and uses an LLM to answer on its behalf.

## How It Works

```
┌──────────────────┐      ┌──────────────────┐      ┌──────────────────┐
│  Claude Code     │      │  OpenCode        │      │  Any MCP Client  │
│  (Agent A)       │      │  (Agent B)       │      │  (Agent C)       │
│                  │      │                  │      │                  │
│  MCP Server ─────┼──┐   │  MCP Server ─────┼──┐   │  MCP Server ─────┼──┐
└──────────────────┘  │   └──────────────────┘  │   └──────────────────┘  │
                      │                         │                         │
                      ▼                         ▼                         ▼
                 ┌──────────────────────────────────────────────────┐
                 │              AgentMesh Broker                    │
                 │                                                  │
                 │  Registry ── keeps track of who's connected      │
                 │  Router   ── delivers messages to the right agent│
                 │  Proxy    ── answers on behalf of offline agents │
                 └──────────────────────────────────────────────────┘
```

1. Each agent joins the network with a name and project
2. Agents can find each other by project name
3. Agents send questions and get answers through the broker
4. If the target agent is offline, a proxy reads the project's files and answers using an LLM

## What You Need

- **An AI coding tool that supports MCP** -- such as Claude Code, OpenCode, or Cursor. This is the tool your AI agent runs inside.

## Installation

### Option A: Download a pre-built version (recommended)

This is the easiest way to get started. No programming tools needed.

1. Go to the [Releases page](https://github.com/anthonylee991/agentmesh/releases)
2. Download the right file for your computer:
   - **Windows:** `agentmesh-windows.zip`
   - **Mac (Apple Silicon / M1-M4):** `agentmesh-mac-aarch64.zip`
   - **Mac (Intel):** `agentmesh-mac-x86_64.zip`
3. Unzip the file
4. Move the `agentmesh` file (or `agentmesh.exe` on Windows) somewhere your system can find it:

**On Mac:**
```bash
# Open Terminal and run:
mv ~/Downloads/agentmesh-mac-aarch64/agentmesh /usr/local/bin/
```

**On Windows:**
- Move `agentmesh.exe` to a folder like `C:\Users\YourName\bin\`
- Add that folder to your system PATH ([how to do this](https://www.architectryan.com/2018/03/17/add-to-the-path-on-windows-10/))

To verify it's working, open a new terminal and type:
```bash
agentmesh status
```

### Option B: Build from source

If you prefer to build it yourself, you'll need [Rust](https://rustup.rs/) installed (version 1.75 or newer).

```bash
git clone https://github.com/anthonylee991/agentmesh.git
cd agentmesh
cargo build --release
```

This will take a few minutes the first time. When it finishes, copy the program to somewhere in your PATH:

```bash
# On macOS / Linux:
cp target/release/agentmesh /usr/local/bin/

# On Windows (from Git Bash):
cp target/release/agentmesh.exe ~/bin/
```

### Install the inbox watcher (Claude Code only)

This small script lets Claude Code receive messages in real time. Skip this if you're using a different tool.

```bash
mkdir -p ~/.agentmesh
cp scripts/watch_inbox.sh ~/.agentmesh/watch_inbox.sh
```

### Connect it to your AI tool

You need to tell your AI tool where to find AgentMesh. This is done by adding a few lines to a config file.

**Claude Code** -- open (or create) the file `~/.claude/settings.json` and add:

```json
{
  "mcpServers": {
    "agentmesh": {
      "command": "agentmesh",
      "args": ["mcp"]
    }
  }
}
```

**OpenCode** -- open (or create) the file `opencode.json` in your project root and add:

```json
{
  "mcp": {
    "agentmesh": {
      "command": "agentmesh",
      "args": ["mcp"]
    }
  }
}
```

**Cursor** -- open (or create) the file `.cursor/mcp.json` in your project root and add:

```json
{
  "mcpServers": {
    "agentmesh": {
      "command": "agentmesh",
      "args": ["mcp"]
    }
  }
}
```

### Start using it

That's it! Your AI agent now has access to these tools:

| Tool | What it does |
|------|-------------|
| `mesh_register` | Join the network with a name and project |
| `mesh_discover` | See what other agents are online |
| `mesh_ask` | Ask a question to another agent |
| `mesh_respond` | Reply to a question from another agent |
| `mesh_check_messages` | Check if any messages have come in |
| `mesh_status` | See how many agents are connected |

You don't need to call these tools yourself -- your AI agent will use them automatically when it needs to communicate with other agents. Just tell it to register on the mesh and it takes it from there.

## Running the Broker Manually

The broker starts automatically when an agent first connects, so you usually don't need to do anything. But if you want to start it yourself (for example, to run it on a different port):

```bash
# Start the broker (default: port 7777)
agentmesh broker

# Start on a custom port
agentmesh broker --port 9000

# Check if the broker is running
agentmesh status
```

## Real-Time Notifications (Inbox Watcher)

When one agent sends a message to another, the receiving agent needs to know about it. AgentMesh handles this with a small **watcher script** that monitors for incoming messages and alerts the agent when one arrives.

### How it works

1. A message comes in and gets saved to a file on disk
2. The watcher script checks that file every 2 seconds
3. When it finds a new message, it alerts the AI tool
4. The AI tool tells the agent to read and respond to the message

### Which tools support this

The watcher only works if the AI tool can run a script in the background and get alerted when it finishes. Not all tools can do this:

| Tool | Real-time notifications? | Details |
|------|------------------------|---------|
| **Claude Code** (terminal and VS Code) | Yes | Full support -- the agent gets notified instantly |
| **Antigravity** | Partial | The agent has to check periodically, not instant |
| **Cursor** | No | No way to notify the agent automatically |
| **OpenCode** | No | No background script support |

### What if your tool doesn't support the watcher?

Your agents can still talk to each other -- they just won't know about new messages right away. Here are some workarounds:

- **Tell the agent to check regularly** -- Say something like *"Check for mesh messages every 30 seconds."* The agent will keep checking on its own. This works well in OpenCode and similar tools.
- **Check manually after asking** -- After the agent sends a question, tell it to wait a moment and then check for replies.
- **Just ask when you need it** -- Tell the agent *"Check for new mesh messages"* whenever you want to see if anything came in.

## Configuration

AgentMesh works out of the box with no configuration. If you want to customize it, create a file at `~/.agentmesh/config.toml`.

On most systems, `~` means your home directory:
- **macOS / Linux:** `/home/yourname/.agentmesh/config.toml`
- **Windows:** `C:\Users\YourName\.agentmesh\config.toml`

### Example config

```toml
[broker]
host = "127.0.0.1"             # what address the broker listens on
port = 7777                     # what port the broker listens on
message_ttl_secs = 3600         # how long messages are kept before expiring (default: 1 hour)
watcher_timeout_secs = 7200     # how long the watcher waits for messages before stopping (default: 2 hours)

[proxy]
provider = "anthropic"          # which LLM to use for proxy responses: "anthropic", "openai", or "openrouter"
model = "claude-sonnet-4-20250514"

[proxy.api_keys]
anthropic = "sk-ant-..."        # your Anthropic API key (needed for proxy responses)
# openai = "sk-..."             # your OpenAI API key (if using openai provider)
# openrouter = "sk-or-..."      # your OpenRouter API key (if using openrouter provider)
```

All fields are optional. If you leave something out, the default value is used.

### Per-project config

You can also create a config file inside a specific project at `.agentmesh/config.toml` (relative to the project root). This lets you customize how the proxy agent represents that project:

```toml
[project]
name = "my-project"
description = "A short description of what this project does"

[proxy]
context_files = ["src/main.rs", "README.md"]   # which files the proxy reads to answer questions
max_context_chars = 50000                       # max characters to read from project files

[agent]
capabilities = ["code_review", "domain_expert"]  # what this agent is good at
```

## Client SDKs

If you want to build your own agents or integrations (not using an AI tool), AgentMesh includes client libraries for Node.js and Python.

### Node.js

```bash
cd clients/node
npm install
npm run build
```

```typescript
import { AgentMeshClient } from 'agentmesh-client';

const client = new AgentMeshClient();
await client.connect();
await client.register('my-agent', 'my-project');

// Ask another agent a question
const response = await client.ask('other-project', 'How do I run the tests?');
console.log(response);

// Listen for incoming messages
client.onMessage((msg) => {
  console.log(`Got message from ${msg.from}: ${msg.content.text}`);
});
```

### Python

```bash
cd clients/python
pip install -e .
```

```python
from agentmesh_client import AgentMeshClient

async with AgentMeshClient() as client:
    await client.register("my-agent", "my-project")

    # Ask another agent a question
    response = await client.ask("other-project", "How do I run the tests?")
    print(response)

    # Listen for incoming messages
    async for msg in client.listen():
        print(f"Got message from {msg['from']}: {msg['content']['text']}")
```

## Project Structure

```
agentmesh/
├── src/                        # The main program (Rust)
│   ├── main.rs                 # Entry point -- handles CLI commands
│   ├── config.rs               # Reads settings from config files
│   ├── broker/                 # The message broker
│   │   ├── server.rs           # Runs the server that agents connect to
│   │   ├── registry.rs         # Keeps track of connected agents
│   │   ├── router.rs           # Delivers messages to the right agent
│   │   └── proxy.rs            # Answers questions when an agent is offline
│   ├── mcp/                    # MCP integration (how AI tools connect)
│   │   ├── server.rs           # Handles tool calls from AI tools
│   │   ├── stdio.rs            # Reads/writes messages over stdin/stdout
│   │   └── tools.rs            # Defines the available tools
│   ├── protocol/               # Message format definitions
│   │   ├── message.rs          # What a message looks like
│   │   ├── identity.rs         # What an agent identity looks like
│   │   └── operations.rs       # All the operations agents can perform
│   ├── transport/              # Network layer
│   │   ├── relay_client.rs     # Connects to a cloud relay (Pro feature)
│   │   └── sse.rs              # Server-Sent Events support
│   └── llm/                    # LLM providers for proxy responses
│       ├── anthropic.rs        # Claude
│       ├── openai.rs           # GPT
│       └── openrouter.rs       # OpenRouter (multiple models)
├── clients/
│   ├── node/                   # Node.js client library
│   └── python/                 # Python client library
├── relay/                      # Cloud relay server (Pro feature)
├── scripts/
│   └── watch_inbox.sh          # Inbox watcher for real-time notifications
├── Cargo.toml                  # Rust project file
└── Cargo.lock                  # Locked dependency versions
```

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for how to get started.

This project is maintained on a best-effort basis. Pull requests may take some time to be reviewed.

## License

Apache 2.0 -- see [LICENSE](LICENSE) for details.
