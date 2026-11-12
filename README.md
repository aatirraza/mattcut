# MattCut

MattCut is the Mattrics video editor: a short-form editing workflow (magnetic main track, library
of media/text/effects/transitions/filters/captions, one-click export) for Mac and Windows. It runs
entirely on your own computer: no account, no server, no internet needed. Claude can drive it over
MCP.

**Download:** see the download page (deployed from `site/` on Vercel) or this repo's
[Releases](https://github.com/aatirraza/mattcut/releases).

It is built on the [FilmCraft](https://github.com/storytold/filmcraft) engine (MIT OR Apache-2.0):
its own H.264 / HEVC / ProRes / AAC codecs, GPU compositor, exact frame timing, colour grading,
90+ effects, captions and export. MattCut adds its own interface and the Claude connector.

```
mattpremier-pro/
├── mattcraft/     FilmCraft engine checkout (github.com/storytold/filmcraft @ c9be1424); MattCut builds against it
└── mattcut/       this repo
    ├── apps/mattcut          the desktop app (Mac + Windows)
    ├── apps/mattcut-mcp      the Claude / MCP connector
    ├── apps/mattcut-server   optional office server (team mode only)
    ├── crates/office         the app <-> server protocol (team mode only)
    ├── packaging/            installers (macOS .dmg)
    └── site/                 the download page (Vercel)
```

## Build

```sh
git clone https://github.com/storytold/filmcraft mattcraft && git -C mattcraft checkout c9be1424
git clone https://github.com/aatirraza/mattcut mattcut
cd mattcut
cargo build --release -p mattcut -p mattcut-mcp
../mattcraft/target/release/mattcut          # binaries land in the shared ../mattcraft/target
```

Projects are saved in `~/Movies/MattCut Projects` (Windows: `Videos\MattCut Projects`).
Connecting Claude: [docs/mcp.md](docs/mcp.md).

### Optional team mode

`cargo build --release -p mattcut --features team` builds the original office version instead:
sign-in to a `mattcut-server` on the office network, shared projects with edit locks and 20 saved
versions, and an app that locks when it can't reach the server. See
[docs/office-server.md](docs/office-server.md).

## Release

1. Bump `version` in `Cargo.toml`.
2. Mac: run `packaging/macos/package.sh` (on a Mac). It builds Apple Silicon + Intel and writes
   `dist/MattCut-mac-universal.dmg` (ad-hoc signed; see the script for proper signing).
3. Create a GitHub Release and attach the `.dmg`. **Keep the file name `MattCut-mac-universal.dmg`**:
   the download button links to `releases/latest/download/MattCut-mac-universal.dmg`.
4. Update the version shown in `site/index.html`; pushing to `main` redeploys the page on Vercel
   (`vercel.json` serves `site/` with no build step).

## Editing

| Area | What it does |
|---|---|
| **Library** (left) | Media · Audio (music, text-to-speech) · Text styles · Stickers · Effects · Transitions · Filters · Captions. Drag onto the timeline, or press **＋** to add at the playhead. |
| **Player** (centre) | Live preview with sound, timecode, aspect ratio switch (16:9, 9:16, 1:1, 4:5, 4:3, 3:4, 21:9). Click to play/pause. |
| **Inspector** (right) | Selected clip: scale, position, rotation, opacity, effects; volume, fades, noise reduction, voice enhance, pitch; speed, reverse, smooth slow-mo; colour adjustments; text, size, colours, outline, box. |
| **Timeline** (bottom) | Overlay tracks above the **magnetic main track**, audio below. Drag to move, drag an edge to trim, drag the ruler to scrub, ⌘/Ctrl + wheel to zoom. |

### Keyboard shortcuts (⌘ = Ctrl on Windows)

| Key | Action | Key | Action |
|---|---|---|---|
| Space | Play / pause | ⌘B | Split at playhead |
| Q / W | Delete left / right of playhead | ⌫ / Delete | Delete selected |
| ⌘Z / ⇧⌘Z | Undo / redo | ⌘C ⌘X ⌘V ⌘D | Copy, cut, paste, duplicate |
| ← / → | Previous / next frame | ⇧← / ⇧→ | Back / forward 1 s |
| ↑ / ↓ | Previous / next edit point | Home / End | Start / end |
| ⌘= / ⌘- | Zoom timeline | ⇧Z | Fit timeline |
| M | Marker | N / S | Magnet / snapping on-off |
| ⌘I | Import | ⌘E | Export |
| ⌘S / ⇧⌘S | Save / save a copy | ⌘/ | Show all shortcuts |

## Optional features

```sh
cargo build --release -p mattcut --features auto-captions     # Whisper speech-to-text (on this computer)
cargo build --release -p mattcut --features natural-voices    # natural TTS voices (Kokoro)
```

## Licence and credits

MattCut is MIT OR Apache-2.0. The engine is FilmCraft by the ArtCraft team (MIT OR Apache-2.0);
see `../mattcraft/LICENSE-*`, `NOTICE` and `ATTRIBUTION.md`. MattCut's workflow follows the
common short-form editor layout; its name, icon, colours and every asset are its own, with no
third-party brand assets, code, templates, stickers or music. The logo
(`apps/mattcut/assets/logo.png`) is © Mattrics Pvt Ltd. The UI font is the operating system's
own (SF on macOS, Segoe UI on Windows), read at run time, not shipped.
