# Contributing to AgentMesh

Thank you for your interest in contributing to AgentMesh.

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/agentmesh.git`
3. Create a branch: `git checkout -b my-feature`
4. Make your changes
5. Run the build: `cargo build`
6. Submit a pull request

## Development Setup

### Prerequisites

- Rust 1.75+
- Node.js 18+ (for relay and Node client)
- Python 3.9+ (for Python client)

### Building

```bash
# Build the Rust binary
cargo build

# Build the Node client
cd clients/node && npm install && npm run build

# Build the relay server
cd relay && npm install && npm run build
```

### Running locally

```bash
# Start the broker
cargo run -- broker

# In another terminal, start the MCP server
cargo run -- mcp
```

## Guidelines

- Keep PRs focused on a single change
- Add tests for new functionality when possible
- Follow existing code style and conventions
- Update documentation for user-facing changes

## Reporting Issues

Please open a GitHub issue with:
- A clear description of the problem
- Steps to reproduce
- Expected vs actual behavior
- Your environment (OS, Rust version, AI tool being used)

## License

By contributing, you agree that your contributions will be licensed under the Apache 2.0 License.
