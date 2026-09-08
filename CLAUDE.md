# CLAUDE.md

Rules for AI coding agents working in this repo, whether that is Claude Code, Codex, or
anything else. Read this before making any change.

**These rules override default behaviour. Follow them exactly.**

The full reference lives in `docs/GUIDE.md`: architecture, roadmap, security model, and the
real machine setup. This file is the short version. If the two ever disagree, the guide
wins and this file needs fixing.

---

## What this project is

**borrow** lets a light machine use the RAM, CPU, and GPU of a heavy machine, without
leaving the light machine.

The user stays in their normal environment, meaning their editor, terminal, and browser,
but the heavy parts of development run somewhere else. Builds, servers, databases,
containers, coding agents, and small AI models all go to the powerful box. Heavy work never
touches the light machine. Files live on the light machine, and the powerful machine reaches
them over a mount. It has to feel local. **No VM, and no remote desktop.**

* **Goal:** a public open source tool that anyone can install and set up in minutes. This is
  not a personal script that happens to be on GitHub. `serve` and `link` are the product for
  every user who is not the author. Setup friction is a bug, not something to explain away
  in documentation.
* **Working name:** `borrow`. It may change.
* **Language:** Rust.
* **Two roles:** the **Client** is the machine you work from. The **Agent** is the machine
  with the resources. A third tiny piece, the **Coordinator**, exists only for cross network
  use and is Phase 4 work.

---

## Golden rules, do not violate these

1. **The Client stays light.** Never move heavy work onto the Client. If a change makes the
   Client do real work, the change is wrong.
2. **Always report where things run.** Every remotely executed command must clearly print
   its location, for example `▶ running on archbox`. This is a hard requirement and not
   cosmetic.
3. **Do not rebuild existing systems.** We wrap `ssh`, `sshfs` or NFS, `docker`, `ollama`,
   and `nvidia-smi`. Do not write a custom SSH, filesystem, container engine, or inference
   engine.
4. **Keep setup dead simple.** One binary per machine, one pairing step, one command to use.
   Reject designs that add setup friction.
5. **Stay platform generic.** Model everything as Client and Agent, never as Mac and Linux.
   Mac to Linux and Linux to Linux must both work. Only the menu bar helper is Mac specific.
6. **Assume a stranger, not the author.** Anything that is easy because it is the author's
   own machine becomes a preflight check or an installer step. Every failed check prints the
   exact command that fixes it. Detect and instruct, never auto install.
7. **Nothing may be a prerequisite that a stranger would not already have.** A mesh VPN such
   as Tailscale is a detected fast path, never a requirement. The Coordinator is the answer
   to "works from anywhere".

---

## Hard do not list

* Do **not** implement RAM pooling or network memory. It is physically unworkable, because
  a network is roughly a hundred thousand times slower than RAM. Reject any request for it
  and explain why.
* Do **not** add remote desktop or GUI streaming features. That is out of scope and already
  solved by Sunshine and Moonlight.
* Do **not** mount build artifacts. That means `target/`, `node_modules/`, virtual
  environments, and caches. They live on Agent local disk. Mounting them destroys
  performance, and this is the single most important performance rule in the project.
* Do **not** hardcode a package manager. The target is Arch, which uses `pacman`, not `apt`.
  Prefer detecting and instructing over auto installing.
* Do **not** hardcode OS specific paths such as `/Users/...` or `/home/...` in shared code.

---

## Architecture, the short version

```
CLIENT ──► COORDINATOR (tiny, cross network only) ──► AGENT
type commands   introduces and relays                 runs the real work,
see output      the two machines                      holds the mount
```

**Transport, already decided.** The **daemon is the control plane**, owning identity,
pairing, the process table, health, sessions, and reachability. **ssh is the data plane**,
so `run` and `attach` shell out to it. Do not write a custom execution channel. The user
never types an ssh command and never edits `~/.ssh/config`. The daemon does that for them.

**Security, built in from Phase 1 and never retrofitted.** Pairing codes are short lived and
single use. The daemon binds to loopback plus the LAN, never `0.0.0.0`. Installed keys are
named so they can be revoked. `borrow unlink` works. Remote arguments are never string
concatenated into a shell. The Coordinator relays an encrypted stream and can read nothing.

* The **Client** exposes `localhost` ports that forward to the Agent.
* The **Agent** runs work inside the mounted project directory and dials outward to connect.
* The **Coordinator** is only needed across networks and is not used on the same LAN.

**Files.** Source lives on the Client and is mounted onto the Agent, using SSHFS first.
Build output is redirected to Agent local disk. This is called the artifact split. Full
detail is in `docs/GUIDE.md`.

---

## Repo layout

**The workspace split is done**, as the first task of Phase 2.

```
crates/
├── borrow-core/     lib: protocol, config, telemetry, preflight, keys::marker
├── borrow-agent/    lib: the daemon
└── borrow-cli/      bin `borrow`: ssh, client, keys, commands/
```

**Three crates, one binary.** `borrow-core` and `borrow-agent` are libraries; `borrow-cli`
produces the only executable, named `borrow` via `[[bin]]`. The point of the split is the
crate boundary, which makes the compiler refuse a cycle, not separate executables. Splitting
those is a Phase 7 distribution decision.

Dependencies point one way only: `borrow-cli` uses both, `borrow-agent` uses core, and core
uses nothing of ours. If the agent ever needs something from the cli, the thing belongs in
core instead. That is how `keys::marker` ended up there, since both sides have to agree on
how an installed key is labelled.

**End state, from Phase 2 onward:**

```
borrow/
├── crates/
│   ├── borrow-core/        # shared: protocol, config, mount, stack detect, process, telemetry, error
│   ├── borrow-cli/         # the borrow command, on the Client
│   ├── borrow-agent/       # the Linux daemon, a systemd service
│   └── borrow-coordinator/ # tiny cross network server, Phase 4
├── deploy/                 # systemd unit and installer
├── mac/menubar/            # Mac only menu bar and notifications, Phase 5
└── docs/
```

Shared types, especially the wire **protocol** and the **mount and artifact split** logic,
live in `borrow-core` and are used by every binary. Define them once.

---

## Commands, the target user experience

```bash
borrow serve            # Agent: start the daemon, print a pairing code
borrow link <code>      # Client: connect and remember the box
borrow run <cmd>        # run a command on the box, stream output back
borrow attach <proj>    # drop into a warm session on the box, in the project, mounted
borrow ps               # what is running remotely and where to reach it
borrow stop <id>        # stop a remote process
borrow info             # static specs of the box, cached
borrow health           # live snapshot: CPU, RAM, GPU, disk
borrow top              # live continuously updating resource view
```

Common stacks, meaning Rust, Node, and Python, must work with **zero configuration** by
detecting `Cargo.toml`, `package.json`, and similar, then applying the right artifact split
automatically. Per project overrides go in a small `borrow.toml`.

---

## Build order

1. **Phase 1.** `run` plus pairing, on the LAN only, plus `info`. This is the spine.
2. **Phase 2.** Mount and artifact split with stack detection. This is make or break.
3. **Phase 3.** `attach`, warm sessions, `ps` and `stop`, live `health` and `top`.
4. **Phase 4.** Coordinator and relay tunnel for cross network use. This is the hardest code.
5. **Phase 5.** Menu bar with live readout, notifications, automatic port forwarding.
6. **Phase 6.** Wrap Ollama and ComfyUI, NAT hole punching, Linux to Linux hardening.
7. **Phase 7.** Ship it: static binaries, installer, packages, `borrow doctor`, README.

Each phase must leave a working, usable tool. Phase 4 gates publishing, because before it
"works from anywhere" means "set up a VPN first". Full detail is in `docs/GUIDE.md`.

---

## Current state

**Phase 1 is complete. Phase 2 is the current phase.**

Phase 1 works on a LAN: `serve` with preflight checks and a single use pairing code, `link`
which installs borrow's own named key and learns the box's host keys, `run` with live
streaming, real exit codes and a ctrl-c that stops the remote command, `info` from cache or
refreshed, `health` live, `unlink`, and `--agent` selection. 29 tests.

**Phase 2 is the mount and the artifact split**, and it is what makes the tool worth using.
In order: split the workspace, then `stack.rs` detection, then `mount.rs` split rules, then
the reverse trust, then the SSHFS mount, then wire it into `run`. The first three need no
second machine and are fully unit testable.

Three things to know before starting Phase 2:

* **`cwd` and `env` on `RemoteCommand` are the waiting slots.** They will carry the mounted
  project path and the artifact split variables. `command_line` will need to emit
  `cd <cwd> && env KEY=VAL ... <command>`, with every piece quoted by the same
  `shell_words` path that already protects the arguments.
* **SSHFS is a pull, so the mount runs on the Agent and connects back to the Client.** That
  is the reverse of every connection borrow makes today. Phase 1 `link` installs the
  Client's key on the Agent. Phase 2 needs the mirror image as well: the Agent's public key
  on the Client, and the Client's host key known to the Agent. Two independent one way
  trusts, and neither private key is ever copied.
* **The Client sshd preflight warning becomes a failure.** `link` already warns when this
  machine has no ssh server. In Phase 2 there is no mount without one.

One deviation from `docs/GUIDE.md` worth knowing: `link` does not write a `~/.ssh/config`
entry. Everything ssh needs lives in borrow's own config and is passed on the command line
instead, so borrow never edits files you maintain by hand.

## Conventions

* **Errors.** `anyhow` in the binaries, `thiserror` in `borrow-core`. A non zero remote exit
  code is not an error. borrow did its job and the command it ran happened to fail.
* **Async.** `tokio`. The tool is concurrent by nature, handling connections, streaming, and
  watching processes, so expect async everywhere past Phase 1.
* **Config and wire messages.** `serde`, with `toml` and `serde_json`.
* **Logging.** `tracing`.
* **CLI.** `clap`.
* **Telemetry.** `sysinfo` for CPU, RAM, and disk. `nvidia-smi` or `nvml-wrapper` for GPU.
* **Comments.** Doc comments above items only. Keep function bodies free of comments.
* **Versions.** Always check the current version on crates.io. Never assume.

---

## When unsure

* Prefer the smallest change that keeps the current phase working end to end.
* If a request conflicts with the golden rules or the hard do not list, say so plainly and
  propose an in scope alternative.
* If a request needs a new dependency, name it, say why, and prefer wrapping an existing
  tool over reimplementing one.
* Do not claim something works when it has not been run. Report what was actually verified.
