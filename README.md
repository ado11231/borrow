<h1 align="center">Slingshot</h1>

<p align="center">Use the CPU, RAM, and GPU of a powerful machine from your laptop, without leaving your own editor and terminal.</p>

<p align="center"><img src="docs/media/demo.gif" alt="slingshot run building a project on another machine" width="800"></p>

## What It Does

* Runs builds, tests, servers, containers, and coding agents on a powerful machine while you keep working on your laptop.
* Feels local. Typing, colors, Ctrl C, and exit codes behave as they do on your own machine.
* Copies only your source. Build output such as `target` and `node_modules` stays on the powerful machine.
* Always says where work runs: `▶ Running on archbox via tailnet`.
* Keeps sessions running when your laptop sleeps or loses its network.
* Connects on the same network, over a tailnet, or from anywhere through iroh, with no account and no open router port.
* Shows live CPU, RAM, GPU, and VRAM in the macOS menu bar, and notifies you when runs finish.

<p align="center"><img src="docs/media/menubar.png" alt="The Slingshot menu bar panel showing CPU, RAM, GPU, and workspace" width="360"></p>

## Two Machines

| Name | Meaning |
| --- | --- |
| **Client** | The machine you work on, such as your laptop. |
| **Agent** | The powerful machine that does the work. |

* Both can run macOS or Linux.
* Slingshot is tested between a Mac Client and an Arch Linux Agent.

## Requirements

| Machine | Needs |
| --- | --- |
| Both | [Rust](https://rustup.rs) and `rsync` |
| Agent | An ssh server and `tmux` |
| Mac Client | The Xcode command line tools, for the menu bar app |

* Slingshot checks each machine and prints the exact command for anything missing.

## Get Started

**1. Install Slingshot On Both Machines**

```sh
git clone https://github.com/ado11231/slingshot.git
cd slingshot
cargo install --path crates/slingshot-cli
```

**2. Start The Agent On The Powerful Machine**

```sh
slingshot start
```

**3. Link The Machine You Work On, Using The Printed Code**

```sh
slingshot link <code>
```

* Both machines must be on the same network or tailnet for this step only.
* Slingshot then offers to install the tools you use, such as Docker, Node, and Claude Code, on the Agent. It shows every command and asks first.

**4. Run Work From Your Project Folder**

```sh
slingshot run cargo build
```

**5. Open A Session That Survives Disconnects**

```sh
slingshot attach
```

**6. Show The Agent In Your Menu Bar On macOS**

```sh
slingshot menubar
```

## Everyday Commands

| Command | Does |
| --- | --- |
| `slingshot run <command>` | Copies your changes, then runs the command on the Agent. |
| `slingshot attach` | Opens or returns to a lasting session on the Agent. |
| `slingshot sync --pull` | Brings edits made on the Agent back to you. |
| `slingshot health --watch` | Shows live CPU, RAM, GPU, and disk use. |
| `slingshot ps` | Lists running jobs. |
| `slingshot tools` | Installs tools the Agent is missing, after asking. |

* Every command and option is in [USAGE.md](docs/USAGE.md).

## Status

* Slingshot is early. It has been tested between one Mac and one Arch Linux machine.
* What works, what was tested, and what does not work yet are in the [roadmap](docs/ROADMAP.md).

<br>

<p align="center">
  <a href="docs/USAGE.md">Usage</a> ·
  <a href="docs/TROUBLESHOOTING.md">Troubleshooting</a> ·
  <a href="docs/ARCHITECTURE.md">Architecture</a> ·
  <a href="docs/ROADMAP.md">Roadmap</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>
