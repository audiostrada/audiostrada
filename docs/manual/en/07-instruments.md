# Instruments

AudioStrada comes with two instruments: **Strada-Synth** and **Strada-Drums**. Any CLAP, VST3 or
LV2 instrument can replace them: right-click the instrument's insert › *Change instrument…*, or
*+ insert* › *Instrument*. An instrument always goes first on its track.

## Strada-Synth

![Strada-Synth](img/fx/synth.webp)

A polyphonic synthesizer: two oscillators with unison, a sub oscillator and noise, analog drift,
a filter, glide and MPE. Its window fits without scrolling: Tone, Brightness and Playing side by
side, Shape below (drag the envelope's corners), then the keyboard.

- **Presets** hold ready-made sounds: soft keys, pads, strings, brass, pluck, bell, lead, basses.
- **The keyboard** plays the synth without arming the track: click or drag across the keys
  (lower on a key plays louder), or turn on *Computer keyboard* to play with the letters: A S D F
  G H J K for the white keys, W E T Y U for the black ones, Z and X for the octave. Keys go by
  position, so AZERTY works the same.
- **MPE controller** is for MPE controllers (Roli Seaboard, LinnStrument, Osmose), which send
  each note on its own channel.

<!-- effect:synth -->

## Strada-Drums

![Strada-Drums](img/fx/drums.webp)

Track › *Add drum machine track* creates a track with Strada-Drums: sixteen pads on the General
MIDI drum notes, so that the piano roll and any pad controller play them.

### Pads

A pad lights up while it sounds and plays when clicked (lower on the pad plays louder). Clicking
it also selects it, and the **pad tab** below shows its sound:

- **Character**: 808, 909 or acoustic, or *Load…* a sample of your own.
- **Sound**: level, pan, tune. **Envelope**: attack, decay. **Pitch**: amount and length of the
  pitch drop. **Filter**: tone, low cut, high cut. **Sends**: to the kit's delay and reverb.

The closed hat chokes the open one. The presets hold whole kits by style (hip hop, trap, drill,
house, electro, Latin, Afro, lo-fi, drum and bass…).

The **Kit effects** tab holds the compression, colour, EQ, delay and reverb of the whole kit.

### Pattern

The **Pattern** tab is a step sequencer over the track's MIDI clip at the playhead: what you
draw here is what the piano roll shows. Without a clip, *New pattern* creates one at the
playhead's bar, with the number of bars chosen beside it.

- **Steps**: click to set or clear a step, drag across to paint, drag a step up or down for its
  velocity, right-click for its roll, flam and chance.
- **Swing** pushes the off sixteenths toward triplets.
- **Start from a rhythm** writes a ready-made pattern: boom bap, lo-fi, trap, drill, Jersey club,
  house, techno, electro, dembow, phonk, UK garage, afrobeats, amapiano, dancehall, funk, rock,
  drum and bass. Each shows its usual tempo; the project's tempo is left alone.
- **Clear** empties the variation shown.

### Variations and chain

Bars play **variations**: A, B, C… up to Z, and a **Fill**. A variation is edited once for all
the bars that play it.

- **+ B** puts a second pattern on every other bar, **+ Fill** a fill at the end of every four
  bars, with a snare build-up to start from. Once B exists, **+ C**, **+ D**… add a variation on
  a new bar at the end, copied from the one shown.
- **The chain** shows what each bar plays. A click on a bar steps to the next variation in use;
  right-click for every choice, or to make the bar a new variation that keeps its notes.
- **Bars**: the number at the start of the chain sets the pattern's length, from 1 to 64 bars.
  New bars repeat the chain; bars past the end go, with their notes.

> **Tip** To vary a groove without starting over: build A, add B, then right-click a bar of the
> chain › *New: C* and change only what differs.
