# Preferences

View › *Preferences…* ([[Ctrl+,]]) has five tabs: General, Audio, Keyboard, Plugins and MIDI.

![Preferences](img/prefs.webp)

## General

- **Appearance**: the themes, shown as small pictures: Modern, Midnight, Console (sharp corners,
  lit meters, ridged fader caps), Neon, Vintage, Studio gray, Light and Paper. Each has its
  colours and its look. *Own accent color* replaces the theme's accent with yours.
- **Language**: English or French; *Automatic* follows the system's language.
- **Interface scale** takes effect with *Apply* (*Cancel* puts the slider back).
- **Workspace**: the play key (*Stop goes back to where playback started*, or stays where it
  is), input monitoring of armed tracks, the recording latency offset.
- **Several screens**: puts each window back on its screen (X11 mode, see *Plugins*).
- **New versions**: *Check once a day* tells you when a new official build is out. Nothing is
  installed on its own: AudioStrada points you to the download page.

## Audio

The backend (PipeWire, JACK, ALSA; WASAPI or ASIO on Windows; CoreAudio on macOS), the output
and input devices, the sample rate, the buffer size and the channels, with the latency they
give. *Test tone* plays 440 Hz at -18 dBFS; the input meters show what comes in. On Linux,
**System check** runs the same checks as `audiostrada diagnose` and shows how to fix each
problem.

## Keyboard

Pick a preset (**AudioStrada**, **Pro Tools**, **Logic**, **Reaper**) and change any shortcut:
click *Change*, press the new keys ([[Escape]] cancels). The search finds a command by name.

## Plugins

The plugins found, the scan buttons, the standard folders and your own (*Add folder…*), the
sandbox (each plugin in its own process), and *Disable third-party plugins*.

## MIDI

The MIDI inputs: tick the keyboards and controllers to use. *Refresh* finds a device plugged in
after the start.
