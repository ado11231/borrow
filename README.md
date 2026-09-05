# borrow

Run heavy work on another machine, from a light one.

Stay in your normal environment, meaning your editor, terminal, and browser, but run the
heavy parts of development on a stronger box. Builds, servers, databases, containers, and
models all go somewhere else. Your files stay where they are. No VM, and no remote desktop.

```bash
borrow run cargo build
▶ running on archbox
   Compiling ...
```

## How it works

Two machines. The **Client** is where you type, and it stays light. The **Agent** is where
the resources are, and it does the work.

A small daemon on the Agent handles pairing, health, and sessions. Execution itself rides on
ssh, so there is no new protocol to trust.

Your source stays on the Client and is mounted onto the Agent. Build artifacts such as
`target/` and `node_modules/` live on the Agent's local disk, because mounting those is what
makes remote development slow. Every remote command says where it ran.

## Status

**Early. Not usable yet.** Partway through Phase 1. `borrow run` currently builds the ssh
invocation and prints it rather than executing it.

## Building

```bash
cargo build
cargo run -- --help
cargo test
```

Requires a Rust toolchain, and an ssh connection to the machine you want to borrow.

## Documentation

`docs/GUIDE.md` is the full reference: architecture, design decisions, the security model,
the phase roadmap, and the machine setup. `CLAUDE.md` is the rulebook for AI coding agents
working in this repo.

## License

TBD
