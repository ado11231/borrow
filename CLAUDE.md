# CLAUDE.md

Rules for AI coding agents working in this repo, whether that is Claude Code, Codex, or
anything else. Read this before making any change.

**These rules override default behaviour. Follow them exactly.**

The full reference lives in `docs/GUIDE.md`: architecture, roadmap, security model, and the
real machine setup. This file is the short version. If the two ever disagree, the guide
wins and this file needs fixing.

---

## What this project is

**slingshot** lets a light machine use the RAM, CPU, and GPU of a heavy machine, without
leaving the light machine.

The user stays in their normal environment, meaning their editor, terminal, and browser,
but the heavy parts of development run somewhere else. Builds, servers, databases,
containers, coding agents, and small AI models all go to the powerful box. Heavy work never
touches the light machine. Files are edited on the light machine, and the powerful machine
works on a filtered copy that slingshot keeps in step. It has to feel local. **No VM, and no
remote desktop.**

* **Goal:** a public open source tool that anyone can install and set up in minutes. This is
  not a personal script that happens to be on GitHub. `start` and `link` are the product for
  every user who is not the author. Setup friction is a bug, not something to explain away
  in documentation.
* **Name:** Slingshot, and the command is `slingshot`. It was called borrow until
  September 22, 2026.
* **Language:** Rust.
* **Two roles:** the **Client** is the machine you work from. The **Agent** is the machine
  with the resources. Across networks the two meet through **iroh**, which we wrap rather
  than build, and which is Phase 4 work.

---

## Golden rules, do not violate these

1. **The Client stays light.** Never move heavy work onto the Client. If a change makes the
   Client do real work, the change is wrong.
2. **Always report where things run.** Every remotely executed command must clearly print
   its location, for example `▶ Running on archbox`. This is a hard requirement and not
   cosmetic.
3. **Do not rebuild existing systems.** We wrap `ssh`, `rsync`, `tmux`, `docker`, `ollama`,
   `nvidia-smi`, and the `iroh` library. Do not write a custom SSH, file transfer, terminal
   multiplexer, container engine, inference engine, relay server, or NAT traversal.
4. **Keep setup dead simple.** One binary per machine, one pairing step, one command to use.
   Reject designs that add setup friction.
5. **Stay platform generic.** Model everything as Client and Agent, never as Mac and Linux.
   Mac to Linux and Linux to Linux must both work. Only the menu bar helper is Mac specific.
6. **Assume a stranger, not the author.** Anything that is easy because it is the author's
   own machine becomes a preflight check or an installer step. Every failed check prints the
   exact command that fixes it. Detect and instruct, never auto install.
7. **Nothing may be a prerequisite that a stranger would not already have.** A mesh VPN such
   as Tailscale is a detected fast path, never a requirement. iroh is the answer to "works
   from anywhere", and it needs no account, no server, and no open router port.

---

## Hard do not list

* Do **not** implement RAM pooling or network memory. It is physically unworkable, because
  a network is roughly a hundred thousand times slower than RAM. Reject any request for it
  and explain why.
* Do **not** add remote desktop or GUI streaming features. That is out of scope and already
  solved by Sunshine and Moonlight.
* Do **not** copy or mount build artifacts. That means `target/`, `node_modules/`, virtual
  environments, and caches. They live in separate Agent storage. Syncing them destroys
  performance, and this is the single most important performance rule in the project.
* Do **not** treat environment files as source. `.env`, `.env.*`, `*.env`, and `.envrc`
  never enter manifests, and their contents never travel as command arguments.
* Do **not** hardcode a package manager. The target is Arch, which uses `pacman`, not `apt`.
  Prefer detecting and instructing over auto installing.
* Do **not** hardcode OS specific paths such as `/Users/...` or `/home/...` in shared code.

---

## Architecture, the short version

```
CLIENT ──── ssh, over the first path that answers ────► AGENT
type commands    local network, tailnet, or iroh        runs the real work,
see output                                              keeps project copies
```

**Paths, tried in order.** The Client tries the Agent's saved local address, then its
tailnet address, then iroh, and uses the first that answers. `run` and `attach` say
which one they used, as in `▶ Running on archbox via tailnet`. iroh dials the Agent by
its public key, punches through NAT to connect directly when it can, and falls back to a
public relay when it cannot. Either way ssh runs inside, so a relay only ever carries
encrypted bytes.

**Transport, already decided.** The **daemon is the control plane**, owning identity,
pairing, job records, health, sessions, sync leases, and reachability. **ssh is the data
plane**, so `run`, `attach`, and rsync shell out to it. Do not write a custom execution
channel. The user never types an ssh command and never edits `~/.ssh/config`.

After pairing, every structured request goes through a hidden `slingshot internal-control`
helper started over ssh, which relays to a private Agent Unix socket. The TCP port only
pairs. Never add creation, stop, or data operations to the TCP endpoint.

**Security, built in from Phase 1 and never retrofitted.** Pairing codes are short lived and
single use. The daemon binds to loopback plus the LAN, never `0.0.0.0`. Installed keys are
named so they can be revoked. `slingshot unlink` works. Remote arguments are always quoted and
never concatenated into a shell. Control messages carry a version, a size limit, and
timeouts. The Agent OS account is the trust boundary. Never signal a process by PID alone;
match its start time too. The Agent accepts iroh connections only from Clients it paired
with, and forwards them only to its own sshd. Relays can read nothing.

* The **Client** exposes `localhost` ports that forward to the Agent.
* The **Agent** runs work inside the project copy and dials outward to connect.
* **Pairing** still happens on the same network or tailnet. Pairing across networks is later
  work.

**Files.** Source is edited on the Client. The Agent keeps a filtered copy per project and
Agent, updated with rsync through three way sync against a shared baseline. Conflicts stop
the sync and are never resolved automatically. Build output goes to separate Agent storage,
which is called the artifact split. Environment files are stored outside source. SSHFS is
no longer used for execution. Full detail is in `docs/GUIDE.md`.

---

## Repo layout

**The workspace split is done**, as the first task of Phase 2.

```
crates/
├── slingshot-core/     lib: config, control, protocol, source, sync, storage, artifacts,
│                    stack, telemetry, preflight, presentation, keys
├── slingshot-agent/    lib: pairing daemon, service, projects, jobs, runner
└── slingshot-cli/      bin `slingshot`: client, transfer, project, live, ssh, keys, commands/
```

**Three crates, one binary.** `slingshot-core` and `slingshot-agent` are libraries; `slingshot-cli`
produces the only executable, named `slingshot` via `[[bin]]`. The point of the split is the
crate boundary, which makes the compiler refuse a cycle, not separate executables. Splitting
those is a Phase 7 distribution decision.

Dependencies point one way only: `slingshot-cli` uses both, `slingshot-agent` uses core, and core
uses nothing of ours. If either side ever needs something from the other, the thing belongs
in core instead. That is how `keys::marker`, `keys::client_name`, `storage::short_id`,
`sync::merge`, and `source::environment_target` ended up there: each is a rule both machines
have to apply the same way.

**End state, from Phase 2 onward:**

```
slingshot/
├── crates/
│   ├── slingshot-core/        # shared: protocol, config, source, sync, stack detect, telemetry, error
│   ├── slingshot-cli/         # the slingshot command, on the Client
│   └── slingshot-agent/       # the Linux daemon, a systemd service
├── deploy/                 # systemd unit and installer
├── mac/menubar/            # Mac only menu bar and notifications, Phase 5
└── docs/
```

Shared types, especially the wire **protocol**, **source eligibility**, **sync**, and the
**artifact split** logic, live in `slingshot-core` and are used by every binary. Define them once.

---

## Commands, the target user experience

```bash
slingshot start            # Agent: start the daemon, print a pairing code
slingshot link <code>      # Client: connect and remember the box
slingshot run <cmd>        # sync the project, run on the box, stream output back
slingshot attach [path]    # create or rejoin the project's tmux session on the box
slingshot sync [path]      # push changes; --pull retrieves, --check previews
slingshot env add|list|remove   # environment files kept outside source
slingshot ps [--all]       # Slingshot runs and sessions, with recent history
slingshot stop <id>        # graceful stop, then kill after five seconds
slingshot info             # static specs of the box, cached
slingshot health           # live snapshot; --watch refreshes every two seconds
slingshot top              # live resources and active jobs
```

Common stacks, meaning Rust, Node, and Python, must work with **zero configuration** by
detecting `Cargo.toml`, `package.json`, and similar, then applying the right artifact split
automatically. A small `slingshot.toml` can add `sync.exclude` patterns. Split overrides are
planned.

---

## Build order

1. **Phase 1.** `run` plus pairing, on the LAN only, plus `info`. This is the spine.
2. **Phase 2.** Artifact split with stack detection. Its SSHFS mount was later replaced.
3. **Phase 3.** Source copies and sync, `attach`, `env`, `ps` and `stop`, live `health` and `top`.
4. **Phase 4.** Works from anywhere: path selection, then iroh for cross network use, with
   NAT traversal and relay fallback included.
5. **Phase 5.** Menu bar with live readout, notifications, automatic port forwarding.
6. **Phase 6.** Wrap Ollama and ComfyUI, pairing across networks, self hosted relays, Linux
   to Linux hardening.
7. **Phase 7.** Ship it: static binaries, installer, packages, `slingshot doctor`, README.

Each phase must leave a working, usable tool. Phase 4 gates publishing, because before it
"works from anywhere" means "set up a VPN first". Full detail is in `docs/GUIDE.md`.

---

## Current state

**Phases 1 and 3 are complete. Phase 4 is in acceptance testing. Phase 5 is in progress:
the menu bar app and notifications are built, port forwarding is not.** Phase 3 replaced Phase 2's SSHFS execution with filtered
source copies and kept its stack detection and artifact split.

The rename from borrow to Slingshot changed folders, the ssh key name, and the iroh protocol
name with no migration, so both machines must run `slingshot link` again after updating.

Pairing creates only Client to Agent SSH trust and records the Agent's Slingshot path.
`slingshot unlink` removes the installed key, this Client's environment files, and the learned
host keys, keeping source copies and backups.

**The Phase 2 mount surface is gone.** Its fields were deleted from config and from the
pairing message, so a config file written before that no longer loads and the machines must
run `slingshot link` again. The control protocol is at version 5, raised when unlink began
naming the Client so the Agent can forget its iroh key.

CLI output uses shared formatting in `slingshot-core/src/presentation.rs` and
`slingshot-core/src/step.rs`. Anything that waits is a step: a spinner on a terminal, then
`✓ text  0.4s`. Status symbols are `✓`, `!`, `✗`, and `▶`. Color is for what needs attention:
symbols, the box and path in `▶` lines, and warning or failed values. Healthy values stay
plain, headings are bold, and details are dim. Messages use sentence capitalization. CPU,
RAM, GPU, and VRAM labels use uppercase. Global `--color auto|always|never` controls
Slingshot output. Put Slingshot options before `run`; later arguments belong to the remote
command.

The menu bar app in `mac/menubar/` is SwiftUI and only draws what the hidden
`slingshot internal-watch` prints as JSON lines. Keep thresholds and notification rules in
`slingshot-cli/src/watch/`, never in Swift. `slingshot menubar` opens it, and
`mac/menubar/build.sh` builds and installs it.

The current suite contains 196 passing tests, and formatting and Clippy pass. The Phase 3
flows were accepted on a real Mac Client and Arch Linux Agent, including a network drop
mid build, a daemon restart, and an Agent reboot. Phase 4 was checked on the same two
machines: `run` via the local network, the tailnet, and iroh from a phone hotspot, the
failure message with `start` stopped, and the awake lock.

**Not yet true, do not claim otherwise.** File watchers inside sessions, several Clients
sharing one Agent account, and large Node and Python projects are untested. `slingshot.toml`
split overrides are parsed but never applied; only `sync.exclude` is read. The Phase 3
flows, meaning sync, attach, a network drop, and a reboot, have not been re-run over iroh.
Pairing across networks does not exist.

The detailed file reference and completion record live in `docs/PROJECT_STATUS.md`.

## Conventions

* **Errors.** The current crates use `anyhow`. Typed shared errors remain a future improvement. A non zero remote exit
  code is not an error. slingshot did its job and the command it ran happened to fail.
* **Async.** `tokio`. The tool is concurrent by nature, handling connections, streaming, and
  watching processes, so expect async everywhere past Phase 1.
* **Config and wire messages.** `serde`, with `toml` and `serde_json`.
* **Logging.** `tracing`.
* **CLI.** `clap`.
* **Telemetry.** `sysinfo` for CPU, RAM, and disk. `nvidia-smi` for GPU.
* **Comments.** Keep necessary doc comments above items. Explain important decisions in plain language.
  Avoid em dashes and comments that repeat obvious code. Keep function bodies free of comments.
* **Versions.** Always check the current version on crates.io. Never assume.

---

## When unsure

* Prefer the smallest change that keeps the current phase working end to end.
* If a request conflicts with the golden rules or the hard do not list, say so plainly and
  propose an in scope alternative.
* If a request needs a new dependency, name it, say why, and prefer wrapping an existing
  tool over reimplementing one.
* Do not claim something works when it has not been run. Report what was actually verified.
