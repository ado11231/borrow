# borrow

Run heavy work on another machine, from a light one.

Stay in your normal environment, meaning your editor, terminal, and browser, but run the
heavy parts of development on a stronger box. Builds, servers, databases, containers, and
models all go somewhere else. Your files stay where they are. No VM, and no remote desktop.

```bash
$ borrow run cargo build
▶ running on archbox
   Compiling borrow v0.1.0
    Finished dev profile in 2.04s
```

## Quick start

On the machine with the resources:

```bash
$ borrow serve
✓ ssh server running
✓ sshfs present
✓ rsync present

borrow is serving archbox

  on the other machine, run:

      borrow link 10.0.0.19:7433:K7QW9ZR2
```

On the machine you work from, paste that command:

```bash
$ borrow link 10.0.0.19:7433:K7QW9ZR2
✓ paired with archbox
  key       ~/.ssh/borrow_ed25519
  installed ado@10.0.0.19:~/.ssh/authorized_keys
  host keys ~/.config/borrow/known_hosts (3 learned)
```

That is the whole setup. No ssh config to edit, no keys to paste.

## Commands

| Command | What it does |
| --- | --- |
| `borrow serve` | Agent: start the daemon and print a pairing code |
| `borrow link <code>` | Client: pair with a box and remember it |
| `borrow run <cmd>` | Run a command on the box and stream the output back |
| `borrow info` | What the box is: cpu, memory, disk, gpu, tooling |
| `borrow health` | What the box is doing right now |
| `borrow unlink` | Remove borrow's key from the box and forget it |

Add `--agent <name>` to pick a box when you have more than one.

## How it works

Two machines. The **Client** is where you type, and it stays light. The **Agent** is where
the resources are, and it does the work.

A small daemon on the Agent owns identity, pairing, and health. Execution itself rides on
ssh, so there is no new protocol to trust and no custom execution channel to audit.

Your source will stay on the Client and be mounted onto the Agent. Build artifacts such as
`target/` and `node_modules/` live on the Agent's local disk, because mounting those is what
makes remote development slow. Every remote command says where it ran.

## Security

borrow installs an ssh key on somebody's personal desktop, so the rules are strict.

* Pairing codes are single use and expire in ten minutes. They are printed once, to the
  owner's own console, and never written to a file or logged.
* borrow uses its own named key rather than your personal one, so you can find it in
  `authorized_keys` and `borrow unlink` can take it back out.
* The daemon binds to loopback plus your local network. Never `0.0.0.0`.
* Private keys are never copied between machines.
* Remote arguments are quoted, never concatenated into a shell. There are tests for that.
* The box's ssh host keys travel over the pairing exchange, so the first connection is
  trusted without anybody being asked to eyeball a fingerprint.

## Status

**Phase 1 is complete.** Pair two machines on a LAN, run commands on the box with live
output and real exit codes, and see what the box is and what it is doing.

Not there yet: there is no mount, so commands run in the login directory on the box rather
than in your project. That is Phase 2 and it is the next thing being built. Cross network
use arrives in Phase 4. Until then both machines need to be on the same network.

## Building

```bash
cargo build
cargo run -- --help
cargo test
```

Requires a Rust toolchain on both machines, and an ssh server on the Agent.

## Documentation

`docs/GUIDE.md` is the full reference: architecture, design decisions, the security model,
the phase roadmap, and the machine setup. `CLAUDE.md` is the rulebook for AI coding agents
working in this repo.

## License

TBD
