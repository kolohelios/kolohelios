First song played — 1877.

Karaoke — 1980s.

## CD+G (CD + Graphics)

Backward compatible.

- 2.27% graphics, 97.73% audio
- 26.5 kbit/s for video

**It's rough!**

- 288 x 192 — 16 of 4096 colors
- 6x12 tiles

KJs have a deep catalog of CD+G.

We could probably do better…

We can render CD+G in canvas, with `WebGL`!

## And now the AI stuff

**The DJ's Dream (Stems)**

- mix two songs together
- have music separated out — drums for beat matching, vocals for mashups and remixes

It's been expensive to get stems before.

## AI Source Separation

2024 — DJ software includes it.

- `Demucs` — MIT license
- good quality, `PyTorch`
- breaks out files

**MP4 Stems? The new standard?**

`MP4` isn't a movie file, it's just a container.

Real original audio, **NOT** MIDI recreation!

Control vocal volume (or mute entirely).

## Lyrics

Okay, so we separated the tracks. What about the lyrics?

`Whisper` from OpenAI —

- we get transcription and timing
- feed it isolated vocals from `Demucs`
- put it in an `MP4` file

`Whisper` isn't perfect; timing can be slightly off. Struggles with some lyrics.

**LLMs for lyrics** — fix the transcription errors but keep the timing right.

Can also pass along open source lyrics to it.

"Hold me closer, Tony Danza" 🤣

## Putting it together

Corrected lyrics — we still want a file.

`MP4` spec — atoms:

- standard atoms
- stem atom
- kara atom — synced lyrics for karaoke

Putting it all together — a pipeline.

Also need an open source player.

- `CREPE` can be used for musical key detection.
- `Butterchurn` library — took WinAMP stuff encoded in `WebGL`.

## Karaoke in 2026!

**`.stem.mp4` is a worthy successor to CD+G!**

`youmaynotneedelectron.com` mentioned.
