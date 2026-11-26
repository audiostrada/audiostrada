## 0.3.0

- Keyboard instruments: Strada-Tines, Strada-Reed and Strada-EP80 electric pianos, Strada-Tonewheel organ with the Strada-Rotary effect, Strada-FM Keys, Strada-Mallets, Strada-808 bass and Strada-Grain.
- Other DAWs: DAWproject import and export, Reaper project import. Plugins keep their names, even while not installed.
- MIDI files are imported onto the selected instrument track, or dropped on the timeline.
- Help › Check for updates.
- Effects re-voiced on measured references: compressor characters, channel strip EQs, Tube EQ, Tape, Drive, Chorus, Phaser, Flanger, Rotary, Verb, MB-Comp and Max.
- Playback keeps decoded audio in memory: no more gaps when the system is short of memory, and a warning when a project exceeds the budget.
- Express Mix recognizes keyboard tracks and keeps a stereo pair named L/R centered.
- File dialogs open where that kind of file was last picked.
- Plugins installed while AudioStrada runs are found without restarting it.
- Dialogs open in windows of their own; editors, the browser and plugin windows stay above the detached panels, the last one clicked in front.
- A window covering an open menu steps aside until the menu closes; a detached panel minimized docks back into the main window.
- macOS: floating windows hide with the main window and show in full screen; clicking a plugin's slot brings its window to the front again.
- Linux: AudioStrada no longer comes back in front of another application.
- Windows: the installer shows AudioStrada's logo; the plugin sandbox no longer stalls while a plugin shows its own dialog.

## 0.2.1

- The AppImage installs itself: at its first start, AudioStrada offers to put it in your programs and the applications menu, with no administrator password, and to open `.strada` files with it. Preferences › General installs or uninstalls it later.
- The AppImage now updates itself like the Windows and macOS versions: *Download and install* replaces it with the new version and AudioStrada starts again.
- A month before your license's updates end, AudioStrada tells you so and, if your system installs new versions by itself (package repository, Flathub, Snap Store), gives the command that keeps the installed version.
- macOS: floating windows and plugin windows now hide while the main window is minimized, and come back with it.
- Windows: the installer now requires Windows 10 or later.

## 0.2.0

- Updates install from AudioStrada: click *Version … available*, then *Download and install*. On Windows and macOS, AudioStrada downloads the installer, checks it, and starts it.
- On Linux, the .deb and .rpm packages add AudioStrada's package repository: new versions then come with your system's updates (`sudo apt upgrade`, `sudo dnf upgrade`).
- This window, What's new, shows what changed after each update. It opens again from the Help menu.
- Help › Manual opens the manual on the web site.
- Entering a license now closes the License window and confirms it, with its plan, its holder and the updates it includes.
- Without a license, test versions now run in evaluation mode, like the final version: after 20 minutes, the sound drops out for a moment every 10 minutes. A license removes the dropouts.
- The AppImage now shows AudioStrada's name, description and icon in software centers.

## 0.1.0

The first preview, for testers: recording, editing, mixing, automation, the built-in effects and instruments, and CLAP, VST3 and LV2 plugins.
