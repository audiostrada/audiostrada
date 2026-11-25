# Effects

AudioStrada comes with thirty-two built-in effects and instruments, the Strada series. Each one has
factory presets, user presets and A/B comparison at the top of its window. This chapter goes
through them family by family; the parameter tables give each control's range and default.

## Presets and chains

- **Presets**: the bar at the top of an effect's window shows the preset loaded, with a dot when
  its settings changed since. The arrows beside it, or [[Left]] and [[Right]], step through the
  presets; right-click an arrow › *MIDI learn* and press a button of your MIDI controller to
  step them from there, window open or not. Click its name for the preset panel: a search, the categories, your favorites, the
  factory presets and yours apart. The ear at the right of a preset lets you hear it (a
  second click stops), as browsing with [[Up]] and [[Down]] does; [[Enter]] keeps it, [[Escape]] goes back to the settings before.
  Right-click one of yours to rename or delete it. The star adds the preset to your favorites,
  the disk saves the current settings (type *Category: name* to file it). **A** / **B** hold two
  settings to compare, the button beside them copies one onto the other.
- **Factory presets** start with *Default*, the effect's own settings, then go by source or job
  (*Vocal*, *Drums*, *Bass*, *Bus*, *Master*, *Fix*...), three to six under each. They are set
  on recordings of what they name, to work at once on a track whose peaks sit around -6 dBFS:
  those that shape a sound give it back at the loudness it came in, and a compressor's
  threshold is the first thing to move for a quieter or louder track. Those under *Send* are
  fully wet, for an effect on a bus fed by sends; *Sidechain* and *Ducker* presets expect a
  key from another track.
- **Bypass**, at the right of the bar, is lit amber while the effect is bypassed.
- **Effect chains** keep a whole list of effects with their settings: right-click an effect (or
  use the track menu) › *Save effects as a chain* (its window lists the effects saved and warns
  when the name replaces a chain of yours), then add it to any track from *+ insert* › *Chains*.

Effect windows are windows of their own, which can go to another screen; clicking an insert
again brings its window to the front.

## Mastering and vocal chains

### Strada-Master

![Strada-Master](img/fx/mastering.webp)

A mastering chain behind six big knobs: **Tone** (darker or brighter: two shelves and an air
shelf in Strada-EQ's last bands),
**Glue** (Strada-MB-Comp's thresholds), **Color** (Strada-Tape), **Width** (Strada-Wide, the
bass kept in the center), **Sub** (more or less of the lowest octave: a low shelf at 70 Hz, the last band of Strada-EQ, Strada-Wide keeping it in the center)
and **Loudness** (Strada-Max's drive, Strada-Limit keeping the true peaks under the ceiling).
Above them, a **style** (*Clean*, *Warm*, *Open*, *Punchy*) sets the stages a starting point,
and a **target** sets the limiter's ceiling and the loudness aimed at (choosing one moves the
Loudness knob by the gap between the targets, then trims it by ear over a few seconds of music): *Streaming* -14,
*Apple Music, podcast* -16, *CD* -9 (peaks up to -0.3 dB), *Club* -7, *Vinyl* -12 (lows in
mono, room under the peaks), *TV, radio* -23 LUFS (EBU R128), and *Pre-master*, for a
mastering engineer: nothing maximized, peaks held at -6 dBFS. **Follow** reaches the target,
then keeps it whenever a setting changes (a knob, the style, a stage's settings): each change is
measured over a few seconds, the output against the input, and made up for. The music getting
louder or softer never moves it, so quiet and loud sections keep their difference. **Before, same
loudness** plays the mix without Strada-Master, raised to its loudness, to hear what the
processing does without the loudness speaking for it; it stops when the window closes.

The display draws the song's spectrum in the style's tunnel, its loudness over the last seconds
against the target, the stereo image, the true peak and the maximizer's gain reduction.

The bar at the top is the chain: its first tab, *Strada-Master*, shows the knobs; the others
open each stage's own editor, its presets included. Drag a stage to change the order the sound
goes through them; Strada-Limit stays last. Each knob acts around the settings of the stage
under it, without changing them; a stage's editor shows its settings as they are played, the
knobs applied, and what you change there becomes the base the knobs act around. **Listen and set**, offered when the effect is first opened,
lets the assistant hear eight seconds of the loudest passage: it sets Tone, Glue, Width, Sub and
Loudness for the mix and the target, then trims the loudness once the output has settled. The
slider between *Your settings* and *Assistant* blends its proposal with what you had; *Undo*
puts your settings back.

<!-- effect:mastering -->

### Strada-Vocals

![Strada-Vocals](img/fx/vocals.webp)

A vocal chain behind six big knobs: **Tune** (Strada-Correct: more is faster and firmer),
**Tone** (warmer or brighter, with air on top) with its small **Saturation** knob (Strada-Drive,
from a touch of warmth to grit), **Comp** (Strada-Comp's threshold), **De-ess** (Strada-DeEss),
**Double** (Strada-Doubler) and **Space** (Strada-Delay and Strada-Verb, ducked under the
voice). The **voice** (*Singing*, *Rap*, *Spoken*) and the **space** (*Room*, *Plate*, *Hall*,
*Slapback*, *Echo*) set the stages a starting point. The display draws the voice's spectrum
against its target, the pitch sung over the last seconds, the note, the compression and the
de-essing. **Take**, while recording, leaves out the tuning and the space, and with them their
latency.

As in Strada-Master, the chain's bar opens and reorders the stages, and **Listen and set** sets
Tone, Comp and De-ess for the voice heard, the compressor's and de-esser's thresholds at its
level, and Strada-Correct's key from the notes sung.

<!-- effect:vocals -->

## Dynamics

### Strada-Comp

![Strada-Comp](img/fx/compressor.webp)

A compressor with four characters: **Clean** for transparent control, **Bus** for glue, **Opto**
for smooth levelling and **FET** for punch. Threshold, ratio, attack and release as usual;
*Auto release* follows the material, *Color* adds the character's saturation, *Mix* blends the
dry signal back (parallel compression), the sidechain filter keeps the lows from pumping.

<!-- effect:compressor -->

### Strada-Gate

![Strada-Gate](img/fx/gate.webp)

Gate, expander and ducker, with hysteresis (it opens and closes at different levels, so it does
not chatter), lookahead, hold and range. The sidechain filters and *Listen* help it hear only
the drum you want.

<!-- effect:gate -->

### Strada-Transient

![Strada-Transient](img/fx/transient.webp)

More or less punch and sustain on drums and plucks, without a threshold: *Attack* raises or
softens the hits, *Sustain* the ring after them.

<!-- effect:transient -->

### Strada-DeEss

![Strada-DeEss](img/fx/deesser.webp)

Tames harsh S and T sounds on vocals. Set the frequency with *Listen* on, then the threshold;
the mode works on the highs only or on the whole voice.

<!-- effect:deesser -->

### Strada-MB-Comp

![Strada-MB-Comp](img/fx/mb_comp.webp)

A multiband compressor: up to four bands, split at the crossovers, each compressed from above
and lifted from below (*lift*). *Solo* plays one band alone.

<!-- effect:mb_comp -->

### Strada-Limit and Strada-Max

![Strada-Max](img/fx/maximizer.webp)

Both look like a mastering maximizer: the input meter on the left with the **threshold** (drag it
down to push the input harder), the output meter on the right with the **ceiling**, and between
them the music scrolling by, red where the limiter takes peaks away. Below: gain reduction,
output peak, short-term loudness, and how much louder the output is.

**Learn** listens while you play the loudest part and moves the threshold until the output
reaches the chosen loudness (-14 LUFS for streaming, for instance). Strada-Max is the mastering
limiter, with a true-peak ceiling and look-ahead; Strada-Limit is a brickwall peak limiter for
tracks and buses. **Before, same loudness** plays the input without the limiter, raised to
its output's loudness, to judge the limiting itself; it stops when the window closes.

<!-- effect:maximizer -->

<!-- effect:limiter -->

### Strada-Trigger

![Strada-Trigger](img/fx/trigger.webp)

Reinforces or replaces a kick or a snare with sampled drums, hit for hit. The track's hits are
drawn live under a threshold to drag. Choose the drum and its sound (studio, garage, deep,
boom, vintage, rock… or your own sample), then two faders: close microphones to room, and the
track to the trigger.

<!-- effect:trigger -->

### Strada-Channel

A console channel in one insert: filters, gate, compressor, four-band EQ and drive, in an order
you choose (see *Mixing*). **Auto** beside the output keeps the output as loud as the input,
whatever the modules add or take, to compare by ear without the level fooling you; the
*Output* knob trims it.

<!-- effect:channel -->

## EQ and filter

### Strada-EQ

![Strada-EQ](img/fx/eq.webp)

Up to 16 bands on the curve. Double-click the curve to add a band (a cut near the ends, a bell
elsewhere), drag a band to move it, [[Ctrl]]-drag it (up narrower, down wider) or use the wheel
over it to set its Q, double-click it to remove it.

Right-click a band for its shape, drawn (bell, low and high shelf, low and high cut, notch, band
pass, tilt), the slope of a cut (6 to 48 dB per octave), its placement (both channels, left,
right, mid or sides), *Dynamic*, *Solo* and *Bypass*, *Gain to 0 dB* and *Flip the gain*. Under
the curve, the selected band has the same drawn shapes, its slope and placement, then its
frequency, gain and Q.

- A **dynamic** band listens to its own frequencies at the input (a bell around its frequency,
  a low shelf below it, a high shelf above it) and moves its gain while they are above the
  *Threshold*: by as many dB as they pass it, up to the *Range*, at the speed set by *Attack*
  and *Release*. A negative range cuts when loud (a resonance that only rings now and then,
  sibilance, a boomy note); a positive range boosts when loud (the crack of a snare on its hits
  only). To boost while quiet, give the band a gain and a negative range of the same size: it
  boosts at rest and comes back to flat when its frequencies get loud. The panel says what the
  band does and the gain it has now; on the curve, its range shows as a line under the band,
  the gain it has now as a ring.
- **Solo** plays only the frequencies the band works on, to find what to cut; it stops when the
  window closes.

<!-- effect:eq -->

### Strada-Tube EQ

![Strada-Tube EQ](img/fx/vintage_eq.webp)

A passive tube program EQ: the famous low boost and cut at the same frequency (the lows grow
while the low mids clear), silky highs and tube warmth.

<!-- effect:vintage_eq -->

### Strada-Filter

![Strada-Filter](img/fx/filter.webp)

A resonant sweeping filter with drive, an LFO (synced to the tempo or free) and an envelope
follower: from a subtle movement to a wild sweep.

<!-- effect:filter -->

## Space

### Strada-Verb

![Strada-Verb](img/fx/reverb.webp)

Pick a space: Room, Hall, Space, Plate, Spring or Shimmer (hover a button for what it sounds
like). Shimmer is a vast space whose tail rises an octave as it rings; its Shimmer knob sets how
much.

Then a color, the character of an era: **Clean** leaves the tail full and smooth; **80s**, the
digital reverbs of the 1980s, narrows it to 11 kHz with the grain of 12 bits and a lush chorus;
**Vintage**, the first digital reverbs and the plates of the 1970s, makes it darker (7 kHz),
grainier and thinner, with a soft hiss that follows the tail.

The display shows how long each frequency rings, in seconds, over the spectrum of the tail (in
the color's tint) and of the input (a line); the cuts shade its ends, and dashed lines mark a beat
and a bar at the project's tempo. Drag its points: the green one for the decay, the orange one for
how long the lows ring and up to where, the blue one for the highs, the red and violet ones for
the low and high cuts. Double-click a point to reset it. *Fit to the tempo* sets the decay to
whole quarters of a bar and the pre-delay to a note value.

The main controls are the large ones: *Decay*, with *Freeze* (the tail held forever, for pads),
*Pre-delay*, in milliseconds or, with *Sync*, as a note value, and *Mix*. Beside them come the
reflections (Size, Early / late, Diffusion), the motion (the modulation's depth and rate), the
ducking (how much the tail is lowered while the input plays, and how fast it comes back), the
tone (the knobs of the display's points), the width and the Shimmer amount. Those a space does
not use are greyed.

<!-- effect:reverb -->

### Strada-Delay

![Strada-Delay](img/fx/delay.webp)

Pick a machine: **Digital** gives clean repeats, and a new time crossfades in; **Tape**
wobbling, saturated repeats that darken, and a new time glides like a varispeed; **Analog**
(bucket brigade) warm repeats, darker the longer the time.

On the left, the echoes on the tempo grid, left above and right below, light up as they are
heard; ping-pong draws their path. Drag the first echo across for the time and up or down for the
feedback, and the first one on the right across for its offset. On the right, the tone of the
repeats over their spectrum: the yellow curve is one pass through the loop, the fainter ones the
passes after it, darker each time; drag the red and violet points for the low and high cuts.

The main controls are the large ones: *Time*, in milliseconds with *Tap*, or with *Sync* as a
note value at the project's tempo; *Feedback*, with *Ping-pong* and *Freeze* (what is in the delay
repeats forever, the input left out); and *Mix*. Beside them come the cuts, wow and flutter and
drive (tape and analog only), the right side's offset, and ducking (the echoes lowered while the
input plays, back in the gaps).

<!-- effect:delay -->

## Modulation

### Strada-Chorus

![Strada-Chorus](img/fx/chorus.webp)

Short modulated delays for width and shimmer, with one voice, two, or an ensemble; with
feedback it turns into a flanger.

<!-- effect:chorus -->

### Strada-Flanger

The classic jet flanger, with positive or negative feedback, stereo sweep and *Through zero*,
the tape flanging of the sixties. At 50 % *Mix*, half dry and half delayed: the deepest
notches.

<!-- effect:flanger -->

### Strada-Phaser

A swirling phaser of 4 to 12 stages, with feedback and stereo spread. At 50 % *Mix*, half dry
and half phased: the deepest notches.

<!-- effect:phaser -->

### Strada-Rotary

![Strada-Rotary](img/fx/rotary.webp)

The rotary speaker of the organs, for any track: a horn for the highs and a drum for the lows,
each turning slow or fast and taking its time to change speed (the horn in a moment, the drum
in seconds), heard by two microphones. The screen shows them from above, turning as they turn.
*Stop* lets them slow down to a halt; *Inertia* makes the change quicker or slower; *Drive*
pushes its tube amplifier; *Horn and drum* sets their balance.

<!-- effect:rotary -->

### Strada-Mod

![Strada-Mod](img/fx/modulator.webp)

Tremolo, auto-pan, rhythmic cutter (a step sequencer of the level), vibrato and ring modulation,
synced to the tempo.

<!-- effect:modulator -->

### Strada-Pitch

![Strada-Pitch](img/fx/pitch.webp)

A pitch shifter with two voices, delay and feedback: harmonies, octaves, widening, and
shimmering echoes.

<!-- effect:pitch -->

## Colour

### Strada-Drive

![Strada-Drive](img/fx/drive.webp)

Pick a type (Overdrive, Distortion, Fuzz, Tube, Tape, Transformer, Rectifier). *Drive* goes from
clean to exaggerated by even steps: the first third adds a little color to a bass or a bus, the
top a fuzz. Peaks keep their level whatever the drive; *Auto gain* matches the loudness instead,
so that you judge the color and not the volume. *Output* trims the level, *Mix* blends the dry
sound back in.

The display shows the spectrum of the input (a line) and of the output, where the added
harmonics glow. The orange line is how hard each frequency is driven: drag its points for the
drive, the *Low cut* that keeps the lows out of the saturation, and the *Focus*, a band driven
harder (to bring a bass forward) or softer than the rest, its tone unchanged. The white line is
the tone after the drive: drag its points for the *Bass*, the *Middle* (and its frequency) and
the *Tone*, a high cut. Double-click a point to reset it. *Bias* makes the clipping asymmetric
(warmer, even harmonics).

<!-- effect:drive -->

### Strada-Tape

![Strada-Tape](img/fx/tape.webp)

A tape machine, drawn with its reels turning at the chosen speed and its VU meters. Four
machines: **Studio A80** (2-inch studio deck: punchy lows, firm mids), **Mastering 102**
(half-inch mastering deck: wide, clean and smooth), **Vintage J37** (four-track: darker,
rounder, saturates early), **Cassette** (soft top, warble and hiss). Three tapes: **GP9**
(modern, most headroom), **456** (the classic, compresses when pushed), **250** (vintage,
saturates early). Speed, head bump, wow and flutter, hiss and bias complete it.

<!-- effect:tape -->

## Stereo

### Strada-Wide

![Strada-Wide](img/fx/imager.webp)

Width per band, mono lows and a goniometer: wide mixes that stay solid in mono. *Stereoize*
makes a stereo image from a mono source.

<!-- effect:imager -->

### Strada-Doubler

![Strada-Doubler](img/fx/doubler.webp)

Double tracking: detuned, delayed and drifting copies spread across the stereo field, for
vocals and guitars recorded once.

<!-- effect:doubler -->

## Utility

### Strada-Utility

![Strada-Utility](img/fx/utility.webp)

Gain, balance, width, mono and mono below a frequency, polarity per channel, channel swap, time
alignment in milliseconds and phase rotation.

<!-- effect:utility -->

### Strada-Correct

Automatic pitch correction for the voice, from a light touch to the stepped, synthetic voice.
The display scrolls the pitch sung (grey) and the corrected one (blue) over the notes of the
scale; at its right, the note sung and how far off it is, the note aimed at, and a bar showing
by how many cents the voice is being moved.

- **Natural** follows the settings: *Retune speed* is the time the voice takes to reach its
  note (0 ms is immediate, 20 to 60 ms sounds like a singer in tune), *Humanize* corrects the
  center of a held note and leaves its vibrato, *Tolerance* leaves alone a note already that
  close, *Amount* corrects only a part of the way.
- **Staircase** puts every note flat on its target at once, whatever those settings: the
  voice moves in steps, vibrato and slides gone, as on the first automatically corrected
  records. Choose a scale with few notes for wider steps.
- **Harmony** corrects the voice as *Natural* does and adds a second voice singing along in
  the key: a third, fifth, sixth or octave above, a third, fourth or octave below, counted in
  the notes of the scale (of the major scale of the key when the scale is chromatic). *Level*
  sets the second voice against the first; the pair keeps the loudness of one voice.

Choose the key and the scale (or click notes on the keyboard for a *Custom* one). While the
voice plays, the effect listens for the key of what is sung and shows it under the keyboard
(*Heard: A minor*); **Use this key** sets the key and the scale to it. *Transpose* moves the
whole voice by semitones, formants kept.

<!-- effect:pitch_correct -->

### Strada-Scope

![Strada-Scope](img/fx/analyzer.webp)

A spectrum analyzer, with peak hold, tilt and the choice of what it shows.

<!-- effect:analyzer -->

### Strada-Tuner

A chromatic tuner for guitar, bass and voice; it can mute the track while tuning.

<!-- effect:tuner -->

## Amp and cab

### Strada-Amp

![Strada-Amp](img/fx/amp_rig.webp)

A whole guitar or bass rig: up to eight pedals, an amp and a cabinet. The window has three
sections: Pedalboard, Amp and Cab.

- **Pedals**: click **+** to add one; over a pedal, the pencil changes it and the cross removes
  it; drag a pedal to move it. Click its footswitch to turn it on or off. 21 pedals after the
  classics: Tube Screamer, Klon, Blues Driver, SD-1, DS-1, RAT, Metal Zone, Fuzz Face, Big Muff,
  Rangemaster, Cry Baby wah, compressor, modulation, analog and tape delay, reverb.
- **Amps**: the arrows beside the amp step through them; click its name for the full list. 16
  **built-in** models after the most played amps (Twin, Super and Deluxe Reverb, Tweed Deluxe,
  Bassman, AC30, JTM45, Plexi, JCM800, Friedman, Dumble, Dual Rectifier, 5150, Diezel, SVT, B-15),
  each with its own preamp, tone stack and power stage, and a matching cabinet.
- **My library** lists your Neural Amp Modeler captures by category (Amps, Amps + cab, Pedals,
  Other, Favorites). *Add captures…* copies `.nam` files into AudioStrada's library, sorted by
  gear; right-click a card to rename it, change its category, star it or remove it. A folder of
  captures of one amp is one amp: its Gain knob steps between them. A capture freezes the amp at
  one setting: Bass, Middle, Treble, Presence and Master shape the sound after it.
- **TONE3000**: *Browse TONE3000…* opens its free captures by kind (amps, effects, cabs):
  trending, latest, your favorites and downloads. A click downloads a tone and loads it where it
  belongs. Signing in (free account) shows your favorites.

<!-- effect:amp_rig -->
