# borrow

Run heavy work on another machine, from a light one.

Stay in your normal environment (editor, terminal, browser) but run the *heavy* parts of
development on a beefier box: builds, servers, databases, containers, models. Your files
stay where they are. No VM, no remote desktop.

```bash
borrow run cargo build
▶ running on archbox
   Compiling ...
```

## How it works

Two machines:

* **Client**, where you type. Stays light.
* **Agent**, where the resources are. Does the work.

A small daemon on the Agent handles pairing, health, and sessions. The actual execution
rides on ssh, so there is no new protocol to trust.

Your source stays on the Client and is mounted onto the Agent. Build artifacts such as
`target/`, `node_modules/` and virtualenvs live on the Agent's local disk. Mounting those
is what makes remote development slow, so borrow keeps them off the mount automatically.

## Status

**Early. Not usable yet.** Building Phase 1: run a command on a paired machine and stream
the output back.

* [x] CLI skeleton
* [ ] `borrow run`, execute remotely, stream output, pass exit codes through
* [ ] `borrow link`, pair two machines
* [ ] `borrow info` and `borrow health`, see the box's CPU, RAM, GPU
* [ ] Mount and artifact split
* [ ] Warm sessions, `ps` and `stop`
* [ ] Works across networks, not just a LAN

## Building

```bash
cargo build
cargo run -- --help
```

Requires Rust and an ssh connection to the machine you want to borrow.

## License

TBD
