# Contributing to Lectern

Welcome. The short version:

- **Build everything**: `cargo build` (engine + CLI + daemon), then per surface:
  `apps/desktop` → `npm install && npm run app` (needs WebKitGTK dev libs on Linux),
  `apps/tui` → `bun install && bun run src/index.tsx`. The website is a separate repo: [lectern-web](https://github.com/ShrimpScript/lectern-web).
- **Tests**: `cargo test --workspace` must stay green; TUI changes get a drive with
  `scripts/tui-drive.sh` (private tmux server); CI runs Linux + Windows on every
  engine-touching PR and macOS weekly.
- **PRs**: small, verified slices. Say what you ran or drove, not just what
  compiles. docs/ and CHANGELOG.md show how changes have shipped so far.
- **Security issues**: see SECURITY.md — please don't open public issues for those.

### Shipping a user-facing feature

So a feature actually reaches users on every surface, do these as part of the PR:

1. **Changelog** — add a bullet under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md).
   This is the single source: it becomes the website `/changelog`, the GitHub Release
   notes, and the in-app "what's new" (see [RELEASING.md](RELEASING.md)).
2. **Onboarding** — if it's a *headline* feature (something a new user should know on day
   one), add one short line to the desktop onboarding (`apps/desktop/src/Onboarding.tsx`).
   Keep onboarding short; only headline features go here.
3. **Docs** — if it has a surface worth explaining, add or update the relevant
   `/docs` page in the [lectern-web](https://github.com/ShrimpScript/lectern-web) repo.

A bug fix or internal change needs only the changelog bullet (or nothing, if not
user-visible).

Lectern is **Apache-2.0** (see LICENSE). By contributing you agree your
contributions are licensed under the same terms (inbound = outbound). No CLA.
