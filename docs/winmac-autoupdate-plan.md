# Windows / macOS auto-update — plan

Status: plan (mission slice S1). No CI change yet; this pins the current state, the exact changes,
and the one action that is Zeke's.

## Goal

Give Windows and macOS the in-app auto-update Linux already has, so they self-update **the moment**
the Tauri signing key is added to GitHub Actions secrets.

## Current state (verified in-repo)

- **The app already trusts the signing key.** `apps/desktop/src-tauri/tauri.conf.json` →
  `plugins.updater` has the `endpoints` (`releases/latest/download/latest.json`) and the baked-in
  `pubkey`; `bundle.createUpdaterArtifacts: true`; `bundle.targets: ["deb", "appimage"]`.
- **Linux ships signed updater artifacts** and `latest.json` (via `scripts/make-latest-json.sh`)
  lists **only** `platforms.linux-x86_64`.
- **Windows/macOS ship unsigned, no updater.** `.github/workflows/cross-platform.yml` → the
  `installer` job (dispatch-only, `inputs.installers`) builds Win `nsis` + Mac `dmg` with
  `--config src-tauri/tauri.conf.ci.json`, which overrides `createUpdaterArtifacts: false` — because
  the signing key is deliberately kept out of CI. So they compile + bundle but produce no `.sig` and
  no updater artifact.

So the only things missing are: (a) signed Win/Mac updater artifacts in CI, (b) Win/Mac entries in
`latest.json`, and (c) the CI secret that lets (a) happen.

## What Tauri v2 needs (verified against the docs)

| Platform | Updater artifact | Installer artifact |
|---|---|---|
| Windows | NSIS `myapp-setup.exe` **+ `.sig`** | same `.exe` |
| macOS | **`myapp.app.tar.gz` + `.sig`** (the app bundle tarball — **not** the `.dmg`) | `.dmg` |
| Linux | `myapp.AppImage` + `.sig` | `.deb` / AppImage |

- **`latest.json` shape:** top-level `version`, `pub_date`, `notes`, and a `platforms` object keyed
  `OS-ARCH` — `windows-x86_64`, `darwin-x86_64`, `darwin-aarch64`, `linux-x86_64` — each
  `{ "signature": "<contents of the .sig>", "url": "<release asset URL>" }`. (Matches what we emit
  for Linux today.)
- **Signing env for the build:** `TAURI_SIGNING_PRIVATE_KEY` (the key's contents or a path) and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (only if the key was generated with a password). `.env` files
  are ignored — must be real env vars / CI secrets.
- **Enable artifacts:** `bundle.createUpdaterArtifacts: true` (already on in the base config; the CI
  override turns it off — that override must be dropped on the signed path).
- **macOS build note:** the updater consumes `.app.tar.gz`, which is produced from the **app**
  bundle. The current CI restricts macOS to `--bundles dmg`; the signed path must build the app
  bundle too (e.g. `--bundles app,dmg`) so the `.app.tar.gz` + `.sig` are generated.

## The changes (S2 / S3)

- **S2 — `scripts/make-latest-json.sh` → multi-platform.** Extend it to accept per-platform
  (artifact, sig) inputs and emit `windows-x86_64` / `darwin-*` entries alongside `linux-x86_64`,
  staying back-compatible with the Linux-only call. Verify by running it and validating the JSON.
- **S3 — CI signed path (secret-gated), in `cross-platform.yml`.** When
  `secrets.TAURI_SIGNING_PRIVATE_KEY` is present, build Win/Mac with `createUpdaterArtifacts: true`
  (drop the disabling `tauri.conf.ci.json` override, or a signed-variant config) and the signing env
  from secrets, and upload the updater artifact + `.sig` per OS (Win `.exe`+`.sig`, Mac
  `.app.tar.gz`+`.sig`). When the secret is absent, the existing **unsigned** path is unchanged (no
  regression). The key is never committed; only referenced via `secrets.*`.

**Arch scope:** GitHub's `macos-latest` is Apple Silicon → this produces `darwin-aarch64`. Intel
(`darwin-x86_64`) Macs would need a separate `macos-13`/x86_64 build — a documented follow-up, not
in the first cut. Windows is `windows-x86_64`.

## Zeke: the one action that's yours

To turn Win/Mac auto-update on, add the CI secret(s) (repo → Settings → Secrets and variables →
Actions → New repository secret):

1. **`TAURI_SIGNING_PRIVATE_KEY`** — paste the **entire contents** of `~/.lectern/lectern-updater.key`
   (the base64 minisign private key Tauri generated). This is the app's updater signing key; its
   public half is already baked into the app, so it must be the same key.
2. **`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`** — only if you set a password when generating that key;
   otherwise skip it (or set it empty).

The autopilot will **never** read, print, or commit this key — it only wires the workflow to consume
`secrets.TAURI_SIGNING_PRIVATE_KEY`. Once the secret exists, a dispatched cross-platform installer
run (or the next release) produces signed Win/Mac updater artifacts, and `latest.json` gains their
entries — at which point installed Win/Mac apps self-update like Linux.

## Honest limits

A real signed Win/Mac build can't be produced here (no Win/Mac machine, no CI secret, and the
dispatch build is heavy). Everything locally verifiable — the `latest.json` generator, the workflow
YAML, the config — is built and checked; the final signed-build proof is gated on the secret + a
dispatch run, which is Zeke's to trigger.

## Slice map (mirrors MISSION.md)

S2 `make-latest-json.sh` multi-platform · S3 CI secret-gated signed path · S4 RELEASING.md + the
recorded Zeke ask + audit.

## Sources

- Tauri v2 updater plugin — https://v2.tauri.app/plugin/updater/
- Tauri v2 distribution / signing — https://v2.tauri.app/distribute/
