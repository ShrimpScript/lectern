# Releasing Lectern

How a Lectern release is cut. The goal is that every release reaches users: the app
updates itself, and GitHub, the website, and the in-app "what's new" all agree on what
changed — because they are all generated from one source, [CHANGELOG.md](CHANGELOG.md).

## Versioning

Lectern follows [Semantic Versioning](https://semver.org/): `MAJOR.MINOR.PATCH`.

- **PATCH** (`0.6.0 → 0.6.1`) — bug fixes and small improvements, no new surface.
- **MINOR** (`0.6.0 → 0.7.0`) — new user-facing features, backward compatible.
- **MAJOR** (`0.x → 1.0`) — breaking changes to a stable interface (CLI flags, the engine
  store, the daemon protocol). Pre-1.0, breaking changes may land in a MINOR with a clear
  note in the changelog.

The version lives in these places and must match:

- `Cargo.toml` → `[workspace.package] version` — the CLI, daemon, and TUI inherit it
  (`version.workspace = true`).
- `apps/desktop/src-tauri/Cargo.toml` → `version` — the desktop app is its own crate,
  outside the workspace, so it carries its own version.
- `apps/desktop/src-tauri/tauri.conf.json` → `version`.

## When to release

Release when there is something worth updating *for* — not on every merge.

- Cut a release once `[Unreleased]` in the changelog has accumulated at least one
  user-facing feature (MINOR) or a set of meaningful fixes (PATCH).
- If user-facing changes are sitting in `[Unreleased]`, don't let them go stale — aim to
  ship within roughly two weeks of them landing.
- Security fixes ship as soon as they're ready.
- On-demand releases are always fine.

Every user-facing PR should add a bullet to `[Unreleased]` in `CHANGELOG.md` as part of
the change, so cutting a release never means reconstructing history.

## Cutting a release

1. **Pick the version** per the rules above.
2. **Update the changelog.** Move the `[Unreleased]` bullets into a new
   `## [X.Y.Z] - YYYY-MM-DD` section, leave `[Unreleased]` empty, and update the compare
   links at the bottom of the file.
3. **Bump the version** in `Cargo.toml` and `tauri.conf.json` (they must match).
4. **Open a PR** with the changelog + version bump; merge on green CI.
5. **Tag** the merge commit `vX.Y.Z` and push the tag.
6. **Build the signed desktop artifacts.** From `apps/desktop`, with the signing key in the
   environment, build the AppImage and its `.sig`:

   ```
   export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.lectern/lectern-updater.key)"
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""   # set if the key has one
   npm --prefix apps/desktop run app:build
   ```

   The signed `Lectern_X.Y.Z_amd64.AppImage` and `.AppImage.sig` land in
   `apps/desktop/src-tauri/target/release/bundle/appimage/`. The Windows/macOS installers
   come from the `Cross-platform` workflow (`workflow_dispatch` with `installers: true`).
7. **Generate the update manifest.** Point it at the release download for this version:

   ```
   scripts/make-latest-json.sh X.Y.Z \
     apps/desktop/src-tauri/target/release/bundle/appimage/Lectern_X.Y.Z_amd64.AppImage \
     apps/desktop/src-tauri/target/release/bundle/appimage/Lectern_X.Y.Z_amd64.AppImage.sig \
     > latest.json
   ```

   To embed the release notes in the manifest, extract them from the changelog first and
   pass the file as a 4th argument:

   ```
   scripts/changelog-section.py X.Y.Z > notes.md
   scripts/make-latest-json.sh X.Y.Z <appimage> <sig> notes.md > latest.json
   ```
8. **Create the GitHub Release** for the tag, using that same changelog section as the body:

   ```
   scripts/changelog-section.py X.Y.Z > notes.md   # exits non-zero if the version is missing
   # then create the release with notes.md as the body (via the API or gh)
   ```

   Attach: the installers, the CLI tarball, `SHA256SUMS.txt`, the signed AppImage + `.sig`,
   and `latest.json`. Because the app's updater endpoint is
   `releases/latest/download/latest.json`, publishing the release makes installed apps
   discover the update automatically.
9. **Verify auto-update.** An installed older AppImage should detect the new version, show
   its "what's new", download, and relaunch.
10. **Confirm the website is current** — the changelog page reflects the new release (it
    reads `CHANGELOG.md`), and the docs describe anything new.

## The signing key

The desktop updater verifies each update against a public key shipped in the app; releases
are signed with the matching **private** key. The private key never leaves the maintainer's
machine and is never committed — it is passed to the build via the `TAURI_SIGNING_PRIVATE_KEY`
environment variable at release time. Losing it means shipping a new public key in a future
release; leaking it means anyone could sign an "update", so treat it like any other secret.
