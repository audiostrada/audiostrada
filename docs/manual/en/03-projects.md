# Projects

## What a project holds

A project is a folder that contains:

- the `.strada` file, which describes the song;
- an `Audio` folder: recordings and imported files are copied there;
- a `Models` folder: amp captures and impulse responses;
- a `Versions` folder: earlier saves;
- a cache of waveform pictures, rebuilt when missing.

File › New, Open, Save ([[Ctrl+S]]) and *Save as*. AudioStrada also saves on its own; after a
crash, it offers to recover the unsaved work at the next start.

## Undo and versions

Everything can be undone ([[Ctrl+Z]]) and redone ([[Ctrl+Shift+Z]] or [[Ctrl+Y]]), as many steps
back as the session holds.

Each save keeps the file it replaces in the project's `Versions` folder (the last 30). File ›
*Project versions…* lists them and restores one in a single step that [[Ctrl+Z]] takes back.

## Importing audio

File › Import audio ([[Ctrl+I]]) or drop files on the timeline. Each file gets its own track,
named after it: the quickest way to bring in a set of stems. A stereo file makes a stereo track.

AudioStrada reads WAV, AIFF, CAF, FLAC, ALAC, MP3, MP2, AAC (`.m4a`, `.aac`), Ogg Vorbis, and the
audio of MP4, MOV, MKV and WebM files.

The **Browser** ([[Alt+3]]) lists the audio files of your folders, with a filter. With *Play on
select* ticked, selecting a file plays it. Drag a file onto the timeline, or double-click it to
add it on a new track.

![The browser](img/browser.webp)

## Keeping everything together

- File › *Collect media into the project folder* copies the files the project uses from
  elsewhere (audio, amp captures, impulses) into it, so that the folder holds everything.
- File › *Archive project…* packs the project with all its files into one `.tar.gz` file, to
  send or keep, without changing the project.

## MIDI files

File › *Import MIDI file* ([[Ctrl+Alt+I]]) creates a track per part. File › *Export MIDI file*
writes the selected track, or all of them.
