# Export

![The export window](img/export.webp)

## Exporting the mix

File › Export ([[Ctrl+Shift+E]]) opens the export window:

| Setting | Choices |
|---|---|
| **Format** | WAV 16-bit, 24-bit or 32-bit float; FLAC 16 or 24-bit; MP3; Opus |
| **Sample rate** | the project's, or any other |
| **Dither** | triangular (TPDF), when going down to 16 bits |
| **Normalize** | to a peak or to a loudness (LUFS) |
| **Range** | the whole project or the loop range |
| **Tail** | seconds added after the end, for reverbs and delays |
| **Stems** | one file per track, soloed, besides or instead of the mix |

Exports run faster than real time, in the background. The metronome and the monitoring effects
are never exported.

## Render queue

*Add to queue…* keeps an export, with the project as it is at that moment, to render later with
others. The **Render queue** lists them; render them all in one go, when you leave the computer.

## A single track

The track header's menu exports a track alone, after its effects.

## Freeze and bounce

- **Freeze** renders a track with its effects and plays the result instead, to save processing
  power. Unfreeze it to change its effects.
- **Bounce** renders a track onto a new track.

## From a terminal

`audiostrada render song.strada --output mix.wav` mixes a project (or audio files) into a WAV
file without opening the window.
