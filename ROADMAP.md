# MattCut roadmap

The goal is CapCut Pro feature parity for a 10-person office on Mac and Windows: same features,
panels and shortcuts, with our own name, look and assets.

Legend: ✅ in v0.1 · 🟡 partly (engine has it, UI is basic) · ⏳ planned

## v0.1 (this release): the foundation

| Feature | Status |
|---|---|
| Office server: sign-in with office key, user list, LAN-only, licence lease, lock when off-network | ✅ |
| Shared office projects, edit locks, read-only copies, 20 versions per project | ✅ |
| Server discovery on the network, "who is online / editing what" | ✅ |
| MCP connector for Claude (live app or headless) | ✅ |
| CapCut-style layout: library / player / inspector / timeline | ✅ |
| Magnetic main track, overlay tracks, audio tracks | ✅ |
| Split, Q/W trim, delete (ripple on main), move, edge trim, snapping, markers, undo/redo | ✅ |
| Aspect ratios 16:9 · 9:16 · 1:1 · 4:5 · 4:3 · 3:4 · 21:9 | ✅ |
| Import video / audio / images / GIF / SRT / VTT, drag & drop from Finder / Explorer | ✅ |
| Preview with sound (audio-clock playback) | ✅ |
| Text: 8 styles, edit text, size, colour, outline, shadow, box, align | ✅ |
| Effects (~100 video), transitions (~30), filters (looks) | ✅ |
| Colour adjust: exposure, contrast, highlights, shadows, whites, blacks, saturation, vibrance, temperature, tint, sharpen, fade, auto match | ✅ |
| Transform: scale, position, rotation, opacity, fit to frame | ✅ |
| Speed 0.1×–10×, reverse, smooth slow-mo (frame blending / optical flow) | ✅ |
| Audio: volume, fades, noise reduction, enhance voice, de-echo, de-hum, pitch voice changer | ✅ |
| Text to speech (built-in voices; natural voices as a build feature) | ✅ |
| Captions: auto captions (build feature), manual captions, edit, import / export SRT | ✅ |
| Export: MP4 H.264 / HEVC, ProRes, GIF, AAC, WAV; 480p–4K; 24–60 fps; quality presets | ✅ |
| Crash recovery and background saves | ✅ |

## v0.2: everyday CapCut workflow

| Feature | Status |
|---|---|
| Thumbnails in the media library and filmstrips / waveforms on the timeline | ⏳ |
| Keyframe animation UI (diamond buttons per property, as CapCut) | 🟡 engine ready |
| Text animations in / out / loop (presets) | ⏳ |
| Clip animations (zoom in, slide, spin, … presets) | ⏳ |
| Speed curves (montage, hero, bullet, custom) | 🟡 engine has time remapping |
| Freeze frame, extract audio, detach audio, replace clip | ⏳ |
| Crop / mask (shapes, feather, invert) | 🟡 engine ready |
| Chroma key / remove green screen | 🟡 engine has Ultra Key |
| Canvas: blur / colour background for vertical video | ⏳ |
| Picture-in-picture drag handles on the player | ⏳ |
| Track mute / hide / lock buttons, track heights | ⏳ |
| Beat markers (auto beat detection for music) | ⏳ |
| Voice-over recording | 🟡 engine ready |
| Mac ↔ Windows media path mapping for the shared footage drive | ⏳ |
| Windows installer (.msi) and macOS signed app (.dmg) | ⏳ |

## v0.3: "Pro" and AI features

| Feature | Status |
|---|---|
| Auto reframe for vertical video (subject tracking) | ⏳ |
| Remove background without green screen (AI segmentation) | ⏳ |
| Remove silences / filler words from talking-head videos | 🟡 engine has transcript editing |
| Text-based editing (cut by deleting words) | 🟡 engine ready |
| Script to video / AI captions styling / caption templates | ⏳ |
| Stabilise | 🟡 engine has Warp Stabilizer |
| Motion tracking (text follows a face/object) | 🟡 engine has mask tracking |
| Templates: office template library on the server (save any edit as a template) | ⏳ |
| Office asset library on the server: shared stickers, music, fonts, LUTs, brand kit | ⏳ |
| Multi-camera editing | 🟡 engine ready |
| Comments / review links inside the office | ⏳ |
| Admin page on the server (users, projects, versions, restore) | ⏳ |

## Not copied, by design

CapCut's name, logo, icons, artwork, and its template / sticker / effect / music libraries
belong to their owners. MattCut matches the workflow; the content library is our own office
library (v0.3).
