Karaoke in 2026 deserves a better file format than the 1980s gave it — and the pieces to build one are now lying around in open source. The argument: `.stem.mp4` is a worthy successor to `CD+G`.

## The CD+G inheritance

The first song ever played dates to 1877, karaoke to the 1980s, and then `CD+G` — CD plus graphics — arrived backward-compatible. The split is comically lopsided: 2.27% graphics to 97.73% audio, with video squeezed into 26.5 kbit/s. The result is rough — 288x192, 16 of 4096 colors, 6x12 tiles. KJs (karaoke jockeys) still guard deep `CD+G` catalogs. We can probably do better — and we can render `CD+G` on a canvas with `WebGL`.

## Stems are the DJ's dream

The DJ's dream is **stems** — separating a song into its parts so you can beat-match on drums or build mashups and remixes from isolated vocals. Stems were historically expensive, until AI source separation changed the math; by 2024, DJ software shipped it. `Demucs` (MIT-licensed, `PyTorch`) is good quality and breaks audio into separate files. Maybe MP4 stems are the new standard? **MP4 isn't a movie file** — it's just a container. And this is *real original audio*, not a MIDI recreation, so you can ride the vocal volume or mute it entirely.

## Lyrics, with timing

`Whisper` (OpenAI) gives transcription plus timing — feed it the isolated vocals from `Demucs` and stash the result in the MP4. `Whisper` isn't perfect: timing drifts and it mangles some lyrics (hence "Hold me closer, Tony Danza"). **LLMs fix the transcription** while preserving the timing, and you can hand them open-source lyrics too.

## Assembling the file

The MP4 spec uses **atoms** — standard atoms, a stem atom, and a `kara` atom for synced lyrics. Wire it into a pipeline and add an open-source player. (`CREPE` handles musical key detection; `Butterchurn` revives the old WinAMP visualizers in `WebGL`.) See also `youmaynotneedelectron.com`.
