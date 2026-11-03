import sys
from picosvg.svg import SVG
from fontTools.ttLib import TTFont
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.svgLib.path import parse_path
font_path, out = sys.argv[1], sys.argv[2]
icons = sys.argv[3:]  # pairs codepoint=file.svg
f = TTFont(font_path)
glyf, hmtx = f['glyf'], f['hmtx']
order = f.getGlyphOrder()
s = 1000 / 24
for item in icons:
    cp, path = item.split('=')
    cp = int(cp, 16)
    pico = SVG.parse(path).topicosvg()
    pen = TTGlyphPen(None)
    tpen = TransformPen(Cu2QuPen(pen, max_err=1.0, reverse_direction=True), (s, 0, 0, -s, 0, 1000))
    for shape in pico.shapes():
        parse_path(shape.d, tpen)
    name = 'uni%04X' % cp
    glyf[name] = pen.glyph()
    hmtx[name] = (1000, 0)
    if name not in order:
        order.append(name)
    for table in f['cmap'].tables:
        if table.isUnicode():
            table.cmap[cp] = name
f.setGlyphOrder(order)
f['maxp'].numGlyphs = len(order)
f.save(out)
print('ok', len(order))
