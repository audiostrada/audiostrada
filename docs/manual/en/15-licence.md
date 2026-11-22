# Licence and updates

AudioStrada is free software under the GNU GPL: its source code is open, and whoever builds it
from source uses it with no limit. The official builds (signed installers and their updates)
come with a yearly subscription, Personal or Commercial, which pays for the development.

## Evaluation

Without a licence, the official builds run in evaluation mode: every feature works, export
included. After 20 minutes, the sound drops out for a moment every 10 minutes. A licence removes
the dropouts.

## Entering a licence

Help › *License…* shows the state of this copy: evaluation, or the licence with its plan, its
holder and until when updates are included.

- *Enter a license*: paste the licence, or *Open a file…*, then *Activate*. Dropping the licence
  file on the window works too. The window then closes and *License activated* shows the plan,
  the holder and the updates included.
- *Buy a license* opens the shop on audiostrada.com.
- *Find my license* and *Manage my subscription* open your account on audiostrada.com, to get
  the licence again, change the subscription or cancel it.
- *Remove the license*, at the bottom left, takes it off this computer once confirmed.

| Licence | For |
|---|---|
| **Personal** | musicians who earn less than 20,000 € a year from music or audio work, even when they sell their tracks or play paid gigs |
| **Commercial** | studios, companies and professionals, for commercial work of any size |

Both are the same software. The versions you received while subscribed stay yours: when the
subscription ends, the window says until when updates were included, and AudioStrada keeps
working.

## Updates

With *Check once a day* ticked (Preferences › General), AudioStrada tells you when a new version
is out: click *Version … available* at the top right. On Windows and macOS, *Download and
install* downloads the installer and checks it (the release's checksums, and the signature of
AudioStrada's publisher); on Windows AudioStrada then closes, asking to save your projects, and
the installer starts; on macOS the disk image opens: drag AudioStrada onto Applications,
replacing the old one. Projects, preferences and presets stay.

Help › *Check for updates* looks right away, whatever the preference, and says when your version
is the latest.

After an update, Help › *What's new* opens by itself with the notes of the new version; it can be
opened again at any time.

On Linux, the `.deb` and `.rpm` packages also add AudioStrada's package repository: new versions
then come with the system's updates (`sudo apt upgrade`, `sudo dnf upgrade` or the software
updater), and the update window gives the command. Otherwise it downloads the right package.
The AppImage updates itself like the Windows and macOS versions: *Download and install* checks
the new AppImage, puts it in place of the installed one (installing it if it was not), and
AudioStrada starts again.

### Keeping a version

The package repositories, Flathub and the Snap Store install new versions by themselves, even
those released after your updates ended, which run in evaluation mode. A month before the end,
AudioStrada tells you so and gives the command that keeps the installed version; after such an
update, it gives the one that goes back to the last version your licence covers.

| Installed with | Keep this version | Let updates in again |
|---|---|---|
| `.deb` | `sudo apt-mark hold audiostrada` | `sudo apt-mark unhold audiostrada` |
| `.rpm` | `sudo dnf versionlock add audiostrada` | `sudo dnf versionlock delete audiostrada` |
| Snap Store | `sudo snap refresh --hold audiostrada` | `sudo snap refresh --unhold audiostrada` |
| Flathub | `flatpak mask com.audiostrada.AudioStrada` | `flatpak mask --remove com.audiostrada.AudioStrada` |

To go back to version 1.2.0, for example: `sudo apt install --allow-downgrades
audiostrada=1.2.0-1` or `sudo dnf downgrade audiostrada-1.2.0`; `sudo snap revert audiostrada`
goes back to the previous version. On Flathub, `flatpak remote-info --log flathub
com.audiostrada.AudioStrada` lists the earlier versions, and `flatpak update
--commit=<commit> com.audiostrada.AudioStrada` installs one. On older Fedora versions,
`versionlock` comes with the `python3-dnf-plugin-versionlock` package.
