# Mixing

![The mixer](img/mixer-close.webp)

## Channel strips

Each track has a strip in the **Mixer**, in the order the signal goes through it: the input with
its gain and polarity (**Ø**), the channel strip, the inserts, the sends, the output (and VCA),
then the pan beside the automation mode, the fader and meter, mute (**M**), solo (**S**), record
arm (**R**) and the track's name, marked **BUS** for a bus and **VCA** for a VCA. The fader
takes the height the panel leaves. Right-click the empty space beside the strips to add a track,
a bus or a VCA; a click there clears the selection.

Solo keeps what the soloed track goes to audible. Soloing a bus also plays the tracks routed
into it, and the tracks sending to it through their sends only: a soloed reverb bus plays the
reverb alone, without the dry sound of what feeds it.

- **Docked** in the main window, the strip leaves its height to the fader. Beside the meter, in
  the signal's order: the channel strip's button, then **FX** with a pill per effect (a click
  opens the effect, [[Shift]]-click bypasses it, [[Ctrl]]-click removes it) and **+** to add one,
  then **Sends** with a pill per send showing its level. **FX** and **Sends** (lit, with their
  count, when the list holds something) open the whole list just under them, to reorder or add;
  a click elsewhere closes it. The master has its channel strip, **FX** and **Mon** (monitoring
  effects).
- **In its own window** (View › Mixer › *In its own window*, [[Ctrl+M]]), each strip shows its
  effect and send slots: click an empty slot to add an effect or a send. The lists are as tall as
  the longest one, plus an empty slot, and leave the rest to the faders. [[Ctrl+M]]
  again docks it back. *Compact mixer* ([[Shift+Alt+C]], also in Preferences › Appearance)
  narrows the strips. Right-click the master to put it at the left of the tracks, or back at
  their right.

![The mixer in its own window](img/mixer-window.webp)

## Faders and pan

- **Faders** follow a console law (0 dB at three quarters of the travel): drag them, [[Shift]]
  for fine moves, double-click for 0 dB.
- **Pan**: the knob under the output, its value under it. Drag up or right ([[Shift]] for fine
  steps; the centre holds the pointer), double-click to centre.
- **The mouse wheel** moves the fader (half a dB a notch, [[Shift]]: a tenth), the pan (1 % a
  notch) and the sends of the **selected** track only, so that passing over the mixer moves
  nothing.

## Selecting tracks

Click a name plate (or a track header in the timeline); [[Ctrl]] adds or removes, [[Shift]]
extends. Colour, duplicate and delete in the track menu act on every selected track, and so do
the output menu and the fader: moving one selected fader moves the others by as many dB. Drag a
name plate to move tracks. [[Shift+Alt+M]], [[Shift+Alt+S]] and [[Shift+Alt+R]] mute, solo and
arm the selected tracks.

## Inserts

- Click *+ insert* or an empty slot to open the **effect browser**; click an effect to open its
  window.
- In the browser, the left column picks a family, your favorites or the effects added lately;
  the search looks through everything, the buttons at the top keep one format or one vendor.
  A click chooses an effect and shows its presets below, a double-click or *Add* ([[Enter]])
  adds it. The arrows move between the cards, [[/]] goes to the search, [[F]] stars the chosen
  effect.
- [[Shift]]-click bypasses an effect, [[Ctrl]]-click removes it, right-click for the other
  options; *Sidechain* there chooses the track keying an effect that has a sidechain input.
- Drag an effect to reorder it, or onto another strip to copy it there ([[Alt]]: move it). Drop
  it on the list to choose the place, anywhere else on the strip to put it last.
- **Bypass** above the inserts bypasses every effect of the strip, and stays lit while they are
  bypassed; on the master, **ALL** bypasses every effect of the session (an instrument leading a
  track keeps playing).

**Effect windows** show, in their bottom bar, the track driving the detection for the effects
that have a **Sidechain**, the level going in and coming out (peaks and loudness) and the
**Output** gain after the effect, kept in the mix and the export. **Match** is a listening aid: while it is
lit, the output is brought to the input's loudness, so that bypassing the effect compares it at
equal level. It is never saved nor exported and turns off when the window closes; **Keep** moves
its correction into Output.

## The rack

The **Rack** tab ([[Alt+6]]) shows the chain of the selected track (or of a pinned channel) as
modules side by side, each with its main controls, A/B, and its editor a click away. Parallel
chains split the signal into branches of their own and mix them back.

![The rack](img/rack.webp)

## Outputs, buses and sends

- **Output**: the button above the pan says where the sound goes: **Master**, a bus, or *No
  output*. Its menu routes every selected track at once, and *New bus for n tracks* makes a bus
  for them, placed where the first of them was and named after them (*Drums*). Track › *Route
  the selection to a new bus* ([[Ctrl+Shift+B]]) does the same. The menu is also in the track's
  right-click menu and in the Inspector.
- **Buses** sum the tracks routed to them through their own inserts, sends and fader, then go to
  the master. They hold no clips. In the timeline the tracks of a bus show under it and fold
  with it; dragging a track among them routes it there. Track › Add › *Add bus* makes an empty
  one, for a reverb fed by sends.
- **Sends** go to any track or bus (pre or post fader); the menu of an empty send slot lists
  the buses first, and *New effect bus* makes one fed by the send. The send is added to every
  selected track; tick *Cut the output to the master* first for tracks heard only through the
  send. Each send is a button like an insert, with its level (and PRE before the fader): a click
  opens its settings window (destination, level, pre or post fader, mute), the wheel sets its
  level. Drag a send to reorder it, or onto another strip to copy it there ([[Alt]]: move it).
- **VCA**: the track's right-click menu chooses the VCA that controls it; a **V** under the fader
  shows it.

## Groups

Select tracks and press [[Ctrl+G]]. A group has nothing to do with buses: the tracks of a bus
stay each on its own unless they are grouped. The Groups menu sets what members share: level, pan and sends (relative), mute, solo, arm, inserts
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
- **Automation**: a VCA has a volume lane and an automation mode, like any track. Its curve is
  added to the level of every track it controls, whether they have their own automation or not.

## Console colour

![Console colour](img/console-color.webp)

The **Console** menu in the mixer's bar gives the whole mix the sound of an analogue console:
every track goes through the input stage of one of its channels, the buses and the master through
its summing.

| Console | Character |
|---|---|
| **Neutral** | no colour: the mix as it is |
| **British Warm** | transformers: round lows, a silky top (inspired by Neve consoles) |
| **British Bright** | tight and precise, a touch of air (inspired by SSL consoles) |
| **American Punch** | discrete amplifiers: punch and attack (inspired by API consoles) |
| **Nashville** | smooth and warm, very low distortion (inspired by Harrison consoles) |

Each console is a circuit of analogue stages (transformers, class A amplifiers, op-amps, VCAs),
calibrated on measurements of the plugins that model the console it is after. Like the real
ones, they colour subtly: at the working level a channel's harmonics sit 60 to 90 dB below the
signal, and grow as it gets louder. The transformers of British Warm thicken the lows most;
British Bright's channel adds odd harmonics, flat across the spectrum, and its mix bus is the
most coloured; American Punch adds even harmonics; Nashville's channel is nearly clean, its bus
saturates the lows.

*Amount* pushes the circuit: 50 % is the console at its working level, 0 % twelve dB
softer, 100 % twelve dB harder, always at the same output level, so only the colour changes.
*Hiss and crosstalk* adds each channel's noise floor (hiss, a little hum) and the sides of the
buses leaking into each other. Every track is a channel of its own: its parts differ slightly from
its neighbours', which widens the mix as a real console does. Each track can take another console
(*Track console*). The power button beside the menu switches every colour off at the same level,
to compare.

**Blind test › Start…** tells whether you hear the consoles rather than believe it: A is the mix
through its consoles, B without them, X one of the two drawn at random. Play the song, listen to
A, B and X as often as you like, then say which one X is. After ten rounds, the window tells how
often guessing would do as well: 9 right out of 10 happens about once in a hundred tests by
chance. The test changes nothing in the project; closing its window gives the consoles back.

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

- The **master** shows beside its meter the loudness of the mix since **Reset** (**LUFS**,
  integrated; streaming platforms bring songs to about -14) and its highest **PEAK** (in red over
  -1 dB, where it may clip once encoded).
- **M** at the top, then its automation mode, as on the tracks. **M**, **Dim** and **Mono**
  change only what you hear, never the export: M mutes the speakers, Dim lowers them by 20 dB
  (to talk, or answer the phone), Mono sums left and right to check the mix in mono. They are off
  again when the project opens.
- A click on the master's strip selects it: the rack and the Inspector show it.
- **MONITOR** holds effects you hear but that are not exported: room correction, a reference.
- **REF** beside it (also in the transport bar, [[Alt+R]]) compares the mix with a finished song
  (Transport › *Reference track…* to choose it). Click it to hear the reference instead of the
  mix, at the same place in the song and, by default, at the mix's loudness, so that you compare
  the sound and not the level. It is never exported.
