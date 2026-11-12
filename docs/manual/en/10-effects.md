# Effects

AudioStrada comes with thirty built-in effects and instruments, the Strada series. Each one has
factory presets, user presets and A/B comparison at the top of its window. This chapter goes
through them family by family; the parameter tables give each control's range and default.

## Presets and chains

- **Presets**: the bar at the top of an effect's window shows the preset loaded, with a dot when
  its settings changed since. The arrows beside it, or [[Left]] and [[Right]], step through the
  presets; right-click an arrow › *MIDI learn* and press a button of your MIDI controller to
  step them from there, window open or not. Click its name for the preset panel: a search, the categories, your favorites, the
  factory presets and yours apart. Browsing it with the pointer or [[Up]] and [[Down]] lets you
  hear each preset; [[Enter]] keeps it, [[Escape]] goes back to the settings before.
  Right-click one of yours to rename or delete it. The star adds the preset to your favorites,
  the disk saves the current settings (type *Category: name* to file it). **A** / **B** hold two
  settings to compare, the button beside them copies one onto the other.
- **Factory presets** start with *Default*, the effect's own settings, then go by source or job
  (*Vocal*, *Kick*, *Drum bus*, *Master*, *Fix*...). Those under *Send* are fully wet, for an
  effect on a bus fed by sends; *Sidechain* presets expect a key from another track.
- **Bypass**, at the right of the bar, is lit amber while the effect is bypassed.
- **Effect chains** keep a whole list of effects with their settings: right-click an effect (or
  use the track menu) › *Save effects as a chain*, then add it to any track from *+ insert* ›
  *Chains*. Factory chains: Vocal, Drum bus, Acoustic guitar, Master for streaming.

Effect windows are windows of their own, which can go to another screen; clicking an insert
again brings its window to the front.

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
tracks and buses.

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

Right-click a band for its type (bell, low and high shelf, low and high cut from 6 to 48 dB per
octave, notch, band pass, tilt), *Dynamic band*, *Solo* and *Bypass*. Under the curve: the
selected band's type, slope, frequency, gain and Q.

- A **dynamic** band moves its gain by up to its *Range* while its frequencies are above the
  *Threshold*: a negative range tames a resonance only when it rings. Its range shows as a line
  under the band, the gain it has now as a ring.
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

*Decay* is in seconds; *Pre-delay* holds the tail back from the dry sound; *High cut* and *Low
cut* trim it; *Width* spreads it; *Ducking* lowers it while the input plays (the tail comes back
in the gaps); *Mix* sets how much is heard. *Freeze* holds the tail forever, for pads.

The smaller knobs set Size, Early / late, Treble decay, Bass decay, Diffusion and Modulation;
those a space does not use are greyed. The display shows how the lows and highs fade.

<!-- effect:reverb -->

### Strada-Delay

![Strada-Delay](img/fx/delay.webp)

Digital, tape or analog echoes. **Digital** gives clean repeats; **Tape** wobbling, saturated
repeats that darken, and a new time glides like a varispeed; **Analog** (bucket brigade) darker
repeats the longer the time. Sync to the tempo, ping-pong, a right-channel offset, ducking and
freeze.

<!-- effect:delay -->

## Modulation

### Strada-Chorus

![Strada-Chorus](img/fx/chorus.webp)

Short modulated delays for width and shimmer, with one voice, two, or an ensemble; with
feedback it turns into a flanger.

<!-- effect:chorus -->

### Strada-Flanger

The classic jet flanger, with positive or negative feedback, stereo sweep and *Through zero*,
the tape flanging of the sixties.

<!-- effect:flanger -->

### Strada-Phaser

A swirling phaser of 4 to 12 stages, with feedback and stereo spread.

<!-- effect:phaser -->

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

Pick a type (Overdrive, Distortion, Fuzz, Tube, Tape, Transformer, Rectifier); the curve shows
how it bends the signal. Drive, Tone, Level and Mix are the pedal's knobs; *Low cut* tightens
the lows before the drive, *Bias* makes the clipping asymmetric (warmer, even harmonics), *Bass*
and *Middle* shape the sound after it.

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

Automatic pitch correction for the voice, from natural to the robot effect. Choose the key and
the scale (or *Custom* notes); *Retune speed* at 0 ms gives the hard effect, higher values a
natural correction, *Humanize* leaves the held notes some life.

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
