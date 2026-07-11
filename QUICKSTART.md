# Lectern — Quickstart

Everything you need to run and use the build. Lectern is fully local — no accounts, no
sign-in, nothing leaves your machine.

## 1. Desktop app (the GUI)

Grab a prebuilt installer from the [latest release](https://github.com/ShrimpScript/lectern/releases/latest)
(AppImage / `.deb` for Linux, `.exe` for Windows, `.dmg` for macOS), or build it yourself.

After `npm run app:build` in `apps/desktop`, the bundles land in
`apps/desktop/src-tauri/target/release/bundle/`:

- **AppImage (portable, no install):**
  ```bash
  chmod +x apps/desktop/src-tauri/target/release/bundle/appimage/Lectern_*_amd64.AppImage
  ./apps/desktop/src-tauri/target/release/bundle/appimage/Lectern_*_amd64.AppImage
  ```
- **Debian/Ubuntu/Mint package:**
  ```bash
  sudo dpkg -i apps/desktop/src-tauri/target/release/bundle/deb/Lectern_*_amd64.deb
  lectern-desktop   # then launch from your app menu or this command
  ```

Dev mode (hot reload): `cd apps/desktop && npm install && npm run app`.

## 2. CLI (`lectern`)
```bash
cargo build --release            # or use target/release/lectern
target/release/lectern --help
target/release/lectern open .                       # index a repo as a workspace
target/release/lectern run "add a settings page"    # run a turn (mock backend)
target/release/lectern run "…" --backend claude-code --apply   # use Claude Code, write changes
target/release/lectern context "fix the parser bug" # see what context it would send + cost
target/release/lectern checkpoint list              # snapshots you can rewind to
target/release/lectern rewind <id>                  # undo a run, restore your files
target/release/lectern skills record --name my-task # learn a skill from the last session
target/release/lectern schedule add "nightly triage" --at +8h
```
Install on PATH: `cargo install --path crates/lectern` (gives you `lectern`).

Run `lectern doctor` to check the engine and your providers (Claude Code, Antigravity,
OpenCode). Your provider logins stay on your machine — Lectern never sees them.

Daemon (background scheduler + status socket): `target/release/lecternd`.

## 3. Terminal UI (`lectern tui`)
A full-screen terminal client over the same engine: `lectern tui` (finds `lectern-tui` on
your PATH, or falls back to `bun run` in a dev checkout).
