# Borrow

## How to run

Use two computers on the same network. Install Rust on both and enable SSH access on both. The Agent also needs SSHFS.

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
3. `borrow run <command>` runs work on the Agent.
4. `borrow info` shows machine specifications.
5. `borrow health` shows current resource use.
6. `borrow unlink` removes the pairing.
7. `borrow help` shows command help.

## Phases

1. Setup and Phase 0: Complete. Prepared the machines and tested the idea.
2. Phase 1: Complete. Pairing, remote commands, information, and health.
3. Phase 2: Main implementation present. Project mounting and local build output still need full testing on two machines.
4. Current cleanup: Comments, CLI formatting, health colors, and documentation.
5. Phase 3: Planned. Persistent sessions and job management.
6. Phase 4: Planned. Connections across networks.
7. Phase 5: Planned. Notifications and port forwarding.
8. Phase 6: Planned. Model workloads and broader platform support.
9. Phase 7: Planned. Installers and public releases.
