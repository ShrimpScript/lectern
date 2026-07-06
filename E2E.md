# E2E.md — proving Lectern actually works

## Web (apps/web)
1. `npm --prefix apps/web run dev -- --port 3777`, wait for 200 on `/`.
2. Screenshot via puppeteer-core harness (scratchpad `shot.mjs` pattern): full-page needs
   a scroll pass so whileInView reveals fire; verify dark + `--light` + `--reduced`.
3. Evidence: landing (hero demo mid-loop), pricing, changelog, /activate. Console must be
   clean in normal + reduced modes.

## Desktop (apps/desktop)
1. `npm --prefix apps/desktop run build` green (tsc catches wiring).
2. `npm run app:build` → launch the AppImage; smoke: new chat renders, composer chips,
   model menu lists discovered models, /conduct toggles the mode pill.
3. Streaming feel: run the MOCK backend (Settings → backend "Mock") — full event
   pipeline (thinking→plan→diff→terminal→message) with zero token spend.

## Engine
- `cargo test -p lectern-engine --lib` (48+ tests). The mock + limit backends cover the
  event stream and fallback paths deterministically.
