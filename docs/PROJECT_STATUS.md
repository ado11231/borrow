# Borrow Project Status

Last updated: September 13, 2026

This document explains what Borrow is, how the repository is organized, what we completed in each phase, and what comes next.

Update this document whenever a feature is completed, tested, delayed, or moved to another phase. It should always describe what the code can do today.

## 1. What Borrow Does

Borrow lets you use a powerful computer from a lighter computer.

The computer you work from is called the Client. It holds your project files, editor, browser, and terminal.

The powerful computer is called the Agent. It runs builds, servers, databases, containers, and other demanding work.

Your source files stay on the Client. The Agent reaches those files through a secure mount. Generated files stay on the Agent because moving thousands of generated files across a network would make builds slow.

The goal is simple:

1. Install one Borrow program on each computer.

2. Pair the computers once.

3. Run a normal command with `borrow run`.

4. See exactly where the command runs.

Borrow uses existing tools such as SSH and SSHFS. It does not create its own remote shell or file system.

## 2. Important Terms

### Client

The computer where the user works. It keeps the source files and sends commands.

### Agent

The computer with more CPU, memory, disk space, or GPU power. It runs the real work.

### Coordinator

A future service that will connect the Client and Agent when they are on different networks. This belongs to Phase 4 and does not exist yet.

### Mount

A way for the Agent to see the Client project as a local folder.

### Artifact split

The rule that keeps generated files on the Agent local disk. Rust `target`, Node `node_modules`, Python `.venv`, and caches must not live on the network mount.

### Control plane

The small Borrow connection used for pairing, machine information, health, and future session management.

### Work connection

The normal SSH connection that runs commands and streams their output.

## 3. Repository Structure

Borrow is one Rust workspace with three parts.

```text
borrow/
    Cargo.toml
    README.md
    CLAUDE.md
    docs/
        GUIDE.md
        PROJECT_STATUS.md
    crates/
        borrow-core/
        borrow-agent/
        borrow-cli/
```

There is one user program named `borrow`. The three parts below are code boundaries, not three programs that the user must manage.

## 4. Main Project Files

### `README.md`

This is the brief starting point. It shows how to run Borrow, its available commands, and phase status. Keep it short while the product is being developed.

### `docs/GUIDE.md`

This is the full technical and product guide. It contains the architecture, security decisions, setup details, complete roadmap, risks, and long term direction.

### `docs/PROJECT_STATUS.md`

This is the simple living reference. It explains what exists today and should be updated as work moves forward.

### `CLAUDE.md`

This contains rules for people and coding tools that change the repository. It protects the main product decisions.

### `Cargo.toml`

This defines the Rust workspace and includes every crate inside `crates`.

## 5. How the Code Is Divided

### `borrow-core`

This is the shared foundation. It contains code that both the Client and Agent need.

#### `src/lib.rs`

Lists the shared modules that the crate provides.

#### `src/config.rs`

Loads and saves paired Agent information. It stores names, addresses, SSH details, cached machine specifications, mount details, and the default Agent.

#### `src/protocol.rs`

Defines the messages that the Client and Agent exchange. Current messages cover pairing, machine information, and health.

#### `src/keys.rs`

Handles shared SSH key work. It learns machine identities, authorizes a Borrow key, labels it clearly, and removes only the key that Borrow owns.

#### `src/telemetry.rs`

Collects CPU, memory, disk, operating system, GPU, and installed tool information.

#### `src/presentation.rs`

Keeps terminal formatting consistent across the Client and Agent. It handles colors, status labels, aligned rows, and readable memory units. It checks stdout and stderr separately so redirected output stays plain.

#### `src/preflight.rs`

Checks whether the machines are ready. It checks SSH, SSHFS, required tools, listening services, and writable directories. Failures explain what the user needs to fix.

#### `src/stack.rs`

Finds the current project and recognizes Rust, Node, Python, and projects that use more than one stack.

It looks for normal project files such as `Cargo.toml`, `package.json`, `pyproject.toml`, and `requirements.txt`.

#### `src/mount.rs`

Decides where the project appears on the Agent and where generated files go. It creates mount commands, detects stale mounts, prepares local build folders, and describes the artifact split.

### `borrow-agent`

This is the Agent side library.

#### `src/lib.rs`

Runs `borrow serve`. It checks the machine, creates a short lived pairing code, listens for Client requests, installs the Client key, prepares the Agent key, and reports machine information and health.

### `borrow-cli`

This is the user facing command line program.

#### `src/main.rs`

Defines the available commands and sends each command to the correct module.

#### `src/client.rs`

Connects to the Agent control service and exchanges structured messages.

#### `src/keys.rs`

Creates and loads the dedicated Client SSH key used by Borrow.

#### `src/ssh.rs`

Builds safe SSH commands. It handles command arguments, project folders, setup commands, environment values, live output, terminal behavior, and exit codes.

#### `src/commands/link.rs`

Pairs the Client with an Agent. It now creates trust in both directions so the Client can run work and the Agent can mount Client files.

#### `src/commands/run.rs`

Finds the current project, prepares its mount, applies the artifact split, prints where the work will run, and starts the command on the Agent.

#### `src/commands/info.rs`

Shows stored Agent specifications. It can also ask the Agent for fresh information.

#### `src/commands/health.rs`

Shows a current snapshot of Agent resource use.

#### `src/commands/unlink.rs`

Releases Borrow mounts, removes Borrow access in both directions, removes saved host information, and forgets the Agent.

## 6. Why We Divided the Code This Way

The project started smaller. At the beginning, keeping everything together made it easier to prove the main idea.

During Phase 2, the responsibilities became clear enough to separate them.

1. `borrow-core` owns shared facts and rules.

2. `borrow-agent` owns the powerful machine service.

3. `borrow-cli` owns user commands and SSH execution.

The dependency direction is simple.

```text
borrow-cli uses borrow-core
borrow-cli uses borrow-agent
borrow-agent uses borrow-core
borrow-core does not use the other Borrow crates
```

This prevents circular code and gives each feature one clear home.

Examples:

1. A message used by both computers belongs in `borrow-core`.

2. Pairing code validation on the Agent belongs in `borrow-agent`.

3. Printing a user command result belongs in `borrow-cli`.

4. A mount rule shared by execution and future sessions belongs in `borrow-core`.

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

1. Added `borrow serve` for starting the Agent.

2. Added setup checks with clear fixes.

3. Added short lived, single use pairing codes.

4. Added `borrow link` for pairing and saving an Agent.

5. Created a dedicated Borrow SSH key.

6. Learned the Agent identity during pairing.

7. Added `borrow run` with live output.

8. Returned the real remote command exit code.

9. Made Ctrl C stop the remote command.

10. Added safe handling for spaces and shell characters.

11. Added `borrow info` for machine specifications.

12. Added `borrow health` for current resource use.

13. Added `borrow unlink` for removing access.

14. Added support for selecting between multiple Agents.

### Result

The Client could pair with an Agent on the same network and run commands safely with live output.

### Phase 1 limitation

Commands ran in the Agent login folder. They could not yet run inside the Client project.

## 10. Phase 2: Project Mount and Artifact Split

Status: Core implementation complete. Real machine acceptance testing remains.

Phase 2 lets remote commands see the real Client project without placing generated files on the network mount.

### What we accomplished

1. Divided the project into `borrow-core`, `borrow-agent`, and `borrow-cli`.

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

13. Updated `borrow run` to execute inside the mounted project.

14. Updated the run message to show the Agent, project location, and artifact split.

15. Updated `borrow unlink` to release mounts before removing access.

### Current example

```text
borrow run cargo build

running on archbox
project at /mnt/borrow/app
target stored on Agent local disk
```

### What still needs proof

1. Run the complete flow on two real machines after fresh pairing.

2. Test real Rust, Node, Python, and mixed projects.

3. Measure clean and incremental build times.

4. Confirm generated files never land on the network mount. Test existing Node and Python folders because the current redirect setup preserves them.

5. Test recovery after sleep and lost network access.

6. Test file watchers such as Vite and Cargo Watch.

7. Finish reading and applying custom settings from `borrow.toml`.

8. Update public documentation after these checks pass.

### Phase 2 completion rule

A real build on the mounted project should stay close to native Agent speed while the Client remains quiet and cool.

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

## 11. Phase 3: Persistent Sessions and Live Status

Status: Not started

Phase 3 will make remote work persistent and manageable.

### Planned work

1. Add `borrow attach`.

2. Use an existing session tool such as tmux or zellij.

3. Let a session survive sleep, disconnects, and network changes.

4. Make reconnecting return the user to the same task.

5. Add an Agent process list.

6. Add `borrow ps` to show remote work.

7. Add `borrow stop` to stop one known job safely.

8. Add continuously refreshed health information.

9. Add `borrow top` for a live resource view.

10. Warn when the Agent has too little free memory, disk space, or GPU memory for a large job.

### Recommended work order

1. Define session and process messages in `borrow-core`.

2. Add the Agent process records.

3. Add tmux or zellij session creation and reconnection.

4. Add `borrow attach` in `borrow-cli`.

5. Add `borrow ps`.

6. Add `borrow stop`.

7. Add live health updates.

8. Add `borrow top`.

9. Add resource warnings.

10. Test disconnect and reconnection on real machines.

### Phase 3 completion rule

Start a long build, close the Client, reconnect later, attach again, and return to the same running task. The user must also be able to inspect and stop that task.

## 12. Later Phases

### Phase 4

Connect machines across different networks through a Coordinator. Prefer a direct connection when available and use a secure relay when needed.

### Phase 5

Add visible status, notifications, and automatic port forwarding.

### Phase 6

Add model workloads, faster direct connections, and broader platform support.

### Phase 7

Prepare public releases, installers, packages, diagnostics, licensing, contribution files, and automated releases.

## 13. Current Verification

The full Rust workspace builds successfully.

All 88 automated tests pass. Formatting checks and Clippy also pass.

The tests currently cover:

1. Project detection.

2. Artifact split rules.

3. Mount command construction.

4. Stale mount handling logic.

5. Agent selection and configuration.

6. Pairing code parsing.

7. SSH command construction.

8. Safe shell argument handling.

9. Working directory and environment setup.

10. Host and authorized key handling.

11. Terminal color options, help, errors, and remote argument forwarding.

12. Health thresholds, readable units, missing GPU data, and invalid values.

Automated tests do not replace the two machine checks still required for Phase 2.

## 14. Important Product Rules

1. The Client must stay light.

2. Every remote command must say where it runs.

3. Generated files must stay off the network mount.

4. Borrow should wrap trusted tools instead of rebuilding them.

5. Setup must remain simple for a new user.

6. Every failed setup check should explain the exact fix.

7. Private keys must never move between machines.

8. Borrow must not require a private network service from the user.

9. Platform specific details must not leak into shared design.

10. A phase is complete only after its real user flow has been tested.

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
