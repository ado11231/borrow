# Slingshot Project Status

Last updated: September 23, 2026

This document explains what Slingshot is, how the repository is organized, what we completed in each phase, and what comes next.

Update this document whenever a feature is completed, tested, delayed, or moved to another phase. It should always describe what the code can do today.

## 1. What Slingshot Does

Slingshot lets you use a powerful computer from a lighter computer.

The computer you work from is called the Client. It holds your project files, editor, browser, and terminal.

The powerful computer is called the Agent. It runs builds, servers, databases, containers, and other demanding work.

You edit source files on the Client. The Agent keeps a filtered copy of each project on its own disk and Slingshot keeps that copy in step. Generated files stay in separate Agent storage because copying thousands of generated files would make every sync slow.

The goal is simple:

1. Install one Slingshot program on each computer.

2. Pair the computers once.

3. Run a normal command with `slingshot run`.

4. See exactly where the command runs.

Slingshot uses existing tools such as SSH, rsync, and tmux. It does not create its own remote shell, file transfer, or terminal multiplexer.

## 2. Important Terms

### Client

The computer where the user works. It keeps the source files and sends commands.

### Agent

The computer with more CPU, memory, disk space, or GPU power. It runs the real work.

### iroh

A library both machines use to reach each other across networks. It dials a machine by its public key, connects directly through NAT when it can, and relays through a public server when it cannot. ssh runs inside it, so a relay sees only encrypted bytes. This is Phase 4 work. It replaced the earlier plan of a Coordinator server we would build and host.

### Source copy

The Agent's filtered copy of one project for one Client. It contains only eligible source. Git ignored files, version control internals, generated folders, and environment files are never copied.

### Baseline

The last state both machines agreed on. Slingshot compares each side with the baseline to tell edits on one side from edits on both.

### Artifact split

The rule that keeps generated files out of the source copy. Rust `target`, Node `node_modules`, Python `.venv`, and caches go to separate Agent storage.

### Session

A persistent tmux session on the Agent for one project. It keeps running when the Client disconnects or sleeps.

### Job

A Slingshot run or session recorded on the Agent, shown by `slingshot ps` and stopped by `slingshot stop`.

### Control plane

The authenticated Slingshot connection used for information, health, sync coordination, sessions, jobs, and environment files. It travels through SSH to a private socket on the Agent.

### Work connection

The normal SSH connection that runs commands, attaches sessions, and carries rsync transfers.

## 3. Repository Structure

Slingshot is one Rust workspace with three parts.

```text
slingshot/
    Cargo.toml
    README.md
    CLAUDE.md
    docs/
        GUIDE.md
        PROJECT_STATUS.md
    crates/
        slingshot-core/
        slingshot-agent/
        slingshot-cli/
```

There is one user program named `slingshot`. The three parts below are code boundaries, not three programs that the user must manage.

## 4. Main Project Files

### `README.md`

This is the brief starting point. It shows how to run Slingshot, its available commands, and phase status. Keep it short while the product is being developed.

### `docs/GUIDE.md`

This is the full technical and product guide. It contains the architecture, security decisions, setup details, complete roadmap, risks, and long term direction.

### `docs/PROJECT_STATUS.md`

This is the simple living reference. It explains what exists today and should be updated as work moves forward.

### `CLAUDE.md`

This contains rules for people and coding tools that change the repository. It protects the main product decisions.

### `Cargo.toml`

This defines the Rust workspace and includes every crate inside `crates`.

## 5. How the Code Is Divided

### `slingshot-core`

This is the shared foundation. It contains code that both the Client and Agent need.

#### `src/lib.rs`

Lists the shared modules that the crate provides.

#### `src/config.rs`

Loads and saves paired Agent information: names, addresses, SSH details, the Agent's Slingshot program path, cached machine specifications, and the default Agent.

#### `src/protocol.rs`

Defines the pairing messages exchanged over TCP.

#### `src/control.rs`

Defines the authenticated control messages, their version, size limits, and framing. It also defines project, snapshot, session, and job records.

#### `src/storage.rs`

Creates private folders, random identifiers, and safe relative paths. It writes files atomically with owner only permissions and takes advisory locks.

#### `src/source.rs`

Decides which files are eligible source and builds manifests with content hashes, executable bits, and link targets. It applies mandatory exclusions, Git ignore rules, and `slingshot.toml` exclusions, and refuses unsafe links.

#### `src/sync.rs`

Plans three way syncs, applies staged changes with backups and a recovery journal, and recovers interrupted syncs.

#### `src/keys.rs`

Handles shared SSH key work. It learns machine identities, authorizes a Slingshot key, labels it clearly, and removes only the key that Slingshot owns.

#### `src/network.rs`

Classifies an address as this machine, the local network, a tailnet, or anything else. The Agent uses it to label pairing codes and the Client uses it to decide which address to try first.

#### `src/tunnel.rs`

What both machines share for iroh: the protocol name, the refusal reason, and the persistent iroh identity each machine keeps in its own storage.

#### `src/telemetry.rs`

Collects CPU, memory, disk, workspace disk, operating system, GPU, and installed tool information. It also decides resource warnings.

#### `src/presentation.rs`

Keeps terminal formatting consistent across the Client and Agent. It handles colors, status labels, aligned rows, and readable memory units. It checks stdout and stderr separately so redirected output stays plain.

#### `src/preflight.rs`

Checks whether the machines are ready. It checks for an SSH server, rsync, tmux, and GPU tooling. Failures explain what the user needs to fix.

#### `src/stack.rs`

Finds project markers and recognizes Rust, Node, Python, and projects that use more than one stack.

#### `src/artifacts.rs`

Decides where generated files go for each stack and describes the artifact split.

### `slingshot-agent`

This is the Agent side library.

#### `src/lib.rs`

Runs `slingshot start`. It checks the machine, starts the private control service, creates a short lived pairing code, installs the Client key, and reports the Agent's Slingshot program path.

#### `src/tunnel.rs`

Runs the Agent's iroh endpoint. It refuses keys that never paired and joins every stream from a paired Client to the Agent's own sshd on loopback.

#### `src/clients.rs`

Records which Client iroh keys may connect. Pairing adds a key and unlink removes it.

#### `src/awake.rs`

Keeps the Agent from sleeping while `slingshot start` runs, using `systemd-inhibit` on Linux and `caffeinate` on macOS. The lock ends with `start`, even when `start` is killed.

#### `src/service.rs`

Runs the private control socket and the hidden SSH helper that relays to it. It checks versions, sizes, timeouts, and the connecting account, and ties each sync lease to its connection.

#### `src/projects.rs`

Manages project storage on the Agent: opening projects, sync leases, verified pushes, agreement based pulls, build output folders, environment files, and unlink cleanup.

#### `src/jobs.rs`

Keeps job records, reconciles them after restarts and reboots, creates and finds tmux sessions, and stops work without trusting stale process IDs.

#### `src/runner.rs`

Runs one foreground command for `slingshot run` in its own process group. It passes terminal control to the command, forwards hangups, notices lost connections, and records the result.

### `slingshot-cli`

This is the user facing command line program.

#### `src/main.rs`

Defines the available commands, including hidden helpers used over SSH, and sends each command to the correct module.

#### `src/client.rs`

Sends pairing requests and holds authenticated control connections to the Agent.

#### `src/project.rs`

Finds the current project and keeps the persistent project ID for each project and Agent.

#### `src/transfer.rs`

Runs previews, pushes, and pulls. It scans local files, plans changes, runs rsync with an explicit file list, sends heartbeats, and applies pulled files safely. Pushes and pulls take and release the Agent's sync lease through one shared pair of helpers.

#### `src/live.rs`

Draws live views that refresh every two seconds, exit on Q or Ctrl C, and restore the terminal.

#### `src/keys.rs`

Creates and loads the dedicated Client SSH key used by Slingshot. Where the key lives is decided once, in `slingshot-core`.

#### `src/ssh.rs`

Builds safe SSH commands, runs interactive commands with the terminal attached, and starts SSH for rsync. Over iroh it adds the tunnel as ProxyCommand and shares one connection across a command's calls.

#### `src/route.rs`

Chooses how to reach a box. It probes every saved address at once, takes the most preferred one that answers, and falls back to iroh when none does.

#### `src/tunnel.rs`

The Client end of iroh. It runs as ssh's ProxyCommand, carries ssh's bytes to the Agent, and explains why a box cannot be reached when a connection fails.

#### `src/commands/link.rs`

Pairs the Client with an Agent. It creates only Client to Agent trust.

#### `src/commands/run.rs`

Syncs the current project, shows resource warnings, prints where the work will run, and runs the command in the Agent copy.

#### `src/commands/attach.rs`

Copies the project on first use, then creates or rejoins its session and attaches the terminal.

#### `src/commands/sync.rs`

Pushes, pulls, or previews project source changes.

#### `src/commands/env.rs`

Adds, lists, and removes environment files kept on the Agent.

#### `src/commands/ps.rs`

Lists Slingshot runs and sessions and stops one by ID.

#### `src/commands/info.rs`

Shows stored Agent specifications. It can also ask the Agent for fresh information.

#### `src/commands/health.rs`

Shows a current snapshot of Agent resource use, or a live view with `--watch`.

#### `src/commands/top.rs`

Shows live Agent resources with active Slingshot jobs.

#### `src/commands/unlink.rs`

Removes this Client's environment files, Slingshot key, and learned host keys from the Agent, then forgets the Agent.

#### `tests/output.rs`

Runs the built `slingshot` program and checks what a user actually sees: color modes, help, hidden internal commands, and the commands that refuse to run without a terminal.

## 6. Why We Divided the Code This Way

The project started smaller. At the beginning, keeping everything together made it easier to prove the main idea.

During Phase 2, the responsibilities became clear enough to separate them.

1. `slingshot-core` owns shared facts and rules.

2. `slingshot-agent` owns the powerful machine service.

3. `slingshot-cli` owns user commands and SSH execution.

The dependency direction is simple.

```text
slingshot-cli uses slingshot-core
slingshot-cli uses slingshot-agent
slingshot-agent uses slingshot-core
slingshot-core does not use the other Slingshot crates
```

This prevents circular code and gives each feature one clear home.

Examples:

1. A message used by both computers belongs in `slingshot-core`.

2. Pairing code validation on the Agent belongs in `slingshot-agent`.

3. Printing a user command result belongs in `slingshot-cli`.

4. A source eligibility rule used by both computers belongs in `slingshot-core`.

## 7. Phase S: Machine and Repository Setup

Status: Complete

This phase prepared the repository and both computers before product work began.

### What we accomplished

1. Created the Rust project and Git repository.

2. Wrote the first project documentation.

3. Prepared SSH access between the Client and Agent.

4. Installed and checked SSHFS.

5. Prepared Rust on both machines.

6. Confirmed that both machines could reach each other without password prompts.

## 8. Phase 0: Prove the Experience

Status: Complete

This phase tested the idea manually before building automation.

### What we accomplished

1. Ran real builds on the Agent through SSH.

2. Compared Client and Agent build behavior.

3. Mounted a project manually with SSHFS.

4. Confirmed that generated files on the mount create serious slowdown.

5. Confirmed that keeping generated files on the Agent solves the main performance problem.

6. Tested a remote development server and local browser access.

### What we learned

The product needs two separate locations on the Agent.

1. Mounted source files from the Client.

2. Generated files on the Agent local disk.

This became the main design rule for Phase 2.

## 9. Phase 1: Pairing and Remote Commands

Status: Complete

Phase 1 built the working foundation.

### What we accomplished

1. Added `slingshot start` for starting the Agent.

2. Added setup checks with clear fixes.

3. Added short lived, single use pairing codes.

4. Added `slingshot link` for pairing and saving an Agent.

5. Created a dedicated Slingshot SSH key.

6. Learned the Agent identity during pairing.

7. Added `slingshot run` with live output.

8. Returned the real remote command exit code.

9. Made Ctrl C stop the remote command.

10. Added safe handling for spaces and shell characters.

11. Added `slingshot info` for machine specifications.

12. Added `slingshot health` for current resource use.

13. Added `slingshot unlink` for removing access.

14. Added support for selecting between multiple Agents.

### Result

The Client could pair with an Agent on the same network and run commands safely with live output.

### Phase 1 limitation

Commands ran in the Agent login folder. They could not yet run inside the Client project.

## 10. Phase 2: Project Mount and Artifact Split

Status: Superseded for execution. Project detection and the artifact split remain in use.

Phase 2 let remote commands see the real Client project through an SSHFS mount without placing generated files on the network mount. Phase 3 replaced the mount with source copies, because the mount needed an SSH server on the Client, reverse SSH trust, and a network round trip for every file operation. The record below describes what Phase 2 built.

### What we accomplished

1. Divided the project into `slingshot-core`, `slingshot-agent`, and `slingshot-cli`.

2. Added automatic Rust, Node, and Python project detection.

3. Added support for projects that use more than one stack.

4. Added the second SSH trust direction.

5. Allowed the Agent to securely reach back to the Client.

6. Added automatic SSHFS mount setup.

7. Added checks for healthy and stale mounts.

8. Added automatic stale mount recovery.

9. Added local Agent build directories.

10. Redirected Rust `target` to Agent storage.

11. Redirected Node `node_modules` to Agent storage.

12. Redirected Python `.venv` and pip cache to Agent storage.

13. Updated `slingshot run` to execute inside the mounted project.

14. Updated the run message to show the Agent, project location, and artifact split.

15. Updated `slingshot unlink` to release mounts before removing access.

### Current example

```text
slingshot run cargo build

running on archbox
project at /mnt/slingshot/app
target stored on Agent local disk
```

### What happened next

The mount acceptance checks were not completed. Phase 3 replaced mounted execution before they ran, so the remaining proof now belongs to the Phase 3 acceptance checks.

## Cleanup Before Phase 3

Status: Complete for local implementation and verification

1. Removed repetitive comments and shortened important explanations.

2. Standardized capitalization, setup checks, pairing details, and errors.

3. Added shared terminal formatting and color control.

4. Improved health and machine information with aligned rows and readable units.

5. Added separate GPU rows and written status labels alongside colors.

6. Added tests for terminal output, color choices, thresholds, and missing measurements.

7. Simplified README to setup, commands, and phase status.

8. Updated the guide and this project reference.

The work was verified locally on September 13, 2026. No tests on a remote Agent were performed during this cleanup.

## 11. Phase 3: Source Copies, Persistent Sessions, and Live Status

Status: Complete

Phase 3 makes remote work persistent and manageable, and replaces mounted execution with source copies.

### What we accomplished

1. Added authenticated control. A hidden helper started over SSH relays versioned, size limited messages to a private Agent socket with timeouts. Info and health moved to this route, and the TCP port now only pairs.

2. Changed pairing to create only Client to Agent SSH trust. The Client no longer needs an SSH server.

3. Added persistent random project IDs for each project and Agent, with separate Agent storage for source, build output, environment files, and state.

4. Added source eligibility rules. Git ignored files, version control internals, generated folders, and every environment file name are always excluded. `slingshot.toml` can add exclusions. Unsafe links are refused.

5. Added three way sync with conflict refusal, receiver only edits kept, staged rsync transfers, hash verification, backups, a recovery journal, and automatic recovery.

6. Added `slingshot sync`, `slingshot sync --pull`, and `--check` previews.

7. Changed `slingshot run` to sync first, then run in the matching folder of the Agent copy with terminal passthrough, exit codes, and cancellation. Lost connections interrupt the run.

8. Added `slingshot attach`, which copies on first use and creates or rejoins one tmux session per project on an isolated Slingshot tmux server.

9. Added persistent job records that survive daemon restarts, `slingshot ps`, `slingshot ps --all`, and `slingshot stop` with a five second grace period.

10. Added `slingshot env add`, `slingshot env list`, and `slingshot env remove` for environment files kept outside source with private permissions.

11. Added `slingshot health --watch` and `slingshot top`, refreshing every two seconds.

12. Added warnings before runs and new sessions at 90 percent RAM use or under 2 GiB of free workspace disk. Warnings never block work.

13. Updated `slingshot unlink` to remove this Client's environment files, keep source copies and backups, refuse while this Client's projects are busy, and release older mounts only for older pairings.

### Current example

```text
slingshot run cargo test

▶ Copying 3 files to archbox
✓ Synced 3 changes to archbox
▶ Running on archbox · app/crates/cli · target → local disk
```

### Two machine acceptance

Verified on September 13, 2026 between a Mac Client and an Arch Linux Agent (archbox) on the same LAN, with GNU rsync 3.5.0 and tmux on the Agent.

1. Pairing over the LAN, `info`, `health`, and the migration path: `unlink` cleared a Phase 2 pairing before the new `link`.

2. First `slingshot run cargo build` copied 47 files and finished in 9 seconds in total. The second run had nothing to sync and started in 1 second. `cargo test` passed all 140 tests on the Agent.

3. Subfolder runs, exit codes, standard input, and Ctrl C. The source copy held no `.git` and no `target`, and `target` lived in `artifacts`. `node_modules` in a Node project stayed on the Agent as a link into `artifacts`.

4. Push, pull, preview, an edit made only on the Agent, and a two sided conflict. `sync`, `sync --pull`, and `run` all refused the conflict and changed nothing.

5. Environment files stored with `-rw-------` permissions, visible to `run`, never included in a sync in either direction, and removed cleanly.

6. `attach`, detach, `run` refused while a session is active, reattach to the same shell, `stop` within a second, and the Agent's personal `tmux ls` showing no Slingshot sessions.

7. `top` and `health --watch` refreshing every two seconds, exiting on Q and Ctrl C, and restoring the terminal.

8. The completion rule. A clean `cargo build --release` started inside `attach`, the Client lost its network for four minutes, and `attach` afterwards returned to the same session with the build finished. A `run sleep 300` started before the outage finished on the Agent and was recorded as Completed.

9. Restarting `slingshot start` kept the session running. Rebooting the Agent marked it Interrupted, a fresh `attach` created a new session, and GPU telemetry appeared once the reboot cleared an NVIDIA driver mismatch.

10. `unlink` refused while a session was active. After `stop` it removed the key and this Client's environment files, and kept the source copies.

### Follow ups found during acceptance

1. Fixed on September 14, 2026: a `run` or `attach` that loses its connection printed raw ssh messages. SSH's own messages now go to a private log, and Slingshot prints one line naming the Agent and what happens next. SSH exits 255 silently on a keepalive timeout, so a silent 255 triggers a quick ssh check before it is reported as a lost connection. Still to confirm on the two real machines.

2. Fixed on September 14, 2026: when `nvidia-smi` is installed but failing, `health` and `top` show the reason. A driver and library version mismatch is named and suggests a reboot. The cached `info` specs still say no GPU data. Still to confirm on the two real machines.

3. Fixed on September 14, 2026: a run ended with Ctrl C is recorded as `Interrupted` instead of `Failed 130`, and `ps` shows exit codes only for failures.

4. Untested: file watchers inside sessions, several Clients sharing one Agent account, and large Node and Python projects.

5. Apply split overrides from `slingshot.toml`. Only `sync.exclude` is read today.

### Phase 3 completion rule

Start a long build, close the Client, reconnect later, attach again, and return to the same running task. The user must also be able to inspect and stop that task. This was met on September 13, 2026, see the acceptance record above.

## 11b. Cleanup Before Phase 4

Status: Complete for local implementation and verification. Not yet run on two real machines.

Done on September 20, 2026, to clear the ground before Phase 4 is built on top of it.

### Four real defects, each found by reading both sides of the code

1. `authorized_keys` was rewritten by truncating the file and then writing it. A crash or a full disk in between left it empty, which locks the owner out of their own machine. Both it and `known_hosts` now go through the same atomic write the rest of the project uses.

2. Sync recovery read the destination's permissions through a symlink, where the matching code in the forward direction deliberately does not. Recovering a step whose destination had become a link stamped the link target's permissions onto the restored file. A test pins this, and it fails against the old code.

3. The name of Slingshot's temporary write files was written out as a literal in one place and as a shared constant in another. If they drifted, files being written would start being treated as project source.

4. `link` learned host keys without a port and `unlink` forgot them with one. Harmless today, because nothing sets a port, and a trap as soon as something does.

### Cleanup

1. The retired Phase 2 mount surface is gone from config, from the pairing message, from `unlink`, and from the artifact rules. The control protocol is version 4, so machines paired before this must run `slingshot link` again.

2. Dead code removed: an unused preflight check, a struct field nothing read, two pairing responses nothing ever sent, and an unused dependency. Items only their own module used are no longer exported.

3. Helpers that existed in two copies now have one home each in `slingshot-core`: where SSH keys live, how a job ID is shortened, what this machine calls itself, how a synced manifest is projected, and which environment file targets are allowed. The last of these means the Client no longer calls into the Agent crate to validate an argument.

4. Names that meant two different things were changed. `Outcome` and `State` each described two unrelated types, `artifacts::env` was about build variables rather than environment files, and `mandatory` read as required when it means excluded. Memory fields now say `mib`, which is what they always held.

5. `slingshot run` says `target → Agent disk` rather than `local disk`, which described the wrong machine from where the user is sitting.

### Still to do

The two real machines were re-paired on September 22, 2026. Running the full Phase 3 flows again is tracked in section 11c.

## 11c. Phase 4: Works From Anywhere

Status: Acceptance testing

Once paired, the Agent is reachable from any network for as long as it is on and `slingshot start` is running. There is no account, no server to host, and no router change. The earlier plan was a Coordinator server we would build and host. It was replaced on September 22, 2026 by wrapping iroh, which provides NAT traversal, relays, and end to end encryption.

### What we accomplished

1. Pairing reports every address the Agent has. The Client probes them all at once and uses the most preferred one that answers: the local network, then anything else, then the tailnet. `run` and `attach` say which path they used.

2. ssh checks host keys under the pairing address, so keys learned at pairing match whichever address or path is used.

3. Each machine keeps a persistent iroh key. Pairing swaps the public halves, and the Agent records which Client keys may connect. `unlink` makes the Agent forget the Client's key. The control protocol is version 5.

4. `slingshot start` runs an iroh endpoint on iroh's public relays and says whether other networks can reach the box. Unpaired keys are refused, and paired Clients reach only the Agent's own sshd.

5. When no saved address answers, ssh reaches the Agent through a hidden `slingshot internal-tunnel` ProxyCommand. `run`, `attach`, sync, and control all use it with no other change.

6. Over iroh, ssh's ControlMaster shares one connection across a command's calls and keeps it for 30 seconds.

7. When a connection over iroh fails, the Client names the cause: no internet, a box that is off or asleep or not serving, or a box that no longer accepts this machine.

8. `slingshot start` keeps the Agent from sleeping while it runs.

9. Library logs such as iroh's show only real errors, and a normal iroh disconnect is not reported as a problem.

### Verified on the two real machines

Verified on September 22, 2026 between the Mac Client and archbox, after re-pairing for protocol version 5.

1. `slingshot run uname -a` ran via the local network at home, via the tailnet on a phone hotspot, and via iroh on the hotspot with Tailscale off.

2. Over iroh, one ssh call took 1.3 to 1.6 seconds before connection sharing and 0.3 seconds for every later call after it. A first `slingshot run` with a small sync took 3.6 seconds and a repeat took 1.5 seconds.

3. With `slingshot start` stopped, `slingshot run` on the hotspot failed after about 20 seconds with a message naming the cause.

4. `systemd-inhibit --list` on archbox showed Slingshot's lock while serving.

5. A Client key that never paired was refused, and a paired one reached sshd, tested on one machine against iroh's live relays.

6. `slingshot sync` and `slingshot attach` with detach and reattach worked via iroh from the hotspot with Tailscale off.

### Follow ups found during acceptance

1. `slingshot attach` works but feels unclear, messy, and not responsive. The user asked for this to be reworked later rather than now. Whether the lag comes from iroh or from the session itself is not yet measured. Inside a session, Claude Code ran but looked broken, and Codex did not run at all. Likely causes to check first: the session shell not activating mise, which is where Codex's Node lives on archbox, and the terminal type, colors, or UTF-8 settings inside the Slingshot tmux server.

2. `slingshot start` does not start by itself after the Agent reboots, so the Agent is unreachable until someone logs in and runs it. Installing it as a service that starts at boot is planned, currently listed under Phase 7.

3. `hostname` inside a session failed because a clean Arch install does not include it. This is not a Slingshot fault, but instructions should use `uname -n`.

### Still to do

Optionally run `sync --pull`, a network drop mid build, and an Agent reboot over iroh, and confirm that the lock disappears when `start` stops. Pairing across networks and a self hosted relay setting are Phase 6.

### Rename to Slingshot

On September 22, 2026 the project was renamed from borrow to Slingshot, and `serve` became `start`. The crates, the command, the config and data folders, the ssh key, the iroh protocol name, and environment variables all changed. Nothing is migrated: after updating, start the Agent and run `slingshot link` again. The old `borrow` folders can be deleted by hand.

### Output redesign

Also on September 22, 2026, output moved to one style. Connecting, syncing, pairing, and stopping are steps: a spinner while they run, then a checked line with the time taken. `run` ends with its total time and exit code. Color marks only symbols, the box and path, and values that need attention, so a warning stands out. Checked on the real Mac and archbox with `run`, `ps`, `info`, `health`, and `env list`. The spinners themselves still need checking by eye in an interactive terminal.

## 12. Later Phases

### Phase 4

Status: Acceptance testing. See section 11c.

### Phase 5

Status: In progress. Menu bar and notifications are built; automatic port forwarding, server up, and needs input notifications are not.

What exists:

1. `crates/slingshot-cli/src/watch/mod.rs` is the hidden `slingshot internal-watch`. It keeps one control connection, polls `Health` every two seconds and `Jobs` every four, reconnects with a backoff of 2, 5, 15, then 30 seconds, probes the path again after losing the box, and reloads the configuration each time. It exits when its input closes.

2. `watch/event.rs` defines the JSON lines, version 1: a `status` line with values and a good, warning, or high level for each, and a `notice` line with a title and body. While offline, the status also carries a problem: a plain title and sentence, and a fix naming the command and the machine to run it on. A `retry` line on the helper's input skips the reconnect wait.

3. `watch/state.rs` decides notifications. The first job list only sets a baseline. A run of ten seconds or more that completes or fails is told once, interrupted jobs are always told, and stopped runs and ended sessions stay quiet. Two failed checks in a row mean unreachable, told once, then back. RAM, workspace disk, and GPU temperature warn once when they cross the high limit and re-arm only after recovering. The limits are the ones `slingshot health` uses, shared from `commands/health.rs`.

4. `commands/menubar.rs` is `slingshot menubar`. On macOS it saves this program's path in the app's settings, because an app opened at login has no shell PATH, and opens the app. Elsewhere it says the menu bar is macOS only. `link` ends with a tip about it on macOS.

5. `mac/menubar/` is the SwiftUI app: an icon in the menu bar and a `MenuBarExtra` panel with CPU, RAM, GPU, and workspace sections drawn as colored bars, rows for Open at login and Quit, an offline screen with the cause, the fix command with a copy button, Try again, when the box was last seen, and the last known numbers greyed out, `UserNotifications` for banners, and `SMAppService` to start at login, turned on at first launch. `build.sh` builds it, signs it ad hoc, and installs it in `~/Applications`.

Verified on September 23, 2026: the new tests pass; `internal-watch` reached archbox via iroh and printed status lines every two seconds, then exited when its input closed; the app built, installed, opened with `slingshot menubar`, started its helper, and was registered as an enabled login item. archbox then stopped answering, which the helper reported as offline, and the menu bar showed offline.

After archbox was turned on and `slingshot start` run there, the menu bar switched to live numbers without restarting the app or pairing again. The panel showed archbox via the local network, CPU, RAM, the RTX 4060's use and temperature, VRAM, and workspace space. `slingshot run sh -c 'sleep 12; exit 3'` produced one failure banner once notifications were allowed in System Settings, and `slingshot run echo hi` produced none.

With `slingshot start` stopped on archbox, the offline screen said Slingshot was not running there and offered `slingshot start`, and it switched back to live numbers by itself once `start` ran again.

Not yet verified: the back notification, resource warnings, and behavior across a log out and log in. On the first install, banners appeared only after notifications were turned on for Slingshot in System Settings. Whether the permission prompt showed is not recorded, so a clear first run prompt still needs checking.

### Phase 6

Add model workloads, pairing across networks, a self hosted relay setting, and broader platform support.

### Phase 7

Prepare public releases, installers, packages, diagnostics, licensing, contribution files, and automated releases.

## 13. Current Verification

Verified on September 22, 2026.

The full Rust workspace builds successfully. All 198 automated tests pass, and formatting checks and Clippy pass. The Phase 3 flows were accepted on a real Mac Client and Arch Linux Agent, recorded in section 11. The Phase 4 checks run on those two machines are recorded in section 11c. The full Phase 3 flows have not yet been run again over iroh.

The tests cover:

1. Project detection, project identity, and artifact split rules.

2. Source eligibility, including nested ignore negations, repository excludes, environment files, generated folders, and `target` beside `Cargo.toml` only.

3. Link safety, including links that escape through other links, loops, and absolute targets.

4. Three way planning, conflicts, receiver only edits, deletions, and agreement based baselines.

5. Staged application, hash verification, rollback after a failure midway, and recovery before and after the commit point.

6. Sync leases, verified pushes, excluded paths sent by a Client, and pulls.

7. Environment file privacy, validation, replacement, removal, and scoped unlink cleanup.

8. Job reconciliation, reboot detection, stale process IDs, graceful stop with a kill after five seconds, and history limits.

9. Control framing, version mismatches, oversized messages, and lease release when a connection closes.

10. Atomic replacement of SSH line files, host keys forgotten under the port they were learned with, and sync recovery reading permissions without following a symlink.

11. Agent selection, configuration, pairing code parsing, and SSH command construction.

12. Safe shell argument handling, rsync remote path escaping, and argument forwarding for hidden helpers.

13. Terminal colors, help, errors, previews, job lists, health thresholds, quit keys, and commands that need a terminal.

A loopback test on one Mac used a private unprivileged SSH server, real rsync, and real tmux, with the Client and Agent as separate Slingshot storage areas. It verified pairing, protected info and health, copying with exclusions, links, executable bits, unusual file names, subfolder runs, pushes, pulls, receiver only edits, conflicts, environment files, busy refusal, stop with grace and kill, interactive input and Ctrl C in a terminal, lost connections, attach, detach, reattach, a daemon restart with a live session, live views, and unlink.

That loopback test found and fixed three problems: openrsync splitting the remote path on spaces, a five second delay closing every control connection, and lost connections going unnoticed on macOS.

Automated and loopback tests do not replace the two machine checks still required for Phase 3.

## 14. Important Product Rules

1. The Client must stay light.

2. Every remote command must say where it runs.

3. Generated files must never be copied into source or mounted.

4. Environment files must never be treated as source.

5. Sync must never overwrite an edit made on both machines.

6. Slingshot should wrap trusted tools instead of rebuilding them.

7. Setup must remain simple for a new user.

8. Every failed setup check should explain the exact fix.

9. Private keys must never move between machines.

10. Slingshot must not require a private network service from the user.

11. Platform specific details must not leak into shared design.

12. A phase is complete only after its real user flow has been tested.

## 15. How to Update This Document

After meaningful work, update these five items.

1. Change the date at the top.

2. Change the phase status if its completion rule has been met.

3. Add completed work under the correct phase.

4. Update the file explanation when a responsibility moves or a new file is added.

5. Record what was actually tested. Do not describe planned work as completed work.

Use these status words consistently:

```text
Not started
In progress
Core implementation complete
Acceptance testing
Complete
```

Keep this file simple enough that a new contributor can understand the project before reading the full guide.
