# Borrow

## How to run

Use two computers on the same network with Rust installed on both. The Agent needs an SSH server, rsync, and tmux. The Client needs rsync.

From the Borrow repository on each computer:

```sh
cargo build
export PATH="$PWD/target/debug:$PATH"
```

Start Borrow on the Agent:

```sh
borrow serve
```

Follow any setup instructions it prints. Keep it running.

On the Client, use the pairing code printed by the Agent:

```sh
borrow link <code>
```

Then open your project folder and run:

```sh
borrow run cargo build
```

## Commands

1. `borrow serve` starts the Agent.
2. `borrow link <code>` pairs the computers.
3. `borrow run <command>` copies project changes to the Agent, then runs the command there.
4. `borrow attach` opens or rejoins a persistent session for the project.
5. `borrow sync` copies project changes to the Agent. Its pull option retrieves Agent edits, and its check option previews without changing anything.
6. `borrow env add`, `borrow env list`, and `borrow env remove` manage environment files kept on the Agent.
7. `borrow ps` lists runs and sessions.
8. `borrow stop <id>` stops a run or session.
9. `borrow info` shows machine specifications.
10. `borrow health` shows current resource use. Its watch option keeps it updating.
11. `borrow top` shows live resource use with active jobs.
12. `borrow unlink` removes the pairing.
13. `borrow help <command>` shows every option.

## Phases

1. Setup and Phase 0: Complete. Prepared the machines and tested the idea.
2. Phase 1: Complete. Pairing, remote commands, information, and health.
3. Phase 2: Replaced. Its project detection and separate build output continue in Phase 3, where source copies replaced its project mount.
4. Cleanup: Complete. Comments, CLI formatting, health colors, and documentation.
5. Phase 3: Implemented and verified locally. Source copies, sync, sessions, job control, environment files, and live status. Testing on two real machines remains.
6. Phase 4: Planned. Connections across networks.
7. Phase 5: Planned. Notifications and port forwarding.
8. Phase 6: Planned. Model workloads and broader platform support.
9. Phase 7: Planned. Installers and public releases.
