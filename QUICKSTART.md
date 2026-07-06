# Lectern — Quickstart

Everything you need to run and use the build.

## 1. Desktop app (the GUI)
Prebuilt bundles (after `npm run app:build` in `apps/desktop`):

- **AppImage (portable, no install):**
  ```bash
  chmod +x apps/desktop/src-tauri/target/release/bundle/appimage/Lectern_0.1.0_amd64.AppImage
  ./apps/desktop/src-tauri/target/release/bundle/appimage/Lectern_0.1.0_amd64.AppImage
  ```
- **Debian/Ubuntu/Mint package:**
  ```bash
  sudo dpkg -i apps/desktop/src-tauri/target/release/bundle/deb/Lectern_0.1.0_amd64.deb
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
target/release/lectern context "fix the login flow" # see what context it would send + cost
target/release/lectern skills record --name my-task # learn a skill from the last session
target/release/lectern schedule add "nightly triage" --at +8h
```
Install on PATH: `cargo install --path crates/lectern` (gives you `lectern`).

Daemon (background scheduler + status socket): `target/release/lecternd`.

## 3. Web app (cloud control plane: accounts, usage, billing, sync)
```bash
cd apps/web
npm install
npm run setup     # creates the local SQLite DB (.data/lectern-dev.db) + tables
npm run dev       # http://localhost:3000
```
Real account creation/login works out of the box on the local DB. Sign up at
`/signup`, then the dashboard shows your real usage.

### Connect the engine to the cloud
```bash
target/release/lectern login --url http://localhost:3000   # device login → approve at /activate
target/release/lectern account                              # shows your plan + limits
target/release/lectern sync push                            # E2E-encrypted skills sync
```

### OAuth (Google + GitHub sign-in)
Put credentials in `apps/web/.env.local` (gitignored):
```
NEXT_PUBLIC_SITE_URL=http://localhost:3000
AUTH_GOOGLE_ID=…       AUTH_GOOGLE_SECRET=…
AUTH_GITHUB_ID=…       AUTH_GITHUB_SECRET=…
```
- **GitHub:** github.com/settings/applications/new → callback `http://localhost:3000/api/auth/github/callback`.
- **Google:** Google Cloud Console → Credentials → OAuth client ID (Web) → redirect `http://localhost:3000/api/auth/google/callback`.

## Production (optional)
Set `DATABASE_URL` (Turso `libsql://…`) + `DATABASE_AUTH_TOKEN`, deploy `apps/web`
to Vercel, point the engine at the deployed URL with `lectern login --url https://…`.
