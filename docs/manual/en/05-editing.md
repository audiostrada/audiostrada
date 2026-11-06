# Editing

![The timeline with its toolbar](img/timeline.webp)

## The toolbar

The bar above the tracks holds, from left to right:

1. **The tools**, keys [[1]] to [[5]]: Smart, Range, Scissors, Eraser and Mute.
2. **The edit mode** (Slip, Shuffle, Grid), the **grid value** and **Snap**, with the menu of
   what edits snap to.
3. **The editing commands**: split, duplicate, delete, heal, consolidate, strip silence,
   process clips. In a narrow window, they gather under the **»** menu.
4. **Markers and navigation**: go to start, previous marker, add a marker, next marker, go to
   end, show the playhead.
5. **The view**, on the right: the waveform menu, the track height and the horizontal zoom,
   *Zoom to fit* and *Zoom to the selection*.

Every button tells its name, keys and gesture when hovered.

## The tools

| Tool | Key | What it does |
|---|---|---|
| **Smart** | [[1]] | moves, trims, fades and selects ranges, depending on where the pointer is on a clip (below) |
| **Range** | [[2]] | dragging selects a time range anywhere, across clips and tracks |
| **Scissors** | [[3]] | a click splits the clip there; [[Alt]]-click splits every track |
| **Eraser** | [[4]] | a click deletes a clip; dragging deletes every clip it passes over |
| **Mute** | [[5]] | a click mutes or unmutes a clip; dragging does the same to every clip it passes over |

Holding [[Ctrl+Alt]] gives the scissors for as long as the keys are down. [[Escape]] goes back to
the Smart tool.

### The Smart tool

What a drag does depends on where it starts on a clip:

| Where | Drag does |
|---|---|
| Lower half of the body, or the name bar | Move ([[Alt]]: copy, onto another track too) |
| Upper half of the body | Select a range, across clips and tracks |
| Left or right edge | Trim |
| Right edge with [[Alt]] | Time stretch, pitch kept |
| Top corners | Fade in, fade out |
| Middle of a fade's curve | Bend the curve |
| Middle of the top edge | Clip volume |
| Body with [[Ctrl]] | Clip volume |
| Body with [[Ctrl+Alt]] | Slip the audio inside the clip, which stays in place |

On tracks shorter than 56 pixels, the whole body moves the clip. [[Shift]]-drag always selects a
range.

While you drag, a label beside the pointer shows the value: the move in bars and milliseconds
and the target track, the new length, the fade's length and shape, the volume in dB, the
stretch in percent.

## Selecting

Click a clip to select it; [[Ctrl]]-click adds or removes one; [[Shift]]-click selects every clip
from the last clicked one to this one, across the tracks between them. Trims, fades and volume
drawn on a selected clip apply to all the selected clips by the same amount. [[Ctrl+A]] selects
every clip, [[Escape]] clears the selection and the range.

A click on a clip selects it without moving the playhead; a click on an empty place moves it.

## Edit modes and snapping

| Mode | Key | Behaviour |
|---|---|---|
| **Slip** | [[F2]] | clips move freely |
| **Shuffle** | [[F1]] | deleting or trimming closes the gap; the following clips move up |
| **Grid** | [[F4]] | moves, trims and ranges snap to the grid |

[[G]] toggles snapping in the other modes. [[Ctrl+Shift+=]] and [[Ctrl+Shift+-]] make the grid
finer or coarser.

The **Snap** menu chooses what edits snap to: the grid, clip edges, markers, the playhead and
transients. When a position snaps, a thin line shows what pulled it.

## Fades and crossfades

- **Fades**: drag a clip's top corners. Drag the middle of the curve up or down to bend it,
  [[Alt]]-click the curve to step through the shapes, double-click it to straighten it, and
  right-click for the list of shapes: equal power, linear, fast, slow, S-curve.
- **Crossfades**: clips that overlap crossfade on their own. The overlap shows as an X with a
  handle: drag its top edge to lengthen or shorten the crossfade, its body to slide it.
  Right-click for its shape, or to remove it.
- **Fades on a range**: with a range selected, [[Ctrl+F]] puts fade-ins and fade-outs on the
  clips at the range's edges; across a cut, it makes a crossfade. With clips selected and no
  range, it puts short fades on them.

## Ranges

A range is a stretch of time on one or several tracks. Draw it with the Range tool, in the upper
half of a clip with the Smart tool, or [[Shift]]-drag. Then:

- [[Shift+R]] makes a range over the selected clips; the Edit menu also makes one from the loop
  or across every track.
- [[Ctrl+Shift+R]] crops the project to the range.
- [[Ctrl+Shift+Delete]] removes the section and closes the gap, on every track.
- [[Shift+Delete]] clears the section and keeps the gap.
- [[Shift+L]] loops the selection; right-click the range for the other commands (insert
  silence, split at the edges, set the punch).

## Clip menu

Right-click a clip: rename, split here ([[S]] splits at the playhead), duplicate ([[Ctrl+D]]),
delete, fade lengths and shapes, volume steps and *Reset to 0 dB*, normalize, mute, loop these
clips, stretch (50 % to 200 %, back to the original speed, or *Match project tempo*, which
detects the clip's tempo), transpose (semitone and octave steps, or the original pitch), slip
content, and *Open in piano roll* for a MIDI clip.

The **Inspector** ([[Alt+4]]) sets exact values, including the stretch percentage and the pitch
to the cent.

## Processing clips

From the Edit menu, or by right-clicking clips or a range:

- **Strip silence** ([[Ctrl+U]]): a window sets the threshold, the shortest silence and what is
  kept before and after the sound; the timeline darkens what will go. Keep the sound, the
  silences, or only cut at the silences.
- **Heal** ([[Ctrl+H]]) joins clips cut from the same audio back into one when they still sit
  in place.
- **Consolidate** ([[Ctrl+Shift+C]]) renders the selected clips (or the range) of each track into
  a single new file, with volume, fades, stretch and pitch applied.
- **Process clips…** ([[Alt+P]]) renders clips through a built-in effect, reverses them or
  normalizes them, as new files. The originals stay in the project; [[Ctrl+Z]] goes back.
- **Beat Detective…** ([[Alt+B]]), with a range over drum tracks, finds the hits, shown as red
  lines, on all the selected tracks together, so that the microphones of a kit are cut at the
  same places. *Separate at the hits* cuts the clips; *Separate and conform to the grid* also
  moves each slice toward the grid by the chosen strength, with short crossfades.
- **Groove…** ([[Alt+G]]) moves the hits of some tracks (a bass, guitars, keys) onto those of a
  reference (the drums) by stretching the audio between them. Tracks grooved together are cut at
  the same places so that their phase is kept; each gets the result as a new playlist. Below
  100 %, some of the feel of the take stays.

## Tracks

Double-click a track's name to rename it ([[Enter]] keeps it, [[Escape]] cancels). A name too
long for the header ends with "…" and shows whole when hovered.

The **track header menu** holds: rename, colour, *Stereo track*, duplicate, delete, move,
freeze, bounce to a new track, export the track alone, new playlist, automation.

- Track › *Add audio track* ([[Ctrl+Shift+T]]) adds a track below the selection, *Add audio
  tracks…* ([[Ctrl+Shift+N]]) several at once.
- [[Ctrl+Up]] and [[Ctrl+Down]] select the previous or next track, [[Alt+Up]] and [[Alt+Down]]
  move it.
- With two tracks selected, *Convert to a stereo track* makes one of them ("Overhead L" and
  "Overhead R" become "Overhead"). The names tell left from right, else the upper track is the
  left one; [[Ctrl+Z]] brings both back.

## Navigation and zoom

- **Zoom**: [[+]] and [[-]], or [[Ctrl]]+wheel, around the pointer when it is over the clips,
  else the playhead. [[F]] fits the whole session.
- **Zoom to the selection**: [[Z]] zooms on the range or the selected clips; [[Z]] again goes
  back to the previous view.
- **Track height**: drag the bottom edge of a track's header (double-click it for the default
  height), or [[Alt]]+wheel and [[Ctrl+Shift+Up]] / [[Ctrl+Shift+Down]] for every track.
- **Overview**: the strip under the tracks shows the whole song in miniature with the visible
  part framed. Drag the frame to scroll, drag its edges to zoom, double-click to fit.
- **Transients**: [[Tab]] and [[Shift+Tab]] take the playhead to the next or previous hit of the
  selected track's clips.
- **Markers**: [[M]] adds a marker at the playhead; [[.]] and [[,]] jump between markers.
  Right-click the ruler or an empty part of a track for *Add marker here*.

## Waveforms

The **waveform menu** in the toolbar chooses how audio is drawn:

- **Scale**: linear, logarithmic (quiet details show) or rectified (level only, twice as tall).
- **Colours**: the track's colour, or spectral colours (lows red, mids green, highs blue).
- **Channels**: separate or merged; *Each clip as tall as it can be* fills the track's height.

[[Alt+Shift+Up]] / [[Alt+Shift+Down]] or [[Alt+Shift]]+wheel zooms the waveform vertically,
[[Alt+Shift+0]] resets it. Peaks that reach 0 dBFS are marked in red. Zoomed in far enough, the
waveform shows each channel and the samples themselves, to check the phase between two tracks.

A folder track shows the clips of its tracks in miniature, one thin band each.
