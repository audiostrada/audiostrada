# Contributing

Thank you for helping. Bug reports, tests on your hardware and pull requests are welcome.

## Before a pull request

- `cargo fmt --all`, `cargo clippy --workspace` (no warnings) and `cargo test --workspace`.
- Interface texts are English keys with a French translation in
  `crates/audiostrada-ui/assets/i18n/fr/` (checked by `every_text_has_its_french`).
- Keep comments short, in English.

## Contributor License Agreement

AudioStrada is released under the GPL, version 3 or later, and its official builds are sold to
fund its development. So that the project can keep doing so, and later offer AudioStrada under
other terms where the GPL cannot apply (an app store, for instance), contributions are
accepted under [CLA.md](CLA.md): you keep the copyright of your work, and you grant the
maintainer the right to distribute it under the GPL and under other licenses. A pull request
says you agree to it.
