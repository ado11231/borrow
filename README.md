# Slingshot

## How to run

Use two computers on the same network with Rust installed on both. The Agent needs an SSH server, rsync, and tmux. The Client needs rsync.

From the Slingshot repository on each computer:

```sh
cargo build
export PATH="$PWD/target/debug:$PATH"
```

Start Slingshot on the Agent:

```sh
slingshot start
```

Follow any setup instructions it prints. Keep it running.

On the Client, use the pairing code printed by the Agent:

```sh
slingshot link <code>
```

Then open your project folder and run:

```sh
slingshot run cargo build
```

## Commands

1. `slingshot start` starts the Agent.
2. `slingshot link <code>` pairs the computers.
3. `slingshot run <command>` copies project changes to the Agent, then runs the command there.
4. `slingshot attach` opens or rejoins a persistent session for the project.
5. `slingshot sync` copies project changes to the Agent. Its pull option retrieves Agent edits, and its check option previews without changing anything.
6. `slingshot env add`, `slingshot env list`, and `slingshot env remove` manage environment files kept on the Agent.
7. `slingshot ps` lists runs and sessions.
8. `slingshot stop <id>` stops a run or session.
9. `slingshot info` shows machine specifications.
10. `slingshot health` shows current resource use. Its watch option keeps it updating.
11. `slingshot top` shows live resource use with active jobs.
12. `slingshot unlink` removes the pairing.
13. `slingshot help <command>` shows every option.

## Phases

1. Setup and Phase 0: Complete. Prepared the machines and tested the idea.
2. Phase 1: Complete. Pairing, remote commands, information, and health.
3. Phase 2: Replaced. Its project detection and separate build output continue in Phase 3, where source copies replaced its project mount.
4. Phase 3: Complete. Source copies, sync, sessions, job control, environment files, and live status, accepted on two real machines.
5. Cleanup: Complete. Comments, formatting, naming, dead code, and documentation.
6. Phase 4: Planned. Connections across networks.
7. Phase 5: Planned. Notifications and port forwarding.
8. Phase 6: Planned. Model workloads and broader platform support.
9. Phase 7: Planned. Installers and public releases.

## Known gaps

1. `slingshot.toml` reads `sync.exclude` only. Split overrides are not applied yet.
2. File watchers in sessions, several Clients on one Agent, and large Node and Python projects are untested.
