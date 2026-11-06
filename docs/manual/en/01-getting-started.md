# Getting started

AudioStrada records, edits and mixes music, from the first take to the master. This chapter
installs it, sets up the sound and walks through a first song in ten minutes. Every later
chapter goes deeper into one part of the program.

## Install

Download the package for your system from **audiostrada.com/download**:

| System | Package |
|---|---|
| Linux | `.deb` (Debian, Ubuntu), `.rpm` (Fedora, openSUSE) or `.AppImage` (any distribution) |
| Windows | the `-setup.exe` installer |
| macOS | the `.dmg` disk image |

Open a project by double-clicking a `.strada` file, or start AudioStrada from the menu of your
system. Without a licence, AudioStrada runs in evaluation mode: every feature works, export
included (see *Licence and updates*).

## Set up the sound

On Linux, AudioStrada plays through PipeWire with no setup; JACK and ALSA are also available. On
Windows choose WASAPI (shared or exclusive) or ASIO; on macOS, CoreAudio.

**Preferences › Audio** ([[Ctrl+,]]) chooses the backend, the device, the sample rate and the
buffer size, shows the resulting latency, plays a test tone and shows the input levels.

> **Tip** If the sound crackles, raise the buffer size. On Linux, `audiostrada diagnose` checks
> the system for low-latency audio (real-time priority, CPU governor, PipeWire quantum) and
> prints the exact fix for each problem.

## A first song in ten minutes

1. **Create a project**: File › New project. Choose a folder: everything the song uses will
   live in it.
2. **Bring in sound**: drop audio files on the timeline, or File › Import audio ([[Ctrl+I]]).
   Each file gets its own track, named after it.
3. **Record**: on an empty track, choose the input in the header (*In 1*), arm it with its **R**
   button and press [[R]]. Press [[Space]] to stop.
4. **Add drums**: Track › Add drum machine track. In the drum machine's **Pattern** tab, click
   *New pattern*, then *Start from a rhythm*.
5. **Edit**: drag clips to move them, drag their edges to trim them, their top corners for fades.
   [[S]] splits at the playhead, [[Ctrl+Z]] undoes anything.
6. **Mix**: in the **Mixer** below the timeline, set the levels with the faders, click *+ insert*
   to add an effect (a compressor on the voice, a reverb on a send).
7. **Export**: File › Export ([[Ctrl+Shift+E]]), choose WAV or MP3, and export.

AudioStrada saves on its own as you work, and every save keeps the previous version: nothing is
ever lost.

## Where to find a command

Every command is in the menus and in the **command palette** ([[Ctrl+Shift+P]]), which finds it
by name and shows its shortcut. **Help › Keyboard shortcuts** ([[Ctrl+/]]) lists them all, and
right-clicking almost anything shows its options: clips, track headers, the ruler, mixer strips,
inserts, faders and knobs.

> **Note** On macOS, [[Cmd]] replaces [[Ctrl]] in every shortcut of this manual.
