# MIDI and the piano roll

![The piano roll](img/piano.webp)

## MIDI clips

Track › *Add instrument track* ([[Ctrl+Shift+I]]) creates a track with the built-in synth.
Double-click an empty part of an instrument track, or Track › *New MIDI clip*
([[Ctrl+Shift+M]]), to create a clip, then edit it in the **Piano roll** ([[Alt+2]];
[[Ctrl+P]] puts it in its own window). A clip grows to hold notes pasted past its end.

## Tools

Keys [[1]] to [[5]] while the piano roll has the focus:

| Tool | What it does |
|---|---|
| **Pointer** | selects, moves ([[Alt]]-drag copies) and resizes notes from either edge, draws a rubber band in the empty space, adds a note with a double-click |
| **Pencil** | adds a note and drags its length |
| **Brush** | paints a note on each grid step |
| **Scissors** | cut notes where they are clicked |
| **Eraser** | removes what it passes over |

The piano roll has its own grid, down to 1/32 with triplets; hold [[Ctrl]] to place notes freely.

## Navigation

The wheel scrolls through the keys ([[Shift]]: in time), [[Ctrl]]+wheel zooms time and
[[Alt]]+wheel the keys, around the pointer. The middle button drags the view; the frame button
fits the notes. The view follows the playhead while playing; click the ruler to move it.

Notes sound while you click the keys on the left, add a note or drag one; the keys light up as
notes play. Double-click a key to select its notes.

## Scale and chords

- **Scale** shades the keys outside it, can *fold* the keyboard to the scale's keys and those in
  use, and *keep the notes in it* when they are drawn, moved or transposed. *Detect from the
  notes* finds it.
- **Chords**: the chord button makes the pencil and the brush add a whole chord: the one the
  scale builds on the key (triad or seventh), or a chosen kind, inverted and voiced (close, open,
  drop 2, spread). The strip above the ruler names the chords the notes make.
- **Ghost notes**: the other MIDI clips show faintly; double-click one to edit its clip.

## Transform

From the toolbar or the right-click menu. Quantize (strength, swing, ends), humanize, strum,
arpeggiate, harmonize, chords from notes, variation, randomize, velocity shape and stretch open a
panel that previews the result on the notes; [[Enter]] applies, [[Escape]] cancels. Legato,
reverse, mirror, glue and mute apply at once. [[Q]] quantizes the selection.

## The lane

The lane under the notes edits **velocity** or **probability** (a note under 100 % plays only
some of the times) as bars: drag one (the selected ones follow), drag across to paint,
[[Shift]]-drag for a straight ramp, right-click for shapes (crescendo, compress, accents).

It also draws controllers and, with one note selected, its own pitch, pressure or timbre (MPE),
drawn inside the note; *Own channels* gives each note its channel.

## Keys

| Key | Command |
|---|---|
| [[Ctrl+C]] / [[Ctrl+X]] / [[Ctrl+V]] | copy, cut, paste at the playhead |
| [[Ctrl+D]] | duplicate right after |
| [[Left]] / [[Right]] | nudge by the grid |
| [[Up]] / [[Down]] | transpose ([[Shift]]: an octave) |
| [[Q]] | quantize |

## Controllers and MIDI learn

A keyboard's knobs turn the built-in synth of the armed tracks with the standard controllers:
CC 74 cutoff, 71 resonance, 73 attack, 75 decay, 72 release, 7 volume (in MPE mode, CC 74 shapes
each note instead).

**MIDI learn**: right-click a fader, pan knob, send level or effect parameter › *MIDI learn*,
then move a control on your MIDI controller. A learned controller does only what it was learned
for. The arrows of an effect's presets learn a button the same way.

A controller learned on an effect's parameter is kept for every project, by the kind of effect:
learned on Strada-Synth's first macro, it turns the first macro of the Strada-Synth of the armed
tracks (else of the tracks selected) in any project. Those of the mixer's faders, pans and sends
belong to the project. A controller turns a time or a frequency as its knob does, finely at the
bottom of its travel.

A controller moves a control by as much as it turns, from the value the control has: an
endless encoder, or a knob left elsewhere by another preset, never makes it jump. Its first
message only tells where it is. A knob or a fader that has stops catches up after one full
sweep.

**Program changes** sent by a keyboard (its preset buttons) choose the preset of the built-in
instrument of the armed tracks: program 1 is the first preset of its menu.
