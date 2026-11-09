# Console measurements

Fingerprints of AudioStrada's consoles and of the plugins they are calibrated on: gain and
harmonics by frequency and level, intermodulation, memory after loud passages, noise, hum and
crosstalk (`crates/audiostrada-dsp/src/measure.rs`). Only these figures are kept, never the
plugins' audio.

- `references/builtin.toml`: AudioStrada's own consoles.
- `references/windows.toml`: the reference plugins, measured on Windows; written on the
  machine that measures them, kept out of the repository.

## Measuring

```
audiostrada measure measurements/references/builtin.toml --out fingerprints
audiostrada measure-report fingerprints --output report.html
```

`audiostrada measure --console "British Warm" --amount 70 --bus` measures one console alone.

## The reference plugins, on Windows

1. Install the plugins, then let AudioStrada find them: `audiostrada plugins`.
2. Write `references/windows.toml`, a `[[reference]]` per plugin (its `name`, its `plugin`
   key or name, its `params`), then list each one's parameters and set it neutral
   (EQ and dynamics out, trims at 0, analog modelling on):
   `audiostrada plugin-params "<plugin>"`.
3. Measure them all, then compare them with the built-in consoles:

   ```
   audiostrada measure measurements\references\windows.toml --out measurements\fingerprints\windows
   audiostrada measure-report measurements\fingerprints\windows fingerprints --output report.html
   ```

Plugins load in the measuring process: one that crashes stops the run, the fingerprints already
written stay.
