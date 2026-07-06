# Lectern Desktop (Tauri v2 + React)

The agent workspace GUI. Embeds the Lectern **engine** (`crates/engine`) via Tauri
commands and renders the normalized agent event stream.

> ⚠️ **Build prerequisites (Linux).** Tauri needs WebKitGTK + GTK system libraries,
> which require root to install. On Debian/Ubuntu/Mint:
>
> ```bash
> sudo apt update && sudo apt install -y \
>   libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
>   libayatana-appindicator3-dev patchelf build-essential curl wget file
> ```
> (These were NOT present in the dev environment, so the GUI hasn't been built yet —
> the engine + CLI are fully built and tested. After installing the above, the
> commands below work.)

## Run (after installing the system libs)
```bash
npm install
npm run app        # tauri dev — launches the desktop app (vite + the Rust host)
npm run app:build  # tauri build — produces .deb + AppImage
```

## Icons
`npm run tauri icon path/to/logo.png` generates the icon set into `src-tauri/icons/`
(required for `app:build`; `app` dev may run without it). A vector Lectern mark is in
`~/Documents/Lectern-Brain/02-Design-System/Brand & Identity.md`.

## Structure
```
src/                 React UI (App.tsx renders the workspace shell + turn stages)
src-tauri/           Rust host (embeds lectern-engine; commands: engine_backends, run_session)
  tauri.conf.json    window + bundle config (.deb/appimage)
  src/main.rs        Tauri builder + commands
```

## Status / next
- ✅ Scaffold + design-token UI shell + engine-backed `run_session` command.
- 🔲 Stream events to the window incrementally (`app.emit`) instead of returning a batch.
- 🔲 File tree + Changes pane + Apply gate UI; backend picker; Settings (MCP, fallback).
- 🔲 Connect to `lecternd` over the local socket instead of embedding (for shared state with the CLI).
See `~/Documents/Lectern-Brain/03-Architecture/Desktop App Stack.md`.
