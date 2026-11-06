# Security

## Reporting a vulnerability

Report it privately through [GitHub's security advisories](https://github.com/audiostrada/audiostrada/security/advisories/new),
or by e-mail to contact@audiostrada.com, never in a public issue. You get an answer within a
week; once fixed, the advisory credits you unless you prefer otherwise.

## What matters most

- **Project files and media**: a `.strada` project, an audio, MIDI or preset file, or an amp
  capture that crashes AudioStrada or runs code when opened.
- **The control channel of the AI assistants**: it listens on 127.0.0.1 and needs the token in
  `~/.config/audiostrada/control-token`; anything that reaches it without the token, or from
  another user or another computer.
- **Plugin sandboxing**: a third-party plugin able to crash AudioStrada or reach past its own
  process.
- **Licences and updates**: forging a licence, or the update check pointing somewhere else.

## Supported versions

Fixes go into the latest release.
