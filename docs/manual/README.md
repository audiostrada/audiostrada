# The manual

One Markdown file per chapter, in `en/` and `fr/` (the same chapters in the same order), and
`cheatsheet.md` for the one-page cheat sheet. The site (`audiostrada-site`) builds its `/manual/`
pages from these files.

- `[[Ctrl+S]]` writes keys, `> **Tip**` (or **Note**, **Warning**, **Astuce**, **Remarque**) a
  note, `![caption](img/file.webp)` a figure.
- `<!-- effect:reverb -->` inserts the parameters of a built-in effect, with their range and
  default, and its factory presets, named as the interface names them in each language.

## Building the PDFs

```
node docs/manual/build.mjs              # both languages, into docs/manual/dist/
node docs/manual/build.mjs --lang fr    # one language
node docs/manual/build.mjs --effects    # first refresh effects.json from the audiostrada binary
```

The PDFs need [WeasyPrint](https://weasyprint.org): on the PATH, in `$WEASYPRINT`, or run through
`uvx weasyprint` when uv is installed. `--effects` runs `audiostrada mcp` (or `$AUDIOSTRADA`) to
list the effects; run it after changing an effect's parameters.

Fonts: Archivo (`assets/fonts`, SIL Open Font License) and Inter (from the program's assets).
