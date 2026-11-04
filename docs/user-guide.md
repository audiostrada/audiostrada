# AudioStrada user guide

AudioStrada records, edits and mixes music. This guide walks through the main tasks; every command
is also in the menus and in the command palette (Ctrl+Shift+P, Cmd on macOS), which shows its
shortcut.

## 1. Install and start

Download the package for your system from the releases page: `.deb`, `.rpm` or `.AppImage` on
Linux, the `-setup.exe` installer on Windows, the `.dmg` on macOS (see
[distribution.md](distribution.md)). Open a project by double-clicking a `.strada` file, or
start AudioStrada from the menu.

### Audio setup

On Linux, AudioStrada plays through PipeWire with no setup; JACK and ALSA are also available.
**Preferences › Audio** (Ctrl+,) chooses the backend, the device, the sample rate and the
buffer size, shows the resulting latency, plays a test tone and shows input levels. If the
sound crackles, raise the buffer size. From a terminal, `audiostrada diagnose` checks the system
for low-latency audio (real-time priority, CPU governor, PipeWire quantum) and prints the
exact fix for each problem; `audiostrada devices` lists the devices.

On Windows choose WASAPI (shared or exclusive) or ASIO; on macOS, CoreAudio.

## 2. The window

- **Transport bar**: go to start, play/pause (Space), stop, record, loop, punch, metronome;
  the counter shows bars.beats.ticks with the clock time below; tempo and time signature
  (MIDI clips keep their bars and beats when the tempo changes, audio clips their time);
  edit mode (Slip, Shuffle, Grid), grid value and snap.
  Effects ring out after a stop (reverb and delay tails fade naturally); stop again while
  stopped to silence them.
- **Timeline**: tracks and their clips, the ruler with markers, loop and punch ranges.
- **Mixer** and **Piano roll** below, in tabs.
- AudioStrada reopens its window, and the panels in their own windows, at the size and in the
  state (maximized or not) they were left; on Wayland the system decides where windows go.
- **View** shows or hides the Browser (audio files), the Inspector (clip and track details)
  and the Markers list. Drag a tab to rearrange the panels; right-click a tab and choose
  *Open in its own window* to put a panel on another screen (the mixer, typically). Closing
  that window puts the panel back.
- **Right-click** almost anything for its options: clips, empty parts of a track, the ruler,
  markers, track headers, mixer strips, inserts, faders and knobs.

## 3. Projects

File › New, Open, Save (Ctrl+S) and Save As. A project is a folder holding the `.strada`
file, an `Audio` folder (recordings and imported files are copied there), a `Models` folder
(amp models and impulse responses) and a cache of waveform peaks. AudioStrada autosaves; after a
crash it offers to recover the unsaved work. Everything can be undone (Ctrl+Z) and redone
(Ctrl+Shift+Z).

Each save keeps the file it replaces in the project's `Versions` folder (the last 30): File ›
*Project versions…* lists them and restores one in a single step that Ctrl+Z takes back.
File › *Collect media into the project folder* copies the files the project uses from
elsewhere (audio, amp captures, impulses) into it, so the folder holds everything. File ›
*Archive project…* packs the project with all of them into one `.tar.gz` file, to send or
keep, without changing the project.

Import audio with File › Import (Ctrl+I) or by dropping files on the timeline: each file gets
its own track, named after it, which is the quickest way to bring in a set of stems. A stereo
file makes a stereo track. AudioStrada
reads WAV, AIFF, CAF, FLAC, ALAC, MP3, MP2, AAC (`.m4a`, `.aac`), Ogg Vorbis and the audio of
MP4, MOV, MKV and WebM files.

## 4. Recording

1. Choose the track's input in its header (*In 1*, *In 1-2* for stereo).
2. Arm the track with **R**: its input is heard through the track (monitoring).
3. Press record (R in the AudioStrada shortcut preset). Several tracks record together.

- **Monitoring**: the speaker button beside **R** says when a track hears its input. On *Auto*
  it does while armed, as set in Preferences (always, or only while stopped or recording);
  *Input* always hears it, armed or not (to play through an amp); *Off* never does (direct
  monitoring on the interface). It is lit while the input is heard. An armed track's meter shows
  its input level, heard or not.
- **Low latency mode** (stopwatch in the transport bar): effects adding more than 1 ms are
  bypassed, except on the tracks being played, so that their latency no longer delays what you
  hear. The mixer shows them struck through.
- **Metronome**: the metronome button (K) clicks on every beat while playing; right-click it
  for its window (volume, sound, accents, count-in of up to four bars). The click is never in exports.
- **Punch**: Shift-drag in the ruler sets the punch range (or right-click the ruler); recording
  only happens inside it.
- **Loop recording**: with the loop on, each pass becomes a take. Takes are kept in
  playlists; the Inspector compiles the best parts (comping).
- **MIDI**: arm an instrument track and play a MIDI keyboard (enabled in Preferences › MIDI);
  recording creates a MIDI clip.

## 5. Editing

The pointer is a Smart Tool on clips:

| Where | Drag does |
|---|---|
| Body | Move (Alt: copy, onto another track too) |
| Left or right edge | Trim |
| Right edge with Alt | Time stretch, pitch kept |
| Top corners | Fade in / fade out |
| Body with Ctrl | Clip gain |

Click a clip to select it, Ctrl+click to add or remove one, Shift+click to select every clip
from the last clicked one to this one (across the tracks between them). Trims, fades and gain
drawn on a selected clip apply to all the selected clips, by the same amount.

**Modes**: Slip moves freely, Shuffle closes gaps when deleting or trimming, Grid snaps to the
grid (G toggles snap in the other modes). Overlapping clips crossfade automatically.

**Clip menu** (right-click): rename, split here (S splits at the playhead), duplicate
(Ctrl+D), delete, fade lengths and shapes (equal power, linear, fast, slow, S-curve), gain
steps and normalize, mute, loop these clips, stretch (50 % to 200 %, back to the original
speed, or *Match project tempo*, which detects the clip's tempo) and pitch (semitone and
octave steps). The Inspector sets exact values, including the stretch percentage and the pitch
to the cent.

Double-click a track's name in its header to rename it (Enter keeps it, Escape cancels); a
name too long for the header ends with "…" and shows whole when hovered.

**Track header menu**: rename, color, *Stereo track*, duplicate, delete, move, freeze, bounce to
a new track, export the track alone, new playlist, automation. Track › Add stereo track makes a
track recording two inputs (In 1-2). Stereo clips show the left channel above the right one.
With two tracks selected, *Convert to a stereo track* makes one of them (an "Overhead L" and an
"Overhead R" become "Overhead"): the names tell left from right, else the upper track is the
left one; the left track keeps its settings, and Ctrl+Z brings both back.

**Removing a selection**: Escape, or right-click the ruler › *Remove the selection*, clears the
time range and the selected clips.

**Processing clips** (Edit menu, or right-click clips or a range):

- **Strip silence** (Ctrl+U): a window sets the threshold, the shortest silence and what is
  kept before and after the sound; the timeline darkens what will go. Keep the sound, the
  silences, or only cut at the silences.
- **Heal** (Ctrl+H) joins clips cut from the same audio back into one when they still sit in
  place.
- **Consolidate** (Ctrl+Shift+C) renders the selected clips (or the range) of each track into
  a single new file and clip, with gain, fades, stretch and pitch applied.
- **Process clips…** renders the clips through a built-in effect, reverses them or normalizes
  them, as new files (the originals stay in the project, Ctrl+Z goes back).
- **Beat Detective…** (with a range selected over drum tracks) finds the hits, shown as red
  lines, on all the selected tracks together so the microphones of a kit are cut at the same
  places. *Separate at the hits* cuts the clips; *Separate and conform to the grid* also moves
  each slice toward the grid by the chosen strength, with short crossfades. One undo step.

**Markers**: M adds a marker at the playhead, or right-click the ruler or an empty part of a
track › *Add marker here*; . and , jump between markers.

## 6. MIDI and instruments

Track › Add instrument track creates a track with the built-in Synth; any CLAP, VST3 or LV2
instrument can replace it: right-click the Synth insert › *Change instrument…*, or *+ insert* ›
*Instrument*. An instrument always goes first on its track, in place of the one there. Double-click an empty part of an instrument
track (or Track › New MIDI clip) to create a clip, then edit it in the **Piano roll**.

- **Tools** (keys 1 to 5): the *pointer* selects, moves (Alt-drag copies) and resizes from
  either edge, drags a rubber band in the empty space and adds a note with a double-click; the
  *pencil* adds a note and drags its length; the *brush* paints a note per grid step; the
  *scissors* cut notes where they are clicked; the *eraser* removes what it passes over.
- **Grid**: the piano roll has its own (down to 1/32, triplets too); hold Ctrl to place freely.
- **Navigation**: the wheel scrolls through the keys (Shift: in time), Ctrl+wheel zooms time
  and Alt+wheel the keys, around the pointer; the middle button drags the view; the frame
  button fits the notes. The view follows the playhead while playing; click the ruler to move it.
- **Scale**: shades the keys outside it, can *fold* the keyboard to the scale's keys and those
  in use, and *keep the notes in it* when they are drawn, moved or transposed. *Detect from the
  notes* finds it.
- **Chords**: the chord button makes the pencil and the brush add a whole chord: the one the
  scale builds on the key (triad or seventh), or a chosen kind, inverted and voiced (close,
  open, drop 2, spread). The strip above the ruler names the chords the notes make.
- **Ghost notes**: the other MIDI clips show faintly; double-click one to edit its clip.
- **Transform** (also on the right-click menu): quantize (strength, swing, ends), humanize,
  strum, arpeggiate, harmonize, chords from notes, variation, randomize, velocity shape and
  stretch open a panel that previews the result on the notes; Enter applies, Escape cancels.
  Legato, reverse, mirror, glue and mute apply at once.
- **The lane** under the notes edits velocity or the **probability** of each note (a note
  under 100 % plays only some of the times), as bars: drag one (the selected ones follow),
  drag across to paint, Shift-drag for a straight ramp, right-click for shapes (crescendo,
  compress, accents). It also draws controllers, and with one note selected its own pitch,
  pressure or timbre (MPE), drawn inside the note; *Own channels* gives each note its channel.
- With the piano roll focused: Ctrl+C, Ctrl+X, Ctrl+V (at the playhead), Ctrl+D (duplicates
  right after), Left/Right nudge by the grid, Up/Down transpose (Shift: an octave), Q quantizes.
  Notes sound while you click the keys on the left (double-click selects a key's notes), add a
  note or drag one; the keys light up as notes play.

A clip grows to hold notes pasted past its end. File › Import MIDI file creates a
track per part; File › Export MIDI file writes the selected track or all of them.

The Synth's presets hold ready-made sounds (soft keys, pads, strings, brass, pluck, bell,
lead, basses). Its window fits without scrolling: Tone, Brightness and Playing side by side,
Shape below (drag the envelope's corners), then the keyboard. *MPE controller* is for MPE
controllers (Roli Seaboard, LinnStrument, Osmose), which send each note on its own channel. Its keyboard plays the synth without arming the track: click or drag across the
keys (lower on a key plays louder), or turn on *Computer keyboard* to play with the letters
(A S D F G H J K the white keys, W E T Y U the black ones, Z and X the octave, by position on
the keyboard, so AZERTY works the same).

Track › Add drum machine track creates a track with the built-in **Drum Machine**, in the
spirit of the 808: twelve synthesized voices (kick, rim shot, snare, clap, closed and open hats,
three toms, cymbal, cowbell, clave) on the General MIDI drum notes, so the piano roll and any pad
controller play them. A pad lights up while it sounds and plays on a click (lower on the pad
plays louder); clicking it also shows its level, tune, decay and tone below. The closed hat
chokes the open one. Accent sets how much velocity changes the level, Drive saturates the whole
kit. Its presets hold kits (classic, boom, trap, electro, tight and dry, dusty lo-fi, tuned up).
**Rhythms** writes a ready-made pattern (boom bap, lo-fi, trap, drill, house, techno, electro,
dembow, funk, rock, drum and bass; each shows its usual tempo) over 1 to 8 bars into a new MIDI
clip at the playhead's bar and opens it in the piano roll; the project tempo is left alone.

An instrument track armed for MIDI never hears the audio inputs. A keyboard's knobs turn the
built-in Synth of the armed tracks with the standard controllers: CC 74 cutoff, 71 resonance,
73 attack, 75 decay, 72 release, 7 volume (CC 74 shapes each note instead in MPE mode).

**MIDI learn**: right-click a fader, pan knob, send level or effect parameter › *MIDI learn*,
then move a control on your MIDI controller. A learned controller does only what it was
learned for.

## 7. Mixing

Each track has a strip: the output (and VCA), the pan, the fader and meter, mute (M), solo
(S), record arm (R), the automation mode and the track's name. The fader takes the height the
panel leaves.

- **Docked** (in the main window), the strip leaves its height to the fader: the **FX** and
  **Send** buttons beside the meter (lit, with their count, when the list holds something)
  open the strip's effects and sends in a window of their own; the master has **FX** and
  **Mon** (monitoring effects).
- **In its own window** (View › *Mixer in its own window*, Ctrl+M, to put it on another
  screen), each strip shows its effect and send slots at the top: click an empty slot to add
  an effect or a send. *Show mixer* (Alt+1) brings the window forward; Ctrl+M again, unticking
  the menu item or closing the window docks it back. Ctrl+P does the same for the piano roll.
- **Pan**: the bar under the output; drag along it (Shift for fine steps, the center holds
  the pointer), use the mouse wheel for 1 % steps, double-click to center.

- **Selecting**: click a name plate (or a track header in the timeline); Ctrl adds or removes,
  Shift extends. Color, duplicate and delete in the track menu act on every selected track,
  and so do the output menu (folder, master, VCA; tracks keep their order inside the folder) and
  the fader: moving one selected fader
  moves the others by as many dB. Drag a name plate or a header to move tracks.
- **Faders** follow a console law (0 dB at three quarters of the travel); drag, Shift for fine
  moves, the mouse wheel, double-click for 0 dB.
- **Effects**: click *+ insert* or an empty slot to open the effect browser; click an effect to open its window;
  Shift+click bypasses it, Ctrl+click removes it, right-click for the other options. Drag an
  effect to reorder it, onto another strip to copy it there (with Alt, to move it): onto its
  list to choose the place, anywhere else on the strip to put it last. **FX**
  beside *+ insert* bypasses every effect of the strip; on the master, **ALL** bypasses every
  effect of the session (an instrument leading a track keeps playing).
- **Effect windows** show the level going in and coming out (peaks and loudness) and the
  **Output** gain after the effect, kept in the mix and the export. **Match** is a listening
  aid: while it is lit, the output is brought to the input's loudness, so that bypassing the
  effect compares it at equal level; it is never saved nor exported and turns off when the
  window closes. **Keep** moves its correction into Output.
- **Sends** go to any track, which then acts as a bus (pre or post fader). An empty send slot adds
  the send to every selected track; tick *Cut the output to the master* first for tracks heard
  only through the bus.
- **Folders**: a track can hold others; its strip is their bus.
- **Groups**: select tracks and press Ctrl+G, or tick *Folder acts as a group* in a folder's
  menu. The Groups menu sets what members share: level, pan and sends (relative), mute, solo,
  arm, inserts bypass, automation mode, polarity, and editing (a click on a clip or a track in
  the timeline takes the members' too, so cuts, moves, trims and fades apply to all). Hold Ctrl
  to use one track's control or select one clip alone; Ctrl+Shift+G switches every group off
  and on. **VCAs**: Add VCA with tracks selected makes a fader that moves their levels
  together.
- **Snapshots** save and recall the console; the master's **MONITOR** section holds effects
  you hear but that are not exported (room correction, reference).
- **Master**: loudness (LUFS) and the highest sample peak beside the meter, restarted by
  **Reset** under the fader. **REF** beside it (also in the transport bar) compares
  the mix with a finished song (Transport › *Reference track…* to choose it): click it to hear
  the reference instead of the mix, at the same place in the song and, by default, at the
  mix's loudness, so you compare the sound and not the level. It is never exported.

### Automation

Show a track's lanes with its **A** button and add lanes with **+**. Click a lane to add a
point, drag points, double-click to delete one, right-click for the curve shape. The mode
button under M/S/R decides what moving a control does: Read plays the lanes, Touch and Latch
write while you move a control (Touch returns to the lane when you let go), Write records
everything while playing, Trim offsets the existing curve.

## 8. Effects and plugins

The effect browser lists the built-in effects by family:

- **Dynamics**: Compressor (Clean, Bus, Opto, FET), Gate, Limiter,
  Maximizer (true-peak mastering limiter), Transient Shaper, De-esser.
- **EQ & Filter**: EQ (up to 16 bands on the curve), Vintage EQ (passive tube style), Filter.
- **Space**: Reverb (Room, Hall, Space, Plate, Spring, Shimmer), Delay (Digital, Tape,
  Analog, tempo sync).
- **Modulation**: Chorus, Flanger, Phaser. **Color**: Drive, Tape.
- **Stereo**: Stereo Imager. **Utility**: Utility, Analyzer, Tuner. **Amp & Cab**: Amp.

Installed plugins join these families, after the built-in effects, from what each declares
(CLAP features, VST3 subcategories, LV2 class); those that declare nothing precise go to
**Other**, and **Plugins** lists them all. Right-click a plugin's card › *Family* to file it
elsewhere (*Automatic* goes back to what it declares). The **Vendor** and **Format** menus
beside the search keep one vendor's (or one format's) effects; clicking the vendor on a
plugin's card does the same.

**EQ**: double-click the curve to add a band (a cut near the ends, a bell elsewhere), drag a
band to move it, Ctrl+drag it (up narrower, down wider) or the mouse wheel over it sets its
Q, double-click it to remove it, right-click
it for its type (bell, low and high shelf, low and high cut from 6 to 48 dB per octave, notch,
band pass, tilt), *Dynamic band*, *Solo* and *Bypass*. Under the curve, the selected band's
type, slope, frequency, gain and Q. A **dynamic** band moves its gain by up to its *Range*
while its frequencies are above the *Threshold* (a negative range tames a resonance only when
it rings); its range shows as a line under the band, the gain it has now as a ring. **Solo**
plays only the frequencies the band works on, to find what to cut; it stops when the window
closes. EQs of earlier projects open with their bands where they were.

**Reverb**: pick a space (Room, Hall, Space, Plate, Spring or Shimmer; hover a button for what
it sounds like). Shimmer is a vast space whose tail rises an octave as it rings; its Shimmer
knob sets how much. Decay is in seconds; Pre-delay holds the tail back from the dry sound, High cut
and Low cut trim it, Width spreads it, Ducking lowers it while the input plays (the tail
comes back in the gaps), Mix sets how much is heard. *Freeze* holds the tail forever, for
pads. The smaller knobs below set Size, Early / late, Treble decay, Bass decay, Diffusion and
Modulation; those a space does not use are greyed (the plate and the spring have no size,
early reflections or bass decay, the spring no modulation). The display shows how
the lows and highs fade.

**Drive**: pick a type (Overdrive, Distortion, Fuzz, Tube, Tape, Transformer, Rectifier;
hover a button for what it sounds like); the curve shows how it bends the signal. Drive,
Tone, Level and Mix are the pedal's knobs; Low cut tightens the lows before the drive, Bias
makes the clipping asymmetric (warmer, even harmonics), Bass and Middle shape the sound after
it.

Every effect has factory presets, user presets and A/B comparison at the top of its window.
**Effect chains** keep a whole list of effects with their settings: right-click an effect (or
use the track menu) › *Save effects as a chain*, then add it to any track from *+ insert* ›
*Chains*, which also holds factory chains (Vocal, Drum bus, Acoustic guitar, Master for
streaming). Right-click a chain of yours there to delete it.
Effect windows are windows of their own, which can go to another screen; clicking an insert
again brings its window to the front. They stay above AudioStrada while you use it, and so do the
plugins' own windows, on Windows and X11. Under Wayland AudioStrada runs in X11 mode for that
(Preferences › General › *Several screens*); untick it if a fractional display scale (125 %,
150 %) makes the windows blurry, at the cost of windows that can fall behind.

**Limiter** and **Maximizer** look like a mastering maximizer: the input meter on the left
with the **threshold** (drag it down to push the input harder), the output meter on the right
with the **ceiling**, and between them the music scrolling by, red where the limiter takes
peaks away. Below: gain reduction, output peak, short-term loudness and how much louder the
output is. **Learn** listens while you play the loudest part and moves the threshold until the
output reaches the chosen loudness (-14 LUFS for streaming, for instance).

**Amp** is a whole guitar or bass rig: up to eight pedals, an amp and a cabinet. The arrows
beside the amp and the cabinet step through them (the amp arrows go through the built-in amps,
then your favourite captures); click the amp's name or the cabinet for the full lists. The
window has three sections, Pedalboard, Amp and Cab. Click **+** to add a pedal; over a pedal,
the pencil changes it and the cross removes it; drag a pedal to move it.

**Browse TONE3000…** (top of the window, and in each section) opens TONE3000's free captures
by kind, Amps, Effects (pedals and outboard such as preamps and compressors) and Cabs
(impulse responses): Trending, Latest, your favorites and downloads (loaded as you scroll).
A click downloads the tone and loads it where it belongs: the amp, the next free pedal, or
the cabinet. **Search all of TONE3000…** searches the whole catalog in your browser. Click the top of a
pedal to change it, its footswitch to turn it on or off: 21 pedals after the classics (Tube
Screamer, Klon, Blues Driver, SD-1, DS-1, RAT, Metal Zone, Fuzz Face, Big Muff, Rangemaster,
Cry Baby wah, compressor, modulation, analog and tape delay, reverb). Click the amp's name to choose it:
**Built-in** amps play right away: 16 models after the most played amps (Twin, Super and
Deluxe Reverb, Tweed Deluxe, Bassman, AC30, JTM45, Plexi, JCM800, Friedman, Dumble, Dual
Rectifier, 5150, Diezel, SVT, B-15), each with its own preamp, tone stack and power stage,
and a matching cabinet; **My
library** lists your Neural Amp Modeler captures by category (Amps, Amps + cab, Pedals,
Other, Favorites): **Add captures…** copies .nam files into AudioStrada's library, sorted by gear;
right-click a card to rename it, change its category, star it or remove it. A folder of
captures of one amp is one amp, its Gain knob steps between them; **TONE3000** shows your favorites, downloads and the
trending tones once signed in (free account), and downloads a tone in one click. A capture
freezes the amp at one setting: Bass, Middle, Treble, Presence and Master shape the sound
after it.

**Plugins**: AudioStrada finds CLAP, VST3 and LV2 plugins in the standard folders and in folders
added in Preferences › Plugins (Windows plugins do not run on Linux). Each plugin runs in
its own process: if it crashes, AudioStrada keeps playing, the insert turns red and its menu offers
*Reload plugin*. A plugin opens its own window when it is added and when its insert is
clicked (again: the window comes to the front); right-click › *Presets, levels and
parameters…* opens AudioStrada's window for it (presets, A/B, levels, Match, every parameter). A
plugin without a window AudioStrada can show opens AudioStrada's window instead.

Shortcuts work from effect windows too. In a plugin's own window, the transport's shortcuts
(Space, R, Home, L…) go to AudioStrada rather than to the plugin, on Linux (X11 or X11 mode); the
other keys stay the plugin's. A plugin that opens a window of its own (not inside AudioStrada's)
keeps its keys.

## 9. Export

File › Export (Ctrl+Shift+E): WAV (16, 24 or 32-bit float), FLAC (16 or 24-bit), MP3 or
Opus, any sample rate, dither, peak or loudness normalization, the whole project or the loop range, a
tail for reverbs, the mix and/or one file per track (stems). Exports run faster than real time
in the background. A single track can also be exported from its header menu. *Add to
queue…* keeps an export (and the project as it is) to render later with others.

**Freeze** renders a track with its effects and plays the result instead, to save processing
power; **Bounce** renders it onto a new track.

## 10. Shortcuts

Preferences › Keyboard picks a preset (AudioStrada, Pro Tools, Logic, Reaper) and changes any
shortcut. A few defaults (AudioStrada preset):

| Key | Command |
|---|---|
| Space | Play / stop (back to where playback started) |
| R | Record |
| Home / End | Go to start / end |
| L / Shift+L | Toggle loop / loop the selection |
| K | Metronome |
| S | Split at the playhead |
| Ctrl+D | Duplicate |
| M, . and , | Add marker, next and previous marker |
| + / - / F, Ctrl+wheel | Zoom in, zoom out (around the pointer when it is over the clips, else the playhead), zoom to fit |
| G | Snap |
| F1 / F2 / F4 | Shuffle / Slip / Grid mode |
| Ctrl+Shift+P | Command palette |
| Ctrl+Shift+D / Ctrl+Delete | Duplicate / delete the selected tracks |
| Ctrl+Up / Down, Alt+Up / Down | Select / move tracks |
| Shift+Alt+M / S / R | Mute / solo / arm the selected tracks |
| Ctrl+G | Group the selected tracks |
| Ctrl+Shift+G | Groups on or off |
| Ctrl+Shift+T / Ctrl+Shift+N | Add an audio track below the selection / add several |
| Ctrl+M | Mixer in its own window (again to dock it) |
| Ctrl+P | Piano roll in its own window (again to dock it) |
| Alt+1 … Alt+5 | Show or hide the mixer, piano roll, browser, inspector, markers |
| F11 | Full screen (View › *Full screen*), with the mixer and piano roll windows when detached |
| Ctrl+Shift+Up / Down, Alt+wheel | Taller / shorter tracks, keeping the track under the pointer in place |
| Alt+Shift+Up / Down, Alt+Shift+wheel | Waveform zoom in / out |
| Ctrl+U / Ctrl+H / Ctrl+Shift+C | Strip silence / heal / consolidate |

The bar under the clips scrolls the session: drag it, or click beside it to go there. Zoomed
in far enough, the waveform shows each channel and the samples themselves (to check
the phase between two tracks). Right-click the metronome button (or Transport › Metronome
settings…) for its window: sound, clicks per beat, accents, groups for odd meters (7/8 as
2+2+3), clicking only while recording, count-in.

## 11. Language and themes

Preferences › General › *Language* shows AudioStrada in English or French; *Automatic* follows the
system's language.

Preferences › General shows the themes as small pictures: Modern, Midnight, Console (sharp
corners, lit meters, ridged fader caps), Neon, Vintage, Studio gray, Light and Paper. Each has
its colors and its look (corners, meters, fader caps). *Own accent color* replaces the theme's
accent with yours. *Interface scale* takes effect with *Apply* (*Cancel* puts the slider back).

## 12. Control by scripts and AI agents

AudioStrada speaks the Model Context Protocol (MCP), so an AI agent (Claude Code, Claude Desktop,
any MCP client) or a script can build and mix a project: import audio, add tracks and
effects, set levels, cut, strip silence, run Beat Detective, export and save. The agent first
calls `inspect_project`, which describes the tracks, clips, inserts and sends.

- `audiostrada mcp` works on a project of its own, without a window (it can create or open one).
  For Claude Code: `claude mcp add audiostrada -- audiostrada mcp`.
- `audiostrada --control 9000 song.strada` opens AudioStrada as usual and listens on that local port;
  `audiostrada mcp --connect 9000` then drives this running AudioStrada, where every change shows and
  can be undone. It can also run any menu command by name. Only programs of the same user
  can connect: the channel needs the token AudioStrada writes to `~/.config/audiostrada/control-token`.

## 13. Troubleshooting

- **Crackles or dropouts**: raise the buffer size; run `audiostrada diagnose` on Linux.
- **No sound**: check the device in Preferences › Audio and that the master is not muted.
- **A plugin misbehaves**: right-click its insert › Reload plugin, or remove it; plugins
  cannot crash AudioStrada itself.
- **Starting without plugins**: `audiostrada --disable-third-party-plugins` neither scans nor loads
  CLAP, VST3, LV2 or AU plugins for that session (built-in effects stay, plugin inserts keep
  their settings); Preferences › Plugins › *Disable third-party plugins* does the same from
  the next start on.
- **Logs**: start AudioStrada from a terminal with `AUDIOSTRADA_LOG=debug audiostrada` and include the output
  when reporting a problem.
