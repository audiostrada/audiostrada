# Track icons

`../fonts/Lucide-TrackIcons.ttf` holds the instrument icons of the tracks:

- Lucide icons (ISC, see `../fonts/Lucide-LICENSE.txt`), subset from `lucide-static` 1.67.0 with
  `pyftsubset` to the code points used in `src/track_icons.rs`;
- AudioStrada's own icons in Lucide's style (GPLv3), drawn in the SVG files of this folder and
  added at U+E900 onwards with `add_glyphs.py` (needs `fonttools` and `picosvg`):

```
python add_glyphs.py ../fonts/Lucide-TrackIcons.ttf out.ttf E900=violin.svg E901=trumpet.svg \
    E902=sax.svg E903=bass.svg
```
