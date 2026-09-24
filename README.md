<h1 align="center">Slingshot</h1>

<p align="center">Use the CPU, RAM, and GPU of a powerful machine from your laptop, without leaving your own editor and terminal.</p>

<br>

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
mac/menubar/build.sh
slingshot menubar
```

<br>

<p align="center">
  <a href="docs/USAGE.md">Usage</a> ·
  <a href="docs/TROUBLESHOOTING.md">Troubleshooting</a> ·
  <a href="docs/ARCHITECTURE.md">Architecture</a> ·
  <a href="docs/ROADMAP.md">Roadmap</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>
