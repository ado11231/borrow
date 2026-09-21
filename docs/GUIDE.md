# borrow: Project Guide

Welcome. This is the single document to read before you touch the code. It explains what
borrow is, who it is for, how the system is put together, why it is put together that way,
and where the project is on its road to being publishable.

This guide is the whole reference. Architecture, roadmap, and the real machine setup all
live here, so there is one place to read and one place to update.

The only other document is `CLAUDE.md` at the repo root, the short rulebook that AI coding
agents must follow. It is a summary of this guide, not a separate source of truth. If the
two ever disagree, this guide wins and `CLAUDE.md` needs fixing.

**Contents**

1. Project Overview
2. Architecture and Design
3. Project Phases and Roadmap
4. Setup and Operations Reference
5. Working on the Project

**A note on section 4.** It contains real machine details: hostnames, tailnet addresses, and
key fingerprints. Before this repo is made public, that section needs rewriting as generic
instructions rather than publishing as it stands.

---

# 1. Project Overview

## The short version

**borrow lets a light machine use the RAM, CPU, and GPU of a heavy machine, without
leaving the light machine.**

You keep working exactly where you already work: your own editor, your own terminal, your
own browser. The heavy parts of development go somewhere else. Builds, servers, databases,
containers, coding agents, and small AI models all run on the powerful box. You keep
editing your own files, and borrow copies eligible source to the box when work needs it.

```bash
borrow run cargo build
▶ Running on archbox
   Compiling ...
```

That arrow is not decoration. Announcing where a command ran is a hard rule of the
project, because a tool that silently moves your work to another machine is a tool you
cannot trust.

## The problem it solves

Picture a very common setup. You have a laptop you love using: quiet, portable, good
screen, all your settings. You also have a desktop that is far more powerful and mostly
sits idle.

Today you have three bad options.

* **Do everything on the laptop.** Builds crawl. Fans scream. You run out of memory the
  moment you open a browser next to a compiler.
* **SSH into the desktop and live there.** Now you have lost your editor, your shell
  config, your clipboard, and your files. You are maintaining two environments instead
  of one.
* **Run a VM or a remote desktop.** Now you are looking at a video stream of a computer.
  It feels wrong immediately and it never stops feeling wrong.

borrow takes a fourth path. Your machine stays your machine. It becomes a very thin
control surface that builds commands, hands them to the powerful box, and streams the
output back. The powerful box does all the actual work on a filtered copy of your project
that borrow keeps in step with your machine.

## Who it is for

* Anyone with a light main machine and a stronger machine within reach, whether that is
  a desktop in the next room or a server in a rack.
* Developers whose builds, test suites, or containers are painfully slow locally.
* People running local AI models who want the GPU box to do it without moving their whole
  workflow onto that box.

An important framing that shapes every decision in this repo: **this is meant to be a
public open source tool that a stranger can install and configure in minutes.** It is not
a personal script that happens to live on GitHub. That single goal settles a lot of
arguments. Setup friction is treated as a bug rather than something to explain away in
documentation, and nothing may be required that a stranger would not already have.

## What it is deliberately not

Saying no clearly is part of the design.

* **Not RAM pooling.** You cannot borrow another machine's memory over a network. Network
  round trips are roughly a hundred thousand times slower than a memory access. Any design
  that pretends otherwise produces something unusably slow. Requests for this are refused
  on physics, not preference.
* **Not a remote desktop.** No GUI streaming, no video of another computer. Tools like
  Sunshine and Moonlight already do that well.
* **Not a new SSH, filesystem, container engine, or inference engine.** borrow wraps `ssh`,
  `rsync`, `tmux`, `docker`, `ollama`, and `nvidia-smi`. Writing our own versions of proven
  infrastructure would be both slower to build and much harder for anyone to trust.

## Vocabulary

Two words appear constantly, and the project is careful to use them instead of naming
operating systems.

* **Client.** The machine you work from. It stays light. My setup for this is a
  Mac with 8GB of RAM.
* **Agent.** The machine with the resources. It does the real work and keeps project copies. In
  my setup this is an Arch Linux desktop.

There is a third piece, the **Coordinator**, but it only exists to connect a Client and an
Agent that are on different networks. It is Phase 4 work and does not exist yet.

Thinking in Client and Agent rather than Mac and Linux is what makes Mac to Linux, Linux
to Linux, and even Linux to Mac fall out of the same code for free. The only genuinely
platform specific piece in the whole plan is the Mac menu bar helper.

## The golden rules

Every design decision in this repo is checked against these seven rules.

1. **The Client stays light.** If a change makes the Client do real work, the change is
   wrong.
2. **Always report where things run.** Every remotely executed command prints its location.
3. **Do not rebuild existing systems.** Wrap the proven tools.
4. **Keep setup dead simple.** One binary per machine, one pairing step, one command to use.
5. **Stay platform generic.** Model everything as Client and Agent.
6. **Assume a stranger, not the author.** Anything easy because it is your own machine
   becomes a preflight check or an installer step. Every failed check prints the exact
   command that fixes it. Detect and instruct, never auto install.
7. **Nothing may be a prerequisite a stranger would not already have.** A mesh VPN such as
   Tailscale is a detected fast path, never a requirement.

---

# 2. Architecture and Design

## The mental model in one sentence

The Client is a command builder and a pipe.

That really is the whole trick. borrow figures out what should run, where it should run,
and with which environment, then hands that off to ssh and forwards the resulting bytes
back to your terminal. Everything else in the architecture exists to make that one motion
feel effortless and safe.

## The three pieces

```
   ┌─────────────┐        ┌──────────────┐        ┌──────────────┐
   │   CLIENT    │        │ COORDINATOR  │        │    AGENT     │
   │             │◄──────►│  (Phase 4)   │◄──────►│              │
   └─────────────┘        └──────────────┘        └──────────────┘
   You type commands      Introduces the two      Runs the real work.
   here. Exposes          machines and relays     Keeps project copies.
   localhost ports        traffic when they       Reports specs and
   forwarding to          cannot connect          health. Dials
   the Agent.             directly.               outward to connect.
```

* **Client.** Runs your commands, shows output, and exposes `localhost` ports that quietly
  forward to the Agent. A dev server on the Agent's port 3000 should appear at
  `localhost:3000` on your machine.
* **Agent.** Does the real work, keeps project copies, reports specs and health, and dials
  outward rather than waiting for inbound connections. Dialing out is what gets through
  home routers and NAT without any port forwarding.
* **Coordinator.** Tiny, cheap, always on. Only needed when Client and Agent are on
  different networks. Not used at all on a shared home network.

## The central design decision: control plane versus data plane

This is the most important architectural choice in the project, so it gets the most space.

Two questions get conflated when people design a tool like this: **who decides**, and
**what carries the bytes**. borrow answers them separately.

* **The control plane is the borrow daemon.** It owns identity, pairing, the process table,
  health, sessions, and knowing how to reach the box. This is what a user installs and
  thinks of as "my connection to my machine."
* **The data plane is ssh.** It is the pipe the daemon hands you once the daemon has
  decided where and how.

The user never types an ssh command, never edits `~/.ssh/config`, and never learns a
hostname. The daemon did all of that for them. Yet borrow never wrote an encryption layer,
and every security auditor on earth already trusts the transport. For an open source tool
that asks strangers to run a daemon on their personal desktop, that second point matters
enormously.

This is the same shape as VS Code Remote SSH, and the same shape as Tailscale itself, which
is a coordination server plus WireGuard. It also follows directly from golden rule 3.

Concretely, two channels doing different jobs:

| Channel | Carries | Why this channel |
| --- | --- | --- |
| **ssh**, shelled out to | `run`, `attach`, and rsync transfers, anything that executes or copies bytes | Authentication, encryption, TTY handling, and live stdout and stderr streaming already work correctly. Rewriting them is the textbook definition of rebuilding an existing system. |
| **borrow daemon**, on a private Unix socket reached through ssh | `info`, `health`, sync leases, sessions, `ps`, `stop`, environment files, unlink cleanup | These want typed messages, not a text stream. A hidden helper started over ssh relays them to a socket only the Agent account can open, so every structured request is authenticated by ssh too. |
| **borrow daemon**, on its TCP port | Pairing only | A new Client has no key yet. The single use code is the only thing this port accepts. |

**Why not one custom channel for everything?** Because you would own message framing,
reconnection, backpressure, and remote process lifecycle *before your first command ever
runs*. With ssh, Phase 1's `run` is simply: build a command string, spawn it, forward
stdio. The daemon then starts small and grows only when there is a real reason.

The daemon grows on a schedule:

* **Phase 1.** Answered `info` and `health` over TCP. Read only, held no state.
* **Phase 3.** Moved every structured request behind ssh authentication, and now owns sync
  leases, persistent tmux sessions, job records, and environment files, so `ps` and `stop`
  are real and sessions survive a disconnect.
* **Phase 4.** Stops listening and starts dialing outward to the Coordinator, which is what
  makes cross network use possible through a home router.
* **Phase 7.** The very same daemon is what a stranger installs. `serve` and `link` are the
  entire setup experience.

## How a command flows

Walk through `borrow run cargo build` end to end.

1. You type `borrow run cargo build` on the Client.
2. The Client finds the project: the enclosing Git repository, or else the nearest project
   marker. It looks up the persistent project ID for this project and this Agent.
3. Over one ssh control connection the Client opens the project, takes a sync lease, and
   compares both manifests with the shared baseline. Changed files go through rsync into
   staging, and the Agent verifies and applies them. Nothing moves when nothing changed.
4. The Client asks for resource warnings and prints any.
5. The Client prints `▶ Running on archbox · app/src · target → local disk` and spawns
   `ssh archbox 'borrow internal-run --project <id> --cwd src -- cargo build'`.
6. The Agent runs the command in the matching folder of the source copy as its own process
   group, with build output redirected to separate Agent storage. Output streams back
   byte for byte and the exit code propagates.

Step 6 has two details that are easy to get wrong and important to get right. Output must
**stream** rather than buffer, or the tool feels frozen. And the exit code must
**propagate**, so `borrow run false` exits 1. Without that, borrow is useless inside scripts
and CI.

## Files: source copies and the artifact split

This is the make or break performance detail of the entire project.

* Your project's **source files live on the Client**. That is where you edit them.
* The Agent keeps a **filtered copy** of eligible source on its own disk, one copy per
  project and Agent. `borrow run` brings it up to date before every run. `borrow sync` pushes,
  `borrow sync --pull` retrieves edits made on the Agent, and `--check` previews either.
* **Build artifacts are never copied.** They are redirected to separate Agent storage.

```
   CLIENT (where you edit)                AGENT (does the work)
   ~/projects/myapp/                      <data>/agent/projects/<id>/
     ├── src/        ── rsync changes ──►   ├── source/        eligible files only
     ├── Cargo.toml                         ├── artifacts/     target, node_modules, venv
     ├── .env        ✗ never copied         ├── environment/   files from borrow env, 600
     └── target/     ✗ never copied         └── state/         baseline, journal, backups
```

`<data>` is Borrow's platform data directory, such as `~/.local/share/borrow` on Linux.

**Why copies replaced the Phase 2 mount.** SSHFS made the Agent dial back into the Client,
which required a second SSH trust and an SSH server on the Client. That broke golden rule 7
for most strangers. Every file operation also became a network round trip, stale mounts hung
builds, and file watchers missed events. A local copy gives native file speed on the Agent,
needs only Client to Agent SSH, and survives sleep and network loss.

**Eligible source.** The same rules apply to copies, pulls, previews, and backups.

1. Git ignore rules from `.gitignore` files inside the project, even outside a Git
   repository, plus the repository's `.git/info/exclude` and the Client's global excludes.
   The Client sends these extra patterns so both machines decide identically. Ignored
   folders are pruned before descending, so a nested negation cannot bring a file back.
   Tracked files that match current rules are excluded too.
2. Version control internals such as `.git`.
3. Generated folders such as `node_modules`, `.venv`, `__pycache__`, and caches at any depth,
   and `target` beside a `Cargo.toml`.
4. `.env`, `.env.*`, `*.env`, and `.envrc` at every depth, including templates.
5. `sync.exclude` patterns from `borrow.toml`, which can add exclusions but cannot override
   the mandatory ones.

Relative symlinks are kept when their resolved target stays inside eligible source. A link
that escapes the project, or points at an excluded path, stops the sync with the path named.
A file that becomes ignored after it was copied keeps its last shared state, so changing
ignore rules never deletes or transfers anything by itself.

**Sync safety.** Each side lists its files with SHA256 hashes, executable bits, and link
targets. Borrow compares the sender, the receiver, and the last shared baseline:

1. Changed only on the sender: copied.
2. Changed only on the receiver: kept.
3. Changed differently on both: the sync stops before anything changes. There is no force
   option and no automatic resolution.
4. Deleted on the sender: deleted on the receiver only if it was previously synchronized.

rsync copies only changed regular files, named in an explicit list, into a staging folder
over ssh. The receiver checks every destination and every staged hash, writes a recovery
journal and backups of replaced files, applies the changes, and advances the baseline only
at the end. A failure rolls back. An interrupted sync is recovered before any new work, and
recovery stops with instructions when a path was edited after the interruption. The latest
20 backup sets per project are kept. The Agent never trusts the Client's manifest: it
enforces exclusions and link safety itself, and a pull only advances the baseline where both
copies actually agree.

Applying a sync requires the project to be idle. A run, a session, or another sync holds the
project, and new work waits for recovery.

**Environment files** stay out of source entirely. `borrow env add --file <local> --target
<path>` sends the contents inside the ssh control message, never as a command argument, and
stores them in a private folder with owner only permissions. The Agent exposes each one at
its target through a Borrow managed link. Replacing needs `--replace`, `list` shows names
only, and nothing keeps secret backups. The Agent account can read these files, and no
encryption at rest is promised.

### Stack detection

Zero configuration is the goal. Detect the stack, apply the right split, and say what you
did.

| Detected file | Split applied |
| --- | --- |
| `Cargo.toml` | `CARGO_TARGET_DIR` points at Agent local disk |
| `package.json` | `node_modules` linked to separate Agent storage |
| `pyproject.toml` or `requirements.txt` | Virtual environment and pip cache kept Agent local |
| Nothing detected, or an override | Whatever the project's `borrow.toml` says |

A small per project `borrow.toml` marks a project and can add `sync.exclude` patterns. Split
overrides are not applied yet.

## Terminal output

Borrow uses compact aligned rows. Labels and sentences are capitalized consistently.
CPU, RAM, GPU, and VRAM stay uppercase. Machine names, paths, and commands keep their
original spelling.

`--color auto|always|never` is a global option. Place it before `run` so it is not
forwarded to the remote program. Automatic mode checks each output stream separately
and disables colors for redirected output, a nonempty `NO_COLOR`, or `TERM=dumb`.
An explicit mode overrides those automatic choices. Help follows the same color choice.

Information and health results go to stdout. Progress, setup checks, and errors go to
stderr. Borrow does not change remote command output.

Health colors always include written status labels:

1. CPU and GPU usage: Light below 70 percent, Busy from 70 percent, High load from 90 percent.
2. RAM and VRAM usage: Available below 75 percent, Limited from 75 percent, Low free memory
   from 90 percent.
3. GPU temperature: Normal below 75°C, Warm from 75°C, Hot from 85°C. These are display
   guides, not device safety limits.
4. Disk: available space only. The health response does not provide live total capacity.

The three levels use green, yellow, and red. Missing or invalid measurements are
Unavailable. Memory uses MiB and GiB with one decimal place. High utilization describes
workload, not a failing machine.

Before a run or a new session, Borrow warns when RAM use is at or above 90 percent or when
the Agent workspace disk has less than 2 GiB free. Warnings never block the job, and a
failed measurement never blocks valid work.

## Seeing the box: specs and health

You are offloading work to a machine you cannot see, so "is it alive, and does it have room
for this?" becomes a constant question. Two kinds of data answer it, both served by the
daemon.

* **Specs, which are static.** What the box *is*: CPU model and core count, total RAM, GPU
  model and VRAM, disk space, OS and kernel, and what tooling is available such as CUDA,
  ROCm, Docker, or a running Ollama. Fetched once at pairing time and cached on the Client.
* **Health, which is live.** What the box *is doing now*: CPU load, RAM used and free, VRAM
  used and free, free disk, free workspace disk, GPU temperature and utilization.

```bash
borrow info             # static specs, instant, from cache
borrow health           # live snapshot right now
borrow health --watch   # the same view, refreshed every two seconds
borrow top              # live resources plus active Borrow jobs
```

Live views need a terminal, exit on Q or Ctrl C, and restore the terminal on exit.
Collection is deliberately cheap and reuses existing tools. CPU, RAM, and disk come from
the `sysinfo` crate. NVIDIA GPU data comes from `nvidia-smi`. Jobs come from the daemon's
records.

This data becomes more than a readout in two places. First, a **preflight check** before a
large job can warn you: `⚠ archbox has 3GB free, this may struggle`. That turns data into
prevention. Second, the **menu bar live readout** in Phase 5 gives a passive glance value
line such as `archbox · 40% CPU · 12/32GB · GPU 60°C`.

## Networking, in order of difficulty

1. **Same network.** No Coordinator. The Client reaches the Agent directly by local IP,
   using ssh for work and the daemon port for structured data. Build this first.
2. **Relay through the Coordinator.** The Agent dials out to the Coordinator, the Client
   connects to it, and all traffic relays through. This always works. Build this second.
3. **Direct connection with hole punching.** Try to connect the two machines directly, and
   fall back to relay if it fails. Lower latency and cheaper to run. Build this last.

| Situation | Path taken |
| --- | --- |
| Same local network | Direct local IP |
| Mesh VPN present on both machines | Direct over the tailnet, no Coordinator |
| Neither | Coordinator relay, then hole punch if possible |

The Client tries these in order and always says which one it used.

**The Coordinator is not an optimization, it is the connection story.** For a public tool,
"works from anywhere" cannot mean "first go set up a VPN." A mesh VPN is a supported fast
path: detect it, use it when present, skip the relay entirely. Anyone who has one gets
lower latency for free. Anyone who does not still gets a working tool.

## Security model

borrow installs ssh keys and runs arbitrary commands on somebody's personal desktop. For an
open source tool that is a serious responsibility. These are design constraints from Phase
1, because they are cheap to build in now and painful to retrofit later.

**Pairing**

* The pairing code is short lived, on the order of minutes, and single use. It is a bearer
  token: whoever holds it can install a key.
* `serve` prints the code once, to the console of the machine's owner. It is never written
  to a file, never logged, never transmitted anywhere.
* Pairing installs exactly one named public key into `authorized_keys`, clearly marked as
  belonging to borrow, so a human can find it and revoke it by hand.
* `link` reports exactly what it did: which key, which file, which host.

**Daemon exposure**

* The TCP port binds to loopback plus the local network. Never `0.0.0.0` on a public
  interface, and never a port opened to the internet. It accepts only pairing, reads at
  most 64 KiB, and times out after ten seconds.
* Everything else goes to a private Unix socket in the Agent's data directory, mode 600,
  checked against the connecting account. The only way to it from another machine is a
  hidden helper started through an authenticated ssh login.
* Control messages carry a protocol version, are limited to 16 MiB, and have first
  request, idle, and write timeouts. A mismatched version gets a message to update both
  machines.
* The trust boundary is the Agent's operating system account. Clients sharing that account
  can see each other's Borrow jobs and projects. Cleanup is still scoped to the projects each
  Client registered.
* Cross network reachability comes from the Agent dialing out, not from an inbound port.
  This is also why it works behind home routers with no configuration.

**Keys**

* borrow never generates a key silently. If it creates one, it says so and says where.
* It never copies a private key between machines, under any circumstance.
* New pairings create only Client to Agent trust. The Client needs no SSH server.
* `borrow unlink` removes the key from the Agent and this Client's environment files, keeps
  source copies and backups, and refuses while that Client's projects have active work.

**Jobs**

* Runs and sessions are recorded on disk and reconciled after a daemon restart or reboot.
* A process is signalled only while its PID and start time both still match the record.
* `borrow stop` sends a graceful signal, waits five seconds, then kills what remains of
  that process tree.
* Sessions run on a Borrow owned tmux server with its own socket, so personal tmux sessions
  are never touched.

**Execution**

* The Agent runs work as an ordinary user, never as root. The systemd unit reflects that.
* Remote command arguments are passed safely and are never string concatenated into a
  shell.

**The Coordinator can read nothing.** It relays an encrypted ssh stream. It cannot see
commands, output, or files. The README says this plainly, because "route my development
traffic through a stranger's server" is the very first objection anyone will raise.

## Rust design notes

### Why Rust at all

Three properties matter here and Rust has all of them together. A single static binary with
no runtime to install, which makes the installer story trivial. Predictable low resource use
on the Client, which is the whole premise. And a strong concurrency story, which this tool
needs everywhere: streaming two output pipes while watching for a Ctrl-C, holding a daemon
connection open, polling health while a build runs.

### Memory and ownership in practice

borrow is not a memory intensive program, and that is by design. The Client's job is to
shuttle bytes, not to hold them.

The practical consequences you will see in the code:

* **Stream, never accumulate.** Remote output is forwarded to the terminal as it arrives.
  A `cargo build` on a large project can emit a great deal of text, and none of it should
  ever sit in a Client side buffer. This is a memory decision and a user experience
  decision at the same time, since buffering makes the tool feel hung.
* **Owned `String` at struct boundaries.** `RemoteCommand` in `src/ssh.rs` holds owned
  `String` and `Vec<String>` fields rather than borrowed slices. The struct is built in one
  place and consumed in another, is tiny, and is created once per invocation. Adding
  lifetime parameters to save a handful of allocations would buy nothing and would make
  every caller harder to write. Reach for borrows where data is large or hot; own it where
  the code reads better.
* **Borrow inside the hot path.** Within `to_ssh_args`, the iteration over program and
  arguments works entirely in `&str`, and only the final joined command becomes a new
  `String`. Own at the edges, borrow in the middle.

### Concurrency with tokio

The runtime is `tokio`, entered with `#[tokio::main]` on `main`, and since Phase 3 nearly
everything past argument parsing is async: the control connection, the transfers, the live
views, and the Agent's socket service.

`run` is the clearest example of why. To run a command remotely you must simultaneously
forward stdout, forward stderr, watch for the process to exit, and watch for a local Ctrl-C
so you can kill the remote process rather than merely detaching from it. That is four concurrent concerns in one small
function. It is precisely a `tokio::select!` problem: small enough to understand fully,
real enough to teach the pattern, and the pattern gets reused everywhere later.

The dependency features in `Cargo.toml` are chosen for exactly that: `rt-multi-thread` and
`macros` for the runtime, `process` for spawning ssh, `io-util` for the streaming, and
`signal` for the Ctrl-C handling.

### Error handling

The convention is deliberately split by crate role.

* **`anyhow` in binaries.** The CLI's job when something fails is to print a good message
  and exit. It does not need callers to match on error variants. `anyhow::Result` plus the
  `?` operator plus context strings gives exactly that.
* **`anyhow` in `borrow-core` too, for now.** Typed library errors with `thiserror` remain
  planned rather than done. The two places that genuinely need matching already carry their
  own types and are downcast rather than string matched: `client::Refused` for an error the
  Agent reported, and `ssh::Disconnected` for a connection that dropped. That pattern is the
  model for widening this later.

There is a nice distinction visible in `commands/run.rs` already. Its signature is
`anyhow::Result<i32>`, and the doc comment explains why: a non zero exit code is **not** an
error. borrow did its job perfectly, the command it ran happened to fail. An `Err` is
reserved for borrow itself failing. Getting this boundary right is what makes the tool
compose properly in scripts.

`main` mirrors that split when it exits. `Ok(code)` becomes the process exit code, while
`Err(e)` prints `Error: {e:#}` to stderr and exits 1. The `{e:#}` is the alternate Display format
for an `anyhow::Error`, which renders the whole context chain on one line rather than only
the outermost message.

### Command line design with clap

The CLI is described as a type, and clap derives both the parser and `--help` from it. The
`Cli` struct holds a `Commands` enum with one variant per subcommand, each variant's fields
being that subcommand's arguments.

The one annotation worth understanding deeply sits on `run`:

```rust
#[arg(trailing_var_arg = true, allow_hyphen_values = true)]
cmd: Vec<String>,
```

`trailing_var_arg` tells clap to stop parsing for itself once it reaches `run`, and
`allow_hyphen_values` stops clap from treating a leading dash as one of its own flags.
Together they mean that in `borrow run cargo build --release`, the `--release` reaches
cargo untouched instead of being claimed by borrow. Without this, every remote tool's flags
would collide with borrow's own, and the tool would be unusable for anything nontrivial.

### Safe remote execution

This is where a small design choice enforces a security rule structurally rather than by
discipline.

`RemoteCommand::to_ssh_args` uses `shell_words::join` to quote the program and each
argument, then returns a `Vec<String>` of exactly two elements: the host, and one command
string. That returned vector is an argv, meant to be handed straight to a process spawner,
not pasted into a shell.

The reason this matters is that ssh takes your command and hands it to the remote user's
shell. Anything you fail to quote is interpreted over there. Quoting once, in one function,
is what makes the rule "never string concatenate remote arguments into a shell" a property
of the code rather than something a future contributor has to remember.

The test module in `src/ssh.rs` reads as a specification of that property. It pins down
five behaviours: nothing is quoted when quoting is unnecessary, spaces are preserved inside
a single argument, an embedded single quote survives, `$HOME` stays literal instead of
expanding remotely, and `a; whoami` stays a literal argument instead of becoming a second
command. That last test is the security test. It is the difference between a tool and a
remote code execution hole.

### Traits

The codebase currently uses traits lightly and intentionally so. `to_ssh_args` is an
inherent method rather than a trait method, because there is exactly one implementation and
no abstraction to justify yet.

The places where traits are genuinely earned are visible on the roadmap rather than in the
code:

* **`serde`'s `Serialize` and `Deserialize`** on the protocol types, so the wire format is
  defined once and used by both machines.
* **A transport abstraction in Phase 4**, once a connection can be a direct LAN socket, a
  tailnet connection, or a Coordinator relay. Three real implementations is when an
  abstraction becomes worth its cost.
* **A stack detector** in Phase 2, if the Rust, Node, and Python cases genuinely share a
  shape. If they do not, three plain functions and a match are better than a trait.

The guiding instinct: introduce the trait when the second and third implementations exist,
not when you imagine they might.

### Key crates

| Need | Crate | Status |
| --- | --- | --- |
| CLI parsing | `clap` with the derive feature | In use |
| Async runtime | `tokio` | In use |
| Application errors | `anyhow` | In use |
| Safe shell quoting | `shell-words` | In use |
| Library errors | `thiserror` | Planned |
| Config and wire messages | `serde` with `toml` and `serde_json` | In use |
| Logging | `tracing` | In use |
| Specs and health | `sysinfo`, plus `nvidia-smi` | In use |
| Git ignore matching | `ignore` | In use |
| Content hashes | `sha2` | In use |
| Live terminal views | `crossterm` | In use |
| Process groups and signals | `libc` | In use |
| QUIC transport | `quinn` | Phase 4 |
| TLS | `rustls` | Phase 4 |
| Notifications | `notify-rust` or `mac-notification-sys` | Phase 5 |
| Menu bar | `tray-icon`, with `tao` and `muda` | Phase 5 |

Always check the current version on crates.io before adding a dependency. Do not assume.

## Repository structure

### The rule

**Start as a single crate. Split into the workspace at Phase 2.**

The four crate workspace is the correct end state, but on day one every crate boundary is a
guess about code that has not been written yet. One crate lets you find the real seams
first. By Phase 2 they are obvious, because the protocol types and the artifact split logic
will visibly be used by both sides.

### Today: three crates, one binary

Done as the first task of Phase 2.

```
borrow/
├── Cargo.toml               # workspace root: member list and shared versions
└── crates/
    ├── borrow-core/src/     # config, control, protocol, source, sync, storage,
    │                        # artifacts, stack, telemetry, preflight, presentation, keys
    ├── borrow-agent/src/    # lib.rs pairing, service, projects, jobs, runner
    └── borrow-cli/src/      # main.rs, client, transfer, project, live, ssh, keys, commands/
```

**Three crates, but still one binary, and that is deliberate.** `borrow-core` and
`borrow-agent` are libraries. `borrow-cli` is the only package that produces an executable,
and `[[bin]] name = "borrow"` is what keeps `borrow serve` and `borrow run` working as
documented.

The benefit being bought here is the **crate boundary**, not separate executables. A
boundary is what makes the compiler refuse a cycle, so `borrow-agent` can never reach into
`borrow-cli`. That enforcement is identical whether the result is one binary or two.
Splitting the executables is a distribution decision, and it can wait for Phase 7 when the
installer and the systemd unit are being written. Splitting the crates is an architecture
decision, and it only gets more expensive with time.

One binary behaves as Client or as Agent depending on the subcommand you give it. This is
also why the tool stays platform generic almost for free, since it is the same binary on
both ends.

### End state: the workspace

```
borrow/
├── Cargo.toml                  # workspace root listing member crates
├── docs/
│
├── crates/
│   ├── borrow-core/            # shared library, used by every binary
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── protocol.rs     # pairing messages
│   │       ├── control.rs      # authenticated control messages
│   │       ├── config.rs       # config plus borrow.toml
│   │       ├── source.rs       # eligible source and manifests
│   │       ├── sync.rs         # three way sync, apply, and recovery
│   │       ├── artifacts.rs    # artifact split logic
│   │       ├── stack.rs        # detect the stack, pick the split
│   │       ├── process.rs      # spawning and streaming external commands
│   │       ├── telemetry.rs    # specs and health types and collection
│   │       └── error.rs        # shared error types
│   │
│   ├── borrow-cli/             # the borrow command on the Client
│   │   └── src/
│   │       ├── main.rs
│   │       └── commands/       # link, run, attach, sync, env, ps, stop, info, health, top
│   │
│   ├── borrow-agent/           # the daemon on the Agent, a library today
│   │   └── src/
│   │       ├── lib.rs          # borrow serve, called by the cli binary
│   │       ├── service.rs      # private control socket and ssh bridge
│   │       ├── projects.rs     # source copies, leases, environment files
│   │       ├── jobs.rs         # job records, tmux sessions, stopping
│   │       └── runner.rs       # foreground runs
│   │
│   └── borrow-coordinator/     # tiny always on server, cross network only
│       └── src/
│           ├── main.rs
│           ├── rendezvous.rs   # introduces Client and Agent
│           └── relay.rs        # relays traffic when direct fails
│
├── deploy/
│   ├── borrow-agent.service    # systemd unit
│   └── install.sh              # one line installer
│
└── mac/
    └── menubar/                # Mac only indicator and notifications
```

**Why `borrow-core` exists.** The message format must be byte identical on both machines.
The source eligibility, sync, and artifact split logic is used by both the agent and the CLI. Telemetry types
are shared between the agent that collects them and the CLI that displays them. Each of
those is defined exactly once.

## The command surface

```bash
# one time setup
borrow serve            # Agent: start the daemon, print a pairing code
borrow link <code>      # Client: connect and remember the box
borrow unlink           # Client: remove borrow's key from the Agent

# daily use
borrow run <cmd>        # sync the project, run on the box, stream output back
borrow attach [path]    # persistent session on the box, in the project copy
borrow sync [path]      # push source changes; --pull retrieves, --check previews
borrow env add|list|remove   # environment files kept outside source on the box
borrow ps [--all]       # active Borrow runs and sessions, or recent history too
borrow stop <id>        # stop a run or session

# see the box
borrow info             # static specs, cached
borrow health           # live snapshot; --watch keeps refreshing
borrow top              # live resources and active jobs
```

---

# 3. Project Phases and Roadmap

## Where things stand today

Phases 1 and 3 are complete. Phase 3 replaced Phase 2's SSHFS execution with
filtered source copies, keeping Phase 2's project detection and artifact split.

Implemented today:

1. `serve` checks for an SSH server, rsync, tmux, and GPU tooling, starts the private
   control socket, and prints a single use pairing code.
2. `link` installs the Client key on the Agent and records the Agent's Borrow path. The
   Client needs no SSH server.
3. `run` syncs eligible source, warns about resources, and runs in the matching folder of
   the Agent copy with terminal passthrough, exit codes, and cancellation.
4. `attach` copies on first use, then creates or rejoins one tmux session per project.
5. `sync` pushes, pulls with `--pull`, and previews with `--check`.
6. `env add`, `env list`, and `env remove` manage environment files outside source.
7. `ps`, `ps --all`, and `stop` manage runs and sessions from persistent records.
8. `info` and `health` use authenticated control. `health --watch` and `top` refresh live.
9. `unlink` removes this Client's environment files, key, and learned host keys.
10. `--agent` selects a machine. `--color auto|always|never` controls Borrow formatting.

Current verification, on September 20, 2026: 149 automated tests pass, and formatting
checks and Clippy pass. A loopback run on one Mac used a private unprivileged sshd, real
rsync, and real tmux. It covered pairing, copying with exclusions, links, executable bits,
unusual filenames, subfolder runs, pushes, pulls, receiver only edits, conflicts,
environment files, busy refusal, stop with grace and kill, Ctrl C and interactive input in a
terminal, lost connections, attach, detach, reattach, a daemon restart with a live session,
live views, and unlink.

Two machine acceptance, on September 13, 2026, ran the same flows between a Mac
Client and an Arch Linux Agent on the LAN with GNU rsync. A clean release build started
inside `attach` finished on its own while the Client was offline for four minutes, and
`attach` returned to the same session afterwards. A daemon restart kept the session, an
Agent reboot marked it Interrupted, and `unlink` refused while a session was active. The
first `run cargo build` took 9 seconds including the copy, and the next started in 1 second.

A cleanup on September 20, 2026 deleted the retired Phase 2 mount surface from config and
from the pairing message, raised the control protocol to version 4, removed dead code, gave
duplicated helpers one home each in `borrow-core`, and renamed the identifiers that meant
two different things. It also fixed four real defects: `authorized_keys` was rewritten by
truncating in place, sync recovery read permissions through a symlink, the partial file
prefix was written out twice, and `learn_host` and `forget_host` could disagree about the
port. None of it has run on the two real machines yet.

Still outstanding:

1. Re-pairing the two real machines, which the protocol bump now requires, and running the
   Phase 3 flows again against the cleaned up tree.
2. Confirming on the real machines that a lost connection and an NVIDIA driver mismatch
   now print Borrow's own messages. Both were fixed after acceptance.
3. File watchers inside sessions, multiple Clients sharing one Agent account, and large
   Node and Python projects.
4. Applying split overrides from `borrow.toml`. Only `sync.exclude` is read today.
5. Build the cross network Coordinator in Phase 4.

SSH host paths with spaces remain quoted, interactive commands request a terminal,
and password fallback stays disabled. Borrow stores SSH options in its own configuration
and does not write `~/.ssh/config`.

See `docs/PROJECT_STATUS.md` for the simple file reference and phase record.

## How the phases work

Each phase must leave a working, usable tool. Do not skip ahead, because later phases lean
on earlier ones. The guiding principle when unsure is always the smallest change that keeps
the current phase working end to end.

## Phase S: Setup, before any Rust

Getting the repo and both machines ready so that Phase 1 is pure coding.

**Status: complete.**

On the repo side: git initialised, `.gitignore` in place, cargo project created as a single
crate, README written, crate versions pinned from crates.io rather than guessed.

On the machines side: a reserved LAN IP for the Agent, a mesh VPN on both machines as a
personal shortcut rather than a product requirement, `sshd` running on the Agent,
passwordless ssh from Client to Agent, and crucially passwordless ssh from Agent back to
Client, which Phase 2 needs because the Agent pulls the SSHFS mount. Plus `sshfs` and a
Rust toolchain on both.

**Done when:** ssh works in both directions with no password, from a cold terminal.

## Phase 0: Prove the feel, no code

The cheapest phase and the one people skip. Its entire job is finding out whether the idea
is pleasant before wrapping it in Rust.

**Status: complete.**

The work: ssh into the Agent and run a real build, timing it against the same build on the
Client. Run a coding agent on the box by hand. SSHFS mount a project folder by hand. Build
it on the mount **with no artifact split**, so you feel exactly how slow that is. Build it
again with the target directory pointed at Agent local disk and compare. Start a dev server
on the Agent, forward the port, and open it on the Client.

**Done when:** you can state from direct experience how much faster the box is, and how bad
the unsplit mount is. Both numbers become the motivation for Phase 2 and the headline of
the README.

**Decision gate:** does this feel good? If the latency were unbearable, the workflow would
need fixing before automating it.

## Phase 1: run, pairing, and info. The spine

The demoable core, and the tokio learning vehicle.

**Status: complete.**

**To implement**

* `main.rs` with all five subcommands: `serve`, `link`, `run`, `info`, `health`.
* `config.rs` reading and writing `~/.config/borrow/config.toml`, holding box name, host or
  IP, ssh user, daemon port, and cached specs.
* `protocol.rs` with small serde derived request and response types.
* `telemetry.rs` collecting specs and health via `sysinfo`, shelling out to `nvidia-smi`
  for GPU data, and degrading gracefully when there is no GPU, which is the Linux to Linux
  case.
* `agent.rs` implementing `borrow serve`: print a pairing code, listen on the daemon port,
  answer info and health requests. Read only, no state yet.
* `ssh.rs` finished: spawn the built command with stdout and stderr streamed live, and
  return the real exit code.
* `commands/link.rs`: take the code, verify reachability, fetch specs once, save config.
* `commands/run.rs`: print the location line, stream, propagate the exit code.
* `commands/info.rs` and `commands/health.rs`: format the JSON as a readable table.

**Preflight checks, which are the installer**

This is not polish and it is not Phase 7 work. For every user who is not the author, these
checks *are* the setup experience. Each failure prints the exact command that fixes it, and
nothing is ever auto installed.

* `serve` checks: is sshd running, are `rsync` and `tmux` present, is GPU tooling available.
* `link` checks: can the host be reached, does a key exist and if one is generated is that
  said out loud, is the Client's own sshd enabled for the Phase 2 mount.
* Every failure is one line: `✗ sshfs not installed  →  sudo pacman -S sshfs`
* Every success is one line too, so a working setup visibly passes.
* `borrow unlink` gets built now. It is five lines here and an awkward retrofit later.

**Security work that lands in this phase**

Short lived single use pairing codes, a daemon bound to loopback plus LAN and never
`0.0.0.0`, a named and therefore revocable installed key, and remote arguments passed
safely rather than concatenated into a shell. The last of these is already true in the code
today.

**Things to watch out for**

* Exit codes must propagate. `borrow run false` has to exit 1 or the tool is useless in
  scripts.
* Ctrl-C must kill the remote process, not merely detach the local one.
* Stream, do not buffer. Output appearing only at the end makes `run` feel broken.
* Quote the remote command correctly. Arguments containing spaces are where this breaks
  first, which is why the tests exist already.

**Done when:** on the Client, `borrow run cargo build` builds a real project on the Agent
with live output, and `borrow info` prints the box's specs. LAN only, no mount yet.

**This phase is demoable.**

## Phase 2: Mount and artifact split. Make or break

Where the tool stops being a fancy ssh alias and starts being genuinely worth using.

**Status: superseded for execution.** Project detection and the artifact split remain in use.
Phase 3 replaced SSHFS execution and the reverse SSH trust with source copies, for the reasons
given under "Files: source copies and the artifact split". The notes below record the
Phase 2 design as it was built.

**Workspace split: complete.** Shared logic, the Agent service, and user commands now
live in three crates.

**Implemented**

* The workspace split into `borrow-core`, `borrow-cli`, and `borrow-agent`.
* `core/stack.rs` detecting `Cargo.toml`, `package.json`, and `pyproject.toml`.
* `core/mount.rs` holding the artifact split rules per stack: environment variables and
  symlinks.
* `link` extended to set up the **second trust**. Phase 1 builds only the Client to Agent
  direction. The mount needs the mirror image: the Agent's public key installed on the
  Client, and the Client's ssh host key learned by the Agent. The Client sshd check stops
  being a warning and becomes a failure.
* Mount setup lives in `core/mount.rs` and is executed on the Agent through SSH.
  It reuses healthy mounts and recreates stale ones.
* `run` now resolving the full chain: local project directory, to remote mount path, to
  split environment variables.
* Saying what it did: `▶ Running on archbox · /mnt/borrow/app · target → local disk`.

**Still outstanding:** Per project `borrow.toml` overrides and the acceptance checks below.

**Things to watch out for**

* A stale SSHFS mount hangs forever rather than returning an error. Detect this and remount.
* File watchers such as `cargo watch` and Vite HMR often do not receive inotify events over
  SSHFS. Find out early whether polling is needed.
* Never mount `target/`, `node_modules/`, or virtual environments. This is the entire point.

**The baseline to beat.** Measured on archbox before any mount existed, building this crate
from clean:

| Build | Wall | User | Sys |
| --- | --- | --- | --- |
| `cargo build --release`, native, from clean | 8.7s | 1m31s | 3.2s |
| `cargo build --release`, native, incremental | not measured yet | | |

The incremental number is still to be taken, and it is the more important of the two. Run
this on the Agent before the mount lands:

```bash
cd ~/borrow && touch src/main.rs && time cargo build --release
```

A clean build is CPU bound, which is the kindest case for a network filesystem. An
incremental build is dominated by filesystem latency, because cargo stats thousands of
files to work out what changed, and over SSHFS every one of those is a round trip. A clean
build going from 8.7s to 12s is tolerable. A two second incremental going to forty is not,
and that is the one that decides whether the tool is usable day to day.

**Watch sys time, not just wall time.** Over SSHFS every file operation becomes a network
round trip, so system time is the leading indicator that artifacts are landing on the mount.
A wall time that crept up is ambiguous. A sys time that went from 3 seconds to 30 says
exactly what went wrong.

**Done when:** a real build on the mount is roughly as fast as a native build on the Agent,
and the Client's fans stay off.

## Phase 3: Sessions, source copies, and live health

Where the daemon starts doing things ssh cannot.

**Status: complete. Accepted on a real Mac Client and Arch Linux Agent on September 13, 2026.**

**Implemented**

* Authenticated control through a hidden ssh helper and a private Agent socket, with
  protocol versions, size limits, and timeouts. `info` and `health` moved there.
* Filtered source copies with persistent project IDs, three way sync, staged application,
  backups, and recovery. `borrow run` syncs before every run.
* `borrow sync`, `borrow sync --pull`, and `--check` previews.
* `borrow attach [path]` creates or rejoins one tmux session per project on an isolated
  Borrow tmux server. Reattaching does not sync.
* Persistent job records reconciled after restarts, `borrow ps`, `borrow ps --all` with the
  latest 100 finished jobs, and `borrow stop` with a five second grace period.
* `borrow env` for environment files kept outside source.
* `borrow health --watch`, `borrow top`, and resource warnings before work starts.
* Pairing without reverse trust, and unlink cleanup scoped to one Client's projects.

**Watch out for:** do not write a terminal multiplexer. Wrap `tmux` on the Agent. Golden
rule 3 applies here more than anywhere. The same goes for file copying: wrap rsync.

**Done when:** you close the lid mid build, reopen, run `borrow attach`, and you are back in
it, on a real Client and Agent pair. Met: the Client dropped its network for four minutes
during a clean release build, and `attach` returned to the finished build.

At the end of this phase borrow is an impressive, shippable personal tool.

## Phase 4: Cross network tunnel

**This is the connection story, not an optimization.** Until it exists, "works from
anywhere" means "go set up a VPN first", which is precisely the friction this project
exists to remove. **This phase gates publishing.**

**Status: not started.**

* Build `borrow-coordinator`, starting with rendezvous only.
* Detect a mesh VPN and prefer a direct connection when one is present.
* The Client says which path it used: `▶ archbox · via relay` or `· direct`.
* The Agent dials outward to the Coordinator and holds the connection open.
* The Client connects through it and all traffic relays.
* Relay only to begin with. No hole punching yet.

This is the hardest code in the project. Take the time. Read the Tailscale NAT traversal
write up before starting.

## Phase 5: Polish

**Status: not started.**

* Menu bar indicator with a live health readout.
* Notifications for build finished, server up, job done, needs input.
* Automatic port forwarding, so the Agent's port 3000 appears at `localhost:3000`.

## Phase 6: Models, direct connections, reach

**Status: not started.**

* Wrap Ollama and ComfyUI as first class jobs, covering small LLMs, embeddings, Whisper,
  and image generation.
* NAT hole punching, direct first with relay fallback.
* Harden Linux to Linux and remove any remaining Mac only assumptions.

## Phase 7: Ship it

The phase that turns a working tool into one strangers actually use. None of it is hard,
and all of it is the difference between a repo and a project.

**Status: not started.**

* Cross compiled static binaries for macOS on arm64 and x86, and Linux on x86 and arm.
* A `curl | sh` installer that picks the right binary and sets up the systemd unit.
* An AUR package and a Homebrew tap.
* `borrow doctor`, one command that diagnoses a broken setup and names each fix. This is
  the single highest value thing you can give a stranger whose install did not work.
* A README with the pitch, a five minute quickstart, and an honest security section that
  states plainly that the Coordinator relays an encrypted stream and cannot read your code.
* A LICENSE, either MIT or Apache 2.0, plus CONTRIBUTING and issue templates.
* CI building and testing on both platforms, and releasing binaries on tag.

**Done when:** someone who has never seen the repo goes from `curl | sh` to a working
`borrow run` in under five minutes, without asking anyone anything.

**Test this for real.** Wipe a VM, follow your own README, and time it. Every question you
have to answer by hand is a bug in the setup.

## Milestones at a glance

| After phase | You have |
| --- | --- |
| S | Two machines that trust each other in both directions, and a repo ready to code in |
| 0 | Proof the idea feels good, and the numbers that justify it |
| 1 | A demoable run on the other machine tool, plus a specs view, on the LAN |
| 2 | Project detection and build output kept apart from source |
| 3 | Source copies, persistent sessions, job control, and live health |
| 4 | Works from anywhere, not just at home |
| 5 | Feels polished: visible status, notifications, automatic ports |
| 6 | Models, faster connections, more platforms |
| 7 | A project other people can install, trust, and contribute to |

## The biggest risks

Naming these honestly is more useful than pretending they are solved.

1. **Keeping two copies honest, in Phase 3.** Source copies are fast, but only safe if sync
   never loses an edit. Three way planning, conflict refusal, staged application, and
   recovery exist for exactly this, and they need real world testing.
2. **NAT traversal, in Phase 4.** Connecting two home machines across networks is fiddly.
   Doing relay only first avoids most of the pain.
3. **Scope creep.** This is not a remote desktop and not a RAM pooling tool. Stay in the
   lane of "run processes over there, files feel local."
4. **Splitting the crate too early.** Four crates on day one means four manifests and a set
   of visibility puzzles standing between you and your first working command.
5. **Setup friction, which is the public release risk.** The tool can be excellent and still
   fail if a stranger cannot get from `curl | sh` to a working `borrow run` in five minutes.
   `serve` and `link` are the product for everyone who is not the author, and every failed
   preflight check must print the exact command that fixes it.

## Which files each phase touches

| Phase | What you build | Main files |
| --- | --- | --- |
| **0** | Prove the feel by hand | no code |
| **1** | `run`, pairing on the LAN, `info` and `health` | single crate: `ssh.rs`, `protocol.rs`, `telemetry.rs`, `agent.rs`, `commands/` |
| **2** | Split to the workspace, then mount and artifact split | `core/mount.rs`, `core/stack.rs`, `core/config.rs`, `agent/mounts.rs` |
| **3** | Source copies, `sync`, `attach`, `env`, `ps` and `stop`, live `health` and `top` | `core/source.rs`, `core/sync.rs`, `core/control.rs`, `agent/service.rs`, `agent/projects.rs`, `agent/jobs.rs`, `agent/runner.rs`, `cli/transfer.rs`, `cli/commands/` |
| **4** | Cross network tunnel | the whole `borrow-coordinator` crate, plus networking in `core` |
| **5** | Menu bar with live readout, notifications, port forwarding | `mac/menubar/`, forwarding logic in `core` and `cli` |
| **6** | Models, hole punching, Linux to Linux | a new module wrapping Ollama and ComfyUI, `coordinator/relay.rs` |

---

# 4. Setup and Operations Reference

This section records what was done by hand to get a working Client and Agent pair, and just
as importantly, **what borrow should do instead** for someone who is not the author.

The rule when adding to this section: every manual step below is either a preflight check,
an installer line, or a bug report from a future user. Nothing here is trivia.

## The machine pair

| | Client | Agent |
| --- | --- | --- |
| Machine | MacBook Air, 8 GB | Arch desktop |
| Hostname | `ados-macbook-air` | `archbox` |
| User | `adnanalagic` | `ado` |
| Tailnet IP | `100.80.212.73` | `100.67.90.119` |
| Resources | n/a | 16 cores, 31 GB RAM with 26 free, RTX 4060 with 8 GB |
| Latency | n/a | roughly 4 ms over the tailnet |

Reachability today is provided by Tailscale. That is scaffolding, so that Phases 1 through 3
can be built while away from home. It is not the product's answer to reachability. The
Coordinator in Phase 4 is.

## The ssh trust model

**Since Phase 3, Borrow needs only the Client to Agent direction.** The Agent to Client trust
below was built by hand for the Phase 2 mount and is kept here as a record. Borrow no longer
creates it, and `borrow unlink` removes the Borrow key it once installed on the Client.

There were **two independent one way trusts**, not one shared credential. Each machine keeps
its own private key, and neither private key is ever copied anywhere.

```
   CLIENT (Mac)                                   AGENT (archbox)
   ados-macbook-air                               100.67.90.119
   100.80.212.73

   ~/.ssh/id_ed25519        ── proves identity ──►  ~/.ssh/authorized_keys
   SHA256:l9+J9XXQWis...                            (holds the Mac's public key)
   passphrase in macOS Keychain          [Phase 1: borrow run]

   ~/.ssh/authorized_keys   ◄── proves identity ──  ~/.ssh/id_ed25519
   (holds archbox's public key)                     SHA256:nc0ARi2XU4d...
                                                    passphrase in ssh-agent
                                       [Phase 2: the sshfs mount]
```

| | Client to Agent | Agent to Client |
| --- | --- | --- |
| Key | `~/.ssh/id_ed25519` on the Mac | `~/.ssh/id_ed25519` on archbox |
| Fingerprint | `SHA256:l9+J9XXQWisBZUjIUohBb7vty7rMvBpNPU9ZKCORUNQ` | `SHA256:nc0ARi2XU4dDHBXvn0HJivsD+YRHVbs0PjTX6E5bJEk` |
| Comment | `adoalagic0@gmail.com` | `ado@archbox` |
| Passphrase | yes, held in the macOS Keychain via `UseKeychain` | yes, held in the login `ssh-agent` |
| Works unattended | **yes**, it survives `BatchMode` | **no**, it needs an unlocked agent |
| Used by | Phase 1 `borrow run` | Phase 2 sshfs mount |

**The asymmetry is deliberate and load bearing.** The Client to Agent direction has to work
with no human present, which is exactly why the Keychain matters. The Agent to Client
direction only ever runs while a user is logged in, which is why a passphrase there is
fine, and which is also why mounts should follow sessions rather than boot. See the open
design questions below.

### Files on the Client

```
~/.ssh/
├── config              # the archbox Host block
├── id_ed25519          # private, 600, passphrase protected, never leaves this machine
├── id_ed25519.pub      # public, 644, safe to share
├── authorized_keys     # 600, holds archbox's public key, for Phase 2
└── known_hosts         # hosts this machine has accepted
```

Permissions matter here and they fail **silently** when wrong. `~/.ssh` must be `700`.
Private keys and `authorized_keys` must be `600`. The home directory must not be group
writable. When any of that is wrong, sshd refuses the key and tells you nothing useful.

### The ssh config block on the Client

```
Host archbox
    HostName 100.67.90.119
    User ado
    IdentityFile ~/.ssh/id_ed25519
    ServerAliveInterval 30
    ServerAliveCountMax 3
    AddKeysToAgent yes
    UseKeychain yes
```

`ServerAliveInterval` and `ServerAliveCountMax` are the travelling settings. Three missed
pings at 30 second intervals, so about 90 seconds, and ssh gives up rather than leaving a
zombie session behind after a café wifi drop. `AddKeysToAgent` together with `UseKeychain`
is what turns the passphrase into a once per boot event instead of a once per command one.

## Every command that was run

**On the Agent, Arch Linux:**

```bash
sudo systemctl enable --now sshd
sudo pacman -S --needed tailscale sshfs rsync
sudo systemctl enable --now tailscaled
sudo tailscale up                      # browser sign in
sudo hostnamectl set-hostname archbox
tailscale ip -4                        # gives 100.67.90.119
mkdir -p ~/.ssh && chmod 700 ~/.ssh
# the Client's key arrives here via ssh-copy-id
sudo nano /etc/ssh/sshd_config         # PasswordAuthentication no, PermitRootLogin no
sudo sshd -t && sudo systemctl restart sshd
sudo sshd -T | grep -Ei 'passwordauthentication|permitrootlogin'
ssh-keygen -t ed25519                  # archbox's own key, with a passphrase
ssh-copy-id adnanalagic@100.80.212.73
```

**On the Client, macOS:**

```bash
ssh-keygen -t ed25519 -C "adoalagic0@gmail.com"    # with a passphrase
brew install --cask tailscale                      # then sign in, same account
# System Settings, then General, then Sharing, then Remote Login: ON
ssh-copy-id archbox
ssh archbox                                        # accept the host key once
```

Remote Login on the Client was needed only for the Phase 2 mount. Borrow no longer needs it.
The Agent now needs `rsync`, and `tmux` for sessions, rather than `sshfs`.

### A discrepancy worth knowing about

The sshd hardening on the Agent was verified on 2026-09-02 and did not fully take. `sshd -T`
reports `passwordauthentication no`, which is the setting that matters, but it also reports
`permitrootlogin prohibit-password` rather than the `no` that was written into the config
file. The edit was overridden, most likely by a file under `/etc/ssh/sshd_config.d/`.

This is not a hole. With password authentication off globally and no root key installed,
root cannot authenticate by either method. To tidy it up:

```bash
grep -rn PermitRootLogin /etc/ssh/sshd_config /etc/ssh/sshd_config.d/
```

The reason it happened is worth internalising: **sshd reads included files first, and the
first value found for a keyword wins.** That is the reverse of nginx, systemd drop ins, and
CSS, all of which let the later value win.

### The verification that actually proves something

```bash
ssh -o BatchMode=yes archbox 'echo ok'
```

`BatchMode=yes` is the entire point. It fails if anything at all would prompt. `borrow run`
can never stop to ask a human for a passphrase, so this, and not "a key file exists", is
what `link` must check before it declares success.

### Restoring this on a new machine

1. Install Tailscale and sign into the same account.
2. Run `ssh-keygen -t ed25519` on the new Client.
3. Run `ssh-copy-id <user>@<agent-tailnet-ip>`. This needs password authentication
   temporarily re-enabled on the Agent, since it is currently off. Copy the key, then turn
   it off again.
4. Recreate the `Host` block shown above.
5. Verify with `ssh -o BatchMode=yes archbox 'echo ok'`.

Step 3 is exactly the chicken and egg problem that the **pairing code** exists to solve.
`borrow link` should carry a short lived token so that a new Client can install its key
without ever loosening the Agent's ssh configuration.

## Keeping the Agent reachable while away

`sshd` and `tailscaled` are both enabled, so they come back after a reboot with no action
needed. The tailnet IP is tied to the machine's identity rather than to a session. What
actually ruins a remote working day is this list:

* **Suspend.** GNOME idle suspend drops the box off the tailnet, and ssh simply hangs. Fixed
  with `gsettings set org.gnome.settings-daemon.plugins.power sleep-inactive-ac-type
  'nothing'` and `sudo systemctl mask sleep.target suspend.target hibernate.target
  hybrid-sleep.target`. Screen locking is harmless. Only suspend matters.
* **Tailscale key expiry.** Disable it for the Agent in the admin console, or the machine
  silently leaves the tailnet after roughly six months.
* **Wifi power management** idling the network card on an unattended box. Ethernet avoids
  this entirely.
* **The Agent to Client direction needed a desktop login,** because that passphrase was held
  by a login ssh-agent. This applied to the Phase 2 mount only.

The preflight to run before leaving the house:

```bash
ssh -o BatchMode=yes archbox 'uptime; echo READY'
```

### This is the hardest job `borrow doctor` has

Every single failure above looks identical from the Client's point of view. It looks like a
hang. The error message has to tell them apart, because each one has a completely different
fix.

| Symptom | Diagnosis |
| --- | --- |
| Tailnet IP responds to ping, ssh times out | The Agent is suspended, or sshd is down |
| No route at all | Tailscale is down on one end |
| Connects, but authentication fails | A key problem |

"Could not connect to archbox" helps nobody. Three sentences and three fixes do.

### Configuration intent versus configuration reality

`sshd -T` prints what sshd actually resolved, rather than what the file says, and that is
what caught the real discrepancy described above.

Steal the idea. When a project `borrow.toml` and the global configuration disagree, **print
which one won.** It is cheap to implement and it kills the entire "I changed it and nothing
happened" category of confusion.

## Bugs hit by hand, and what each one means for the tool

These four are the most valuable content in this section, because each one is a real user
experience failure caught before any user experienced it.

### 1. A pasted key lost its spaces, causing silent authentication failure

`authorized_keys` ended up containing `ssh-ed25519AAAAC3Nza...` with both separators gone,
100 bytes instead of 102. sshd skipped the malformed line and quietly fell back to password
authentication **with no error printed anywhere.** That cost a full debugging cycle for a
technical user who was following correct instructions.

* **`borrow link` must never ask a human to paste a key.** Read the local public key, append
  it over the wire, and verify by logging in. This is what `ssh-copy-id` already does.
* **`borrow doctor` should validate `authorized_keys` structurally:** three space separated
  fields, base64 that actually decodes, one key per line. That is about ten lines of Rust
  and it saves that entire cycle.

### 2. It fell back to password authentication without saying so

The connection worked, so everything looked fine. But it was password authentication, which
would have broken every unattended command later on.

* Never treat "connected" as success. Assert the **method**: non interactive publickey.

### 3. `hostname` was not installed on a clean Arch box

It lives in the `inetutils` package, and is not part of a base installation.

* Get the hostname from `sysinfo` rather than by shelling out. Every external binary is an
  undeclared dependency. `nvidia-smi` earns its place. `hostname` does not.

### 4. Truncating the file after installing the key

Running `ssh-copy-id` and then `> authorized_keys` produces an empty file. This was only
recoverable because password authentication was still enabled at the time.

* **`borrow link` must be idempotent and recoverable.** Users run it twice, out of order,
  and halfway through. Never remove the fallback path before the new one has been verified.

## What borrow automates: the contract

This table is the specification for `serve`, `link`, and `doctor`. The left column is what a
human had to do. The right column is what the tool must do instead.

| Manual step | What the tool does |
| --- | --- |
| Generate an ssh key | `link` generates one if missing, **and says so, and says where** |
| Install the key on the Agent | `link` does it over the wire. Never a paste. |
| Write the `~/.ssh/config` entry | `link` stores the options in Borrow's own config instead |
| Make sure sshd is running | `serve` checks, then prints `✗ sshd not running → sudo systemctl enable --now sshd` |
| Make sure `rsync` and `tmux` are present | `serve` checks and prints the install line for that machine. `link` warns when the Client lacks rsync. **Never auto installs.** |
| Turn on Remote Login on the Client | No longer needed |
| Pick a hostname or display name | Chosen at pairing and stored in config, never read from the machine |
| Harden sshd | `doctor` warns if password authentication is still enabled |
| Install Tailscale | Detect and use it if present. Never require it. |
| Handle key expiry and keepalives | Sane defaults baked in |

**On naming.** The box has three different names: its Tailscale name, its system hostname,
and its ssh alias. The `▶ Running on ...` line must use the one chosen at pairing and stored
in config. Otherwise "where did that actually run?" becomes confusing the moment those three
names drift apart.

## Open design questions

These came out of the manual setup and are not yet decided.

1. **Does the Agent restore mounts at boot, or only when a Client asks for one?**
   Resolved in Phase 3: there are no mounts. Source copies live on the Agent's disk and need
   no reverse connection.

2. **What exactly does the pairing code encode?**
   Currently leaning towards host, port, and a short lived single use token, which is
   exchanged for installing a named public key.

3. **Should there be a `borrow config --effective`?**
   `sshd -T` prints what sshd actually resolved rather than what the file says. When the CLI
   and the daemon disagree about an exclusion or an artifact split, the same idea would end
   a whole category of confused debugging.

## Agent facts worth designing around

Two properties of the specific Agent machine have real design consequences.

* **There is no swap.** `Swap: 0B`. Heavy builds hit OOM and get killed outright rather than
  gradually slowing down. This is why runs and new sessions warn at 90 percent RAM use.
* **The GPU is never fully free.** GNOME, Xwayland, and a browser hold roughly 500 MB of the
  8 GB at idle. `borrow info` must therefore report **available** VRAM, not total. Always
  report the number that would make somebody cancel a job.

---

# 5. Working on the Project

## Building and running

```bash
cargo build
cargo run -- --help
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

You need a Rust toolchain. Using a real Agent also needs ssh, rsync on both machines, and
tmux on the Agent for sessions.

## The full feature checklist

Everything the tool is meant to do eventually, tagged with the phase that delivers it.
Nothing here is checked off yet beyond what section 3 records as complete.

**Core, the reason it exists**

* Pair two machines once with a short code. *(P1)*
* Run any command on the box with live output streamed back. *(P1)*
* A clear "where it ran" confirmation on every remote command. *(P1)*
* Copy eligible project source to the box, and pull edits back safely. *(P3)*
* Artifact split: source copy and build output in separate Agent storage. *(P2)*
* Zero configuration stack detection for Rust, Node, and Python picks the right split. *(P2)*
* Per project `borrow.toml` exclusions *(P3)* and split overrides. *(planned)*
* Environment files kept outside source. *(P3)*

**Sessions and control**

* `attach` drops you into the project on the box. *(P3)*
* Warm sessions survive sleep and network changes, and reconnecting lands you mid task. *(P3)*
* `ps` and `stop` to see and manage Borrow runs and sessions. *(P3)*

**Visibility, seeing the box from the Client**

* `info`, static specs, fetched at pairing and cached. *(P1)*
* `health`, a live CPU, RAM, VRAM, disk, and GPU temperature snapshot. *(P1)*
* GPU detection and reporting, for example "RTX 4060, CUDA available". *(P1)*
* `top`, a live updating resource view. *(P3)*
* A preflight check that warns before a job if the box is low on RAM or workspace disk. *(P3)*

**Networking**

* Same network, no coordinator needed. *(P1)*
* Relay through a coordinator, which works across networks. *(P4)*
* Direct connection with hole punching, for lower latency. *(P6)*

**Polish, what makes it demo worthy**

* A menu bar indicator with a live health readout. *(P5)*
* Notifications for build done, dev server up, job finished, needs input. *(P5)*
* Automatic port forwarding, Agent port 3000 to Client `localhost:3000`. *(P5)*

**Models, the AI angle**

* Wrap Ollama for small LLMs, embeddings, and Whisper as a first class job. *(P6)*
* Wrap ComfyUI for image generation as a first class job. *(P6)*

**Reach**

* Linux to Linux works, not just Mac to Linux. *(P6)*

**Shipping it, which is what makes this a project rather than a script**

* Preflight checks in `serve` and `link` that name the fix. *(P1)*
* `borrow unlink`, clean removal of the installed key. *(P1)*
* Static binaries for macOS on arm64 and x86, and Linux on x86 and arm. *(P7)*
* A one line installer plus a systemd unit for the agent. *(P7)*
* An AUR package and a Homebrew tap. *(P7)*
* A README with a five minute quickstart and an honest security section. *(P7)*
* LICENSE, CONTRIBUTING, and issue templates. *(P7)*
* `borrow doctor`, one command that diagnoses a broken setup. *(P7)*

## Rust skills, in the order you will need them

**Foundations, Phase 1.** Ownership, borrowing, and lifetimes. `Result` and `Option` with
the `?` operator. Structs, enums, traits, and `match`. `std::process::Command` and
`tokio::process::Command` for streaming output. `serde` with `toml` and `serde_json`.

**Concurrency, Phase 1 onward, and this is the real learning curve.** `async` and `await`.
`tokio` tasks, `mpsc` channels, and `select!`. `clap` for the command line.

**Networking, Phase 4, the hardest part.** `quinn` for QUIC and `rustls` for TLS. The
concepts underneath: NAT, ports, TLS, and hole punching.

**An honest note.** The wall is async and tokio, not the basics. `borrow run` is the ideal
place to hit that wall. Streaming two output pipes while watching for Ctrl-C is genuinely a
`select!` problem. It is small enough not to drown in, and it is the exact pattern that gets
reused everywhere later.

## Platform notes

* This is a distro agnostic tool. A static Rust binary runs anywhere.
* Do not hardcode `apt`. The target is Arch, which uses `pacman`. Prefer checking and
  instructing over auto installing.
* The Arch packages that matter are `openssh`, `rsync`, and `tmux`.
* Standardise the daemon on systemd.
* GPU drivers, whether CUDA or ROCm, are the user's setup and not the tool's problem. The
  tool detects and reports the GPU.

## What to do next

1. Pair a real Client with the Linux Agent and run the Phase 3 flow end to end.
2. Measure sync time and build time for real Rust, Node, and Python projects.
3. Close the Client mid build, sleep, reconnect, and reattach.
4. Test Agent reboot recovery, file watchers in sessions, and multiple Clients.
5. Apply split overrides from `borrow.toml`, then begin Phase 4.

## Conventions

* **Errors.** `anyhow` is used in all current crates. Typed shared errors remain planned.
* **Async.** `tokio`. The tool is concurrent by nature, handling connections, streaming, and
  watching processes, so expect async everywhere past Phase 1.
* **Config and wire messages.** `serde`, with `toml` and `serde_json`.
* **Logging.** `tracing`.
* **CLI.** `clap`.
* **Telemetry.** `sysinfo` for CPU, RAM, and disk. `nvidia-smi` or `nvml-wrapper` for GPU.
* **Comments.** Doc comments above items. Keep function bodies clean.
* **Versions.** Always check crates.io. Never assume.

## What not to do

* Do not implement RAM pooling or network memory. Reject the request and explain the
  physics.
* Do not add remote desktop or GUI streaming features.
* Do not copy or mount build artifacts: `target/`, `node_modules/`, virtual environments,
  caches. They live in separate Agent storage. This is the single most important performance
  rule in the project.
* Do not copy environment files as source, and never pass their contents as command
  arguments.
* Do not hardcode a package manager. The target is Arch, which means `pacman`. Prefer
  detecting and instructing over auto installing.
* Do not hardcode OS specific paths in shared code.

## When you are unsure

Prefer the smallest change that keeps the current phase working end to end. If a request
conflicts with the golden rules or the do not list, say so and propose an in scope
alternative. If a request needs a new dependency, name it, say why, and prefer wrapping an
existing tool over reimplementing one.
