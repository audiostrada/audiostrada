# Mixing

![The mixer](img/mixer-close.webp)

## Channel strips

Each track has a strip in the **Mixer**: the output (and VCA), the pan, the fader and meter,
mute (**M**), solo (**S**), record arm (**R**), the automation mode and the track's name. The
fader takes the height the panel leaves.

- **Docked** in the main window, the strip leaves its height to the fader: the **FX** and **Send**
  buttons beside the meter (lit, with their count, when the list holds something) open the
  strip's effects and sends in a window of their own. The master has **FX** and **Mon**
  (monitoring effects).
- **In its own window** (View › *Mixer in its own window*, [[Ctrl+M]]), each strip shows its
  effect and send slots at the top: click an empty slot to add an effect or a send. [[Ctrl+M]]
  again docks it back. *Compact mixer* ([[Shift+Alt+C]]) narrows the strips.

![The mixer in its own window](img/mixer-window.webp)

## Faders and pan

- **Faders** follow a console law (0 dB at three quarters of the travel): drag them, [[Shift]]
  for fine moves, use the mouse wheel, double-click for 0 dB.
- **Pan**: the bar under the output. Drag along it ([[Shift]] for fine steps; the centre holds
  the pointer), use the wheel for 1 % steps, double-click to centre.

## Selecting tracks

Click a name plate (or a track header in the timeline); [[Ctrl]] adds or removes, [[Shift]]
extends. Colour, duplicate and delete in the track menu act on every selected track, and so do
the output menu and the fader: moving one selected fader moves the others by as many dB. Drag a
name plate to move tracks. [[Shift+Alt+M]], [[Shift+Alt+S]] and [[Shift+Alt+R]] mute, solo and
arm the selected tracks.

## Inserts

- Click *+ insert* or an empty slot to open the **effect browser**; click an effect to open its
  window.
- [[Shift]]-click bypasses an effect, [[Ctrl]]-click removes it, right-click for the other
  options.
- Drag an effect to reorder it, or onto another strip to copy it there ([[Alt]]: move it). Drop
  it on the list to choose the place, anywhere else on the strip to put it last.
- **FX** beside *+ insert* bypasses every effect of the strip; on the master, **ALL** bypasses
  every effect of the session (an instrument leading a track keeps playing).

**Effect windows** show the level going in and coming out (peaks and loudness) and the **Output**
gain after the effect, kept in the mix and the export. **Match** is a listening aid: while it is
lit, the output is brought to the input's loudness, so that bypassing the effect compares it at
equal level. It is never saved nor exported and turns off when the window closes; **Keep** moves
its correction into Output.

## The rack

The **Rack** tab ([[Alt+6]]) shows the chain of the selected track (or of a pinned channel) as
modules side by side, each with its main controls, A/B, and its editor a click away. Parallel
chains split the signal into branches of their own and mix them back.

![The rack](img/rack.webp)

## Sends, folders and buses

- **Sends** go to any track, which then acts as a bus (pre or post fader). An empty send slot
  adds the send to every selected track; tick *Cut the output to the master* first for tracks
  heard only through the bus.
- **Folders**: a track can hold others; its strip is their bus. The output menu moves the
  selected tracks into a folder, keeping their order.

## Groups

Select tracks and press [[Ctrl+G]], or tick *Folder acts as a group* in a folder's menu. The
Groups menu sets what members share: level, pan and sends (relative), mute, solo, arm, inserts
bypass, automation mode, polarity, and editing (a click on a clip or a track in the timeline
takes the members' too, so cuts, moves, trims and fades apply to all).

Hold [[Ctrl]] to use one track's control or select one clip alone. [[Ctrl+Shift+G]] switches
every group off and on.

## VCAs

Track › *Add VCA*, with tracks selected, makes a fader that moves their levels and mute together,
keeping their balance.

- **The VCA strip** lists the tracks it controls: a click selects one, the × on hover takes it out
  of the VCA. *Assign selected* adds the selected tracks.
- **Member faders** stand at the level heard, their own plus the VCA's, and move when the VCA
  moves. Moving a member's fader changes only its own level; its tooltip shows the split.
- **Automation**: a VCA has a volume lane, like any track. Its curve is added to the level of
  every track it controls, whether they have their own automation or not.

## Console colour

![Console colour](img/console-color.webp)

The **Console** menu in the mixer's bar gives the whole mix the sound of an analogue console:

| Console | Character |
|---|---|
| **Neutral** | no colour: the mix as it is |
| **British Warm** | transformers: round lows, a silky top (inspired by Neve consoles) |
| **British Bright** | tight and precise, a touch of air (inspired by SSL consoles) |
| **American Punch** | discrete amplifiers: punch and attack (inspired by API consoles) |
| **Nashville** | smooth and warm, very low distortion (inspired by Harrison consoles) |

*Console amount* sets how much colour, *Hiss and crosstalk* how much of the console's noise. Each
track can take another console (*Track console*). The power button beside the menu switches every
colour off at the same level, to compare.

A console's character depends on the level it gets: **Gain staging › Apply** sets each audio
track's input gain so that it plays around -18 dBFS (the selected tracks, or all of them).

## Strada-Channel

![Strada-Channel](img/channel-strip.webp)

Strada-Channel is a whole console channel in one insert: filters, gate, compressor, four-band EQ
and drive. Its window shows the EQ curve with its handles and the meter bridge, the console, the
track's input gain and polarity, then the modules in the order they play; drag a module by its
grip to reorder it. See *Effects* for its parameters.

## Snapshots

**Snapshots** save the state of the console and recall it, to compare two mixes.

## Master and reference

- The **master** shows the loudness (LUFS) and the highest sample peak beside its meter,
  restarted by **Reset** under the fader.
- **MONITOR** holds effects you hear but that are not exported: room correction, a reference.
- **REF** beside it (also in the transport bar, [[Alt+R]]) compares the mix with a finished song
  (Transport › *Reference track…* to choose it). Click it to hear the reference instead of the
  mix, at the same place in the song and, by default, at the mix's loudness, so that you compare
  the sound and not the level. It is never exported.
