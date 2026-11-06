# Contributing

Thank you for helping. Bug reports, tests on your hardware and pull requests are welcome.

## Reporting and proposing

- Bugs, plugin problems and feature requests go through the [issue forms](https://github.com/audiostrada/audiostrada/issues/new/choose).
- Questions and ideas still taking shape go to [Discussions](https://github.com/audiostrada/audiostrada/discussions).
- Issues labelled `good first issue` are a good place to start; comment on one before working on
  it, so that two people do not do the same work.

## Labels

Each issue gets a **type** (bug, feature, plugin compatibility…), an **area** (timeline, mixer,
effects…), the **os** it concerns when it matters, and a **status**: `status: triage` until
someone looks at it, then `needs info`, `confirmed`, `in progress` or `blocked`. Pull requests
get their area labels from the files they change. `sh .github/labels.sh` recreates the labels.

## Before a pull request

- `cargo fmt --all`, `cargo clippy --workspace` (no warnings) and `cargo test --workspace`.
- Interface texts are English keys with a French translation in
  `crates/audiostrada-ui/assets/i18n/fr/` (checked by `every_text_has_its_french`).
- When users see the change, the manual says so: `docs/manual/en` and `docs/manual/fr`.
- Keep comments short, in English.

## Contributor License Agreement

AudioStrada is released under the GPL, version 3 or later, and its official builds are sold to
fund its development. So that the project can keep doing so, and later offer AudioStrada under
other terms where the GPL cannot apply (an app store, for instance), contributions are
accepted under [CLA.md](CLA.md): you keep the copyright of your work, and you grant the
maintainer the right to distribute it under the GPL and under other licenses. A pull request
says you agree to it.
