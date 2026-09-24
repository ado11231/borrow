# Roadmap

* What Slingshot does today, what has been tested, and what comes next.
* A feature is listed as complete only after it has run on real machines.
* Last updated September 23, 2026.

## Contents

1. [Status](#status)
2. [How The Work Is Organized](#how-the-work-is-organized)
3. [Complete](#complete)
4. [In Progress](#in-progress)
5. [Planned](#planned)
6. [Known Limitations](#known-limitations)
7. [Test Record](#test-record)

## Status

| Phase | Goal | Status |
| --- | --- | --- |
| 1 | Pairing, remote commands, and hardware details on a local network | Complete |
| 2 | Project recognition, and keeping build output on the Agent | Replaced by phase 3, which kept both |
| 3 | Source copies, sync, sessions, jobs, environment files, and live health | Complete |
| 4 | Reaching the Agent from any network | Final testing |
| 5 | Menu bar app, notifications, and automatic port forwarding | In progress |
| 6 | AI model tools, pairing across networks, and more platforms | Planned |
| 7 | Installers, packages, and a public release | Planned |

## How The Work Is Organized

* Work is split into phases.
* Each phase leaves Slingshot usable.
* Later phases build on earlier ones, so none are skipped.

## Complete

### Phase 1: Pairing And Remote Commands

* `slingshot start` checks the Agent for an ssh server, `rsync`, and `tmux`, and prints a single use pairing code.
* `slingshot link` installs the Client's key on the Agent. The Client needs no ssh server.
* `slingshot run` runs a command on the Agent with live output, typing, Ctrl C, and the real exit code.
* `slingshot info` and `slingshot health` show the Agent's hardware and current use.

### Phase 3: Source Copies, Sessions, And Live Status

* The Agent keeps its own copy of each project, kept in step by three way sync. Conflicts stop the sync, so no edit is lost.
* Build output for Rust, Node, and Python stays in separate Agent storage.
* `slingshot attach` opens or returns to one lasting session per project.
* `slingshot sync` pushes changes, pulls them with `--pull`, and previews with `--check`.
* `slingshot env` stores environment files on the Agent, apart from the source copy.
* `slingshot ps` and `slingshot stop` list and stop jobs.
* `slingshot health --watch` and `slingshot top` refresh live.
* `slingshot unlink` removes this Client's key and environment files from the Agent.

## In Progress

### Phase 4: Reaching The Agent From Any Network

* **Built:**
  1. The Client tries the local network, then the tailnet, then iroh, and uses the first that answers. Every run names its path.
  2. iroh connects directly when it can, and through a relay when it cannot. A relay only sees encrypted data.
  3. The Agent accepts iroh connections only from paired Clients.
  4. `slingshot start` keeps the Agent awake.
  5. Error messages tell an Agent that is off apart from a Client that is no longer paired.
* **Remaining:**
  1. Test a lost connection during a build over iroh.
  2. Test an Agent restart over iroh.

### Phase 5: Polish

* **Built:**
  1. A macOS menu bar app showing CPU, RAM, GPU, VRAM, and workspace space.
  2. An offline screen with the cause, the fix, and a Try again button.
  3. Notifications for finished, failed, and interrupted runs, the Agent going offline or returning, and low memory, low disk, or a hot GPU.
* **Remaining:**
  1. Automatic port forwarding, so the Agent's port 3000 appears at `localhost:3000` on the Client.
  2. Notifications when a server is ready and when a job waits for input.
  3. A rework of `slingshot attach`, which works but feels slow and unclear.

## Planned

### Phase 6: AI Models, Pairing Anywhere, And Reach

* Support for Ollama and ComfyUI: small language models, embeddings, Whisper, and image generation.
* Pairing across networks, with a password based exchange that keeps the code safe through a relay.
* A setting to use your own iroh relay.
* Stronger support for Linux Clients with Linux Agents.

### Phase 7: Release

* Ready to run programs for macOS and Linux, on Intel and ARM.
* A one line installer that also starts `slingshot start` when the Agent turns on.
* Packages for Arch Linux (AUR) and Homebrew.
* `slingshot doctor`, one command that checks a setup and explains each fix.
* A license, issue templates, and automatic builds and releases.
* **Done when** a newcomer goes from install to a working `slingshot run` in under five minutes, with no help.

## Known Limitations

* `slingshot start` does not start on its own when the Agent restarts.
* `slingshot attach` feels slow and unclear. Some tools, such as Codex, did not run inside a session, likely because of the session's shell or terminal settings.
* File watchers inside sessions, several Clients on one Agent account, and large Node and Python projects are untested.
* `slingshot.toml` supports only `sync.exclude`. Other settings have no effect.
* Pairing across networks is not supported. Both machines must share a network or tailnet to link.
* The notifications for an Agent coming back online, and the resource warnings, have not been seen on real machines.

## Test Record

* All tests ran between a Mac Client and an Arch Linux Agent named archbox.

### Phase 3: September 13, 2026, Same Network

1. The first `slingshot run cargo build` copied 47 files and finished in 9 seconds. The second started in 1 second.
2. Subfolder runs, exit codes, typing, and Ctrl C behaved as they do locally. The Agent's copy held no `.git` and no `target`.
3. Push, pull, preview, an edit made only on the Agent, and a conflict on both sides all worked. The conflict was refused and nothing changed.
4. Environment files were private to the Agent account, visible to `run`, and never synced.
5. `attach`, leaving, returning, and `stop` all worked.
6. A release build in `attach` kept running while the Client was offline for four minutes, and `attach` returned to it after.
7. Restarting `slingshot start` kept the session. Restarting the Agent marked it interrupted.
8. `unlink` refused while a session was active, then removed the key and environment files after `stop`.

### Phase 4: September 22, 2026

1. `slingshot run` worked over the home network, over the tailnet from a phone hotspot, and over iroh from the hotspot with Tailscale off.
2. Over iroh, the first ssh connection took about 1.5 seconds, and later ones about 0.3 seconds.
3. With `slingshot start` stopped, `run` failed after about 20 seconds with a message naming the cause.
4. The Agent stayed awake while `slingshot start` ran.
5. Over iroh, an unpaired Client was refused and a paired one was accepted.
6. `slingshot sync` and `slingshot attach`, including leaving and returning, worked over iroh.

### Phase 5: September 23, 2026

1. The menu bar app built, installed, registered to start at login, and showed live values from archbox. Starting after a real log out has not been checked.
2. With `slingshot start` stopped, the offline screen named the cause and offered `slingshot start`, then returned to live values on its own.
3. Real runs produced the right notifications: finished, failed, and interrupted, and none for a quick `echo hi`.
4. The offline notification appeared when `slingshot start` was stopped.
