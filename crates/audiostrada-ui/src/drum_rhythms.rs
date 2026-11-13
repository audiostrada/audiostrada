//! The ready-made rhythms of Strada-Drums, by genre: each a main groove, a variation and a fill.

use audiostrada_core::Note;
use audiostrada_dsp::drums::VOICES;

/// A ready-made rhythm. Each line is a pad id and its sixteenth-note steps, a bar per 16 steps
/// (a line shorter than the variation repeats): `X` accented, `x` normal, `o` ghost, `f` flam,
/// `r` a roll of two thirty-second notes, `t` a triplet roll of three, `.` rest.
pub(crate) struct Rhythm {
    /// Its name within its genre, in English (translated when shown).
    pub name: &'static str,
    /// One of [`GENRES`].
    pub genre: &'static str,
    pub bpm: u32,
    /// 50 plays straight, 66 swings the off sixteenths to triplets.
    pub swing: f32,
    /// The main groove, the variation (may be empty), the fill (one bar, may be empty).
    pub a: &'static [(&'static str, &'static str)],
    pub b: &'static [(&'static str, &'static str)],
    pub fill: &'static [(&'static str, &'static str)],
}

/// The genres, in the order they are listed.
pub(crate) const GENRES: [&str; 14] = [
    "Hip-hop",
    "Trap and drill",
    "R&B and pop",
    "House",
    "Techno",
    "Electro and breaks",
    "Drum and bass",
    "UK garage and bass",
    "Latin and Caribbean",
    "Afro",
    "Funk, soul and disco",
    "Rock",
    "Synthwave and 80s",
    "Lo-fi and chill",
];

pub(crate) const RHYTHMS: &[Rhythm] = &[
    // Hip-hop
    Rhythm {
        name: "Boom bap",
        genre: "Hip-hop",
        bpm: 90,
        swing: 58.0,
        a: &[
            ("kick", "X......x..x.....X......x..x..x.."),
            ("snare", "....X.......X.......X.......X..o"),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.o."),
        ],
        b: &[
            ("kick", "X.x....x..x.....X......x.X..x..."),
            ("snare", "....X..o....X.......X..o....X.o."),
            ("closed", "x.x.x.x.x.x.x...x.x.x.x.x.x.x.x."),
            ("open", "..............x................."),
        ],
        fill: &[
            ("kick", "X......x..x....."),
            ("snare", "....X.....o.XoXX"),
            ("closed", "x.x.x.x.x.x....."),
        ],
    },
    Rhythm {
        name: "East coast",
        genre: "Hip-hop",
        bpm: 94,
        swing: 56.0,
        a: &[
            ("kick", "X.....x...X.....X.....x...X..x.."),
            ("snare", "....X.......X..."),
            ("closed", "X.x.x.x.X.x.x.x."),
        ],
        b: &[
            ("kick", "X.....x...X..x..X.x...x...X....."),
            ("snare", "....X.......X..o"),
            ("closed", "X.x.x.x.X.x.x.x.X.x.x.x.X.x.xxx."),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X.....x...X....."),
            ("snare", "....X.......X.rr"),
            ("closed", "X.x.x.x.X.x....."),
        ],
    },
    Rhythm {
        name: "West coast",
        genre: "Hip-hop",
        bpm: 94,
        swing: 56.0,
        a: &[
            ("kick", "X.......X.x.....X.......X..x...."),
            ("clap", "....X.......X..."),
            ("snare", "....x.......x..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.....x.X.x.....X.......X..x..x."),
            ("clap", "....X.......X..."),
            ("closed", "x.x.x...x.x.x.x."),
            ("open", "......x........."),
        ],
        fill: &[
            ("kick", "X.......X.x....."),
            ("clap", "....X.......X.X."),
            ("closed", "x.x.x.x.x.x....."),
            ("open", "............x..."),
        ],
    },
    Rhythm {
        name: "G-funk",
        genre: "Hip-hop",
        bpm: 92,
        swing: 60.0,
        a: &[
            ("kick", "X.......X..x..x.X.......X......."),
            ("snare", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("tambourine", "....x.......x..."),
        ],
        b: &[
            ("kick", "X.......X..x..x.X..x....X..x...."),
            ("snare", "....X.......X.......X.......X..o"),
            ("closed", "xoxoxoxoxoxoxoxoxoxoxoxoxoxo..x."),
            ("open", "............................x..."),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X.......X..x...."),
            ("snare", "....X.....o.X.XX"),
            ("closed", "xoxoxoxoxoxo...."),
            ("tambourine", "....x.......x..."),
        ],
    },
    Rhythm {
        name: "Dilla swing",
        genre: "Hip-hop",
        bpm: 88,
        swing: 64.0,
        a: &[
            ("kick", "X.......x..x....X.x.....x..x...."),
            ("snare", "....X.......X.......X..o....X..."),
            ("rim", "..o.......o....."),
            ("closed", "x.xox.x.x.xox.x."),
        ],
        b: &[
            ("kick", "X..x....x..x....X.x...x...x....."),
            ("snare", "....X..o....X.......X......oX..."),
            ("closed", "x.xox.xox.xox.xo"),
        ],
        fill: &[
            ("kick", "X.......x..x...."),
            ("snare", "....X..o..o.XoXo"),
            ("closed", "x.xox.x.x......."),
        ],
    },
    Rhythm {
        name: "Memphis",
        genre: "Hip-hop",
        bpm: 72,
        swing: 50.0,
        a: &[
            ("kick", "X.....x...x.....X.....x..x......"),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.t.x.x.x.x.x.x.x.t.t."),
            ("cowbell", "x..x..x.x..x..x."),
        ],
        b: &[
            ("kick", "X.....x...x..x..X.x...x...x....."),
            ("snare", "....X.......X.......X.......X.o."),
            ("closed", "x.x.t.x.x.x.t.x.x.x.t.x.x.t.t.t."),
            ("cowbell", "x..x..x.x..x..x.x..x..x...x.x.x."),
        ],
        fill: &[
            ("kick", "X.....x...x....."),
            ("snare", "....X.......X.rr"),
            ("closed", "x.t.x.t.t.t.tttt"),
            ("cowbell", "x..x..x.x..x...."),
        ],
    },
    Rhythm {
        name: "Phonk",
        genre: "Hip-hop",
        bpm: 130,
        swing: 50.0,
        a: &[
            ("kick", "X......x..X.....X......x..X..x.."),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.x.x.rrx."),
            ("cowbell", "x..x..x.x..x..x.x..x..x...x.x.x."),
        ],
        b: &[
            ("kick", "X.x....x..X.....X.x....x..X.x..."),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.rrrrx.x."),
            ("cowbell", "x..x..x.x..x..x.x..x..x.x.x.x.x."),
        ],
        fill: &[
            ("kick", "X......x..X....."),
            ("snare", "....X...X.X.XrXr"),
            ("closed", "x.x.x.x.rrrr...."),
            ("cowbell", "x..x..x.x..x...."),
        ],
    },
    Rhythm {
        name: "Old school",
        genre: "Hip-hop",
        bpm: 98,
        swing: 54.0,
        a: &[
            ("kick", "X.......X.X.....X.......X.X..x.."),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X..x....X.X.....X..x....X...X..."),
            ("snare", "....X.......X..."),
            ("clap", "....X.......X..."),
            ("closed", "x.xxx.x.x.xxx.x."),
        ],
        fill: &[
            ("kick", "X.......X.X....."),
            ("snare", "....X...X.X.XXXX"),
            ("closed", "x.x.x.x........."),
        ],
    },
    // Trap and drill
    Rhythm {
        name: "Atlanta trap",
        genre: "Trap and drill",
        bpm: 140,
        swing: 50.0,
        a: &[
            ("kick", "X......x...x....X.x.......x....."),
            ("clap", "........X......."),
            ("closed", "x.x.x.x.rrx.x.x.x.x.x.x.x.rrrrx."),
            ("open", "..............x................."),
        ],
        b: &[
            ("kick", "X..x...x...x....X.x....x..x..x.."),
            ("clap", "........X......."),
            ("snare", "........X.....o."),
            ("closed", "x.x.x.x.ttx.x.x.x.x.rrx.x.t.t.t."),
        ],
        fill: &[
            ("kick", "X......x...x...."),
            ("clap", "........X......."),
            ("snare", "........X..oXoXX"),
            ("closed", "x.x.t.t.rrrrtttt"),
        ],
    },
    Rhythm {
        name: "Trap bounce",
        genre: "Trap and drill",
        bpm: 140,
        swing: 50.0,
        a: &[
            ("kick", "X.....x...x.....X.x.....x..x...."),
            ("clap", "........X......."),
            ("closed", "x.x.x.x.x.x.rrrrx.x.x.x.x.rrx.x."),
            ("open", "......................x........."),
        ],
        b: &[
            ("kick", "X.....x...x..x..X.x.....x.x....."),
            ("clap", "........X......."),
            ("rim", "...x......x....."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.t.t.t.t."),
        ],
        fill: &[
            ("kick", "X.....x...x....."),
            ("clap", "........X...X.X."),
            ("closed", "x.x.x.x.rrrrrrrr"),
        ],
    },
    Rhythm {
        name: "Rage",
        genre: "Trap and drill",
        bpm: 150,
        swing: 50.0,
        a: &[
            ("kick", "X..x..x...X.....X..x..x...x.x..."),
            ("clap", "........X......."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.rrrrrrrr"),
            ("open", "..x.......x....."),
        ],
        b: &[
            ("kick", "X..x..x...X..x..X..x..x...X.x.x."),
            ("clap", "........X......."),
            ("closed", "x.x.x.x.ttttx.x.x.x.x.x.rrrrtttt"),
            ("open", "..x.......x....."),
            ("cymbal", "X..............................."),
        ],
        fill: &[
            ("kick", "X..x..x...X.X.X."),
            ("clap", "........X......."),
            ("closed", "rrrrrrrrtttttttt"),
            ("open", "..x............."),
        ],
    },
    Rhythm {
        name: "Plugg",
        genre: "Trap and drill",
        bpm: 145,
        swing: 50.0,
        a: &[
            ("kick", "X.......x.x.....X.........x....."),
            ("clap", "........X......."),
            ("rim", "..........o.........o.......o..."),
            ("closed", "x...x...x...x...x...x...x.x.x.x."),
        ],
        b: &[
            ("kick", "X.......x.x..x..X.....x...x....."),
            ("clap", "........X......."),
            ("rim", "..o.......o...o."),
            ("closed", "x...x...x...x.x.x...x...x.x.x.x."),
        ],
        fill: &[
            ("kick", "X.......x.x....."),
            ("clap", "........X...X..."),
            ("closed", "x...x...x.x.rrrr"),
        ],
    },
    Rhythm {
        name: "UK drill",
        genre: "Trap and drill",
        bpm: 142,
        swing: 50.0,
        a: &[
            ("kick", "X.........x.....X..x.......x...."),
            ("snare", "........X...............X..o...."),
            ("closed", "x..x..x.x..x..x.x..x..x.rrx..x.."),
        ],
        b: &[
            ("kick", "X.....x...x.....X..x....x.x....."),
            ("snare", "........X......o........X..o.X.."),
            ("closed", "x..x..x.x..x.tx.x..x..x.t.t.x..x"),
        ],
        fill: &[
            ("kick", "X.........x....."),
            ("snare", "........X..X.XXX"),
            ("closed", "x..x..x.tttttttt"),
        ],
    },
    Rhythm {
        name: "New York drill",
        genre: "Trap and drill",
        bpm: 142,
        swing: 50.0,
        a: &[
            ("kick", "X.....x...x.....X..x...x..x....."),
            ("kick2", "X.........x.....X.........x....."),
            ("snare", "........X......x........X......."),
            ("closed", "x..x..x.x..x..x.x..x..x.x..rrx.."),
        ],
        b: &[
            ("kick", "X..x..x...x.....X..x..x.x..x...."),
            ("kick2", "X.........x....."),
            ("snare", "........X..o...x........X...X..."),
            ("closed", "x..x..x.x..x..x.t..t..t.x..x..x."),
        ],
        fill: &[
            ("kick", "X.....x...x....."),
            ("kick2", "X.........x....."),
            ("snare", "........X.XX.XXX"),
            ("closed", "x..x..x.rrrrtttt"),
        ],
    },
    Rhythm {
        name: "Jersey club",
        genre: "Trap and drill",
        bpm: 140,
        swing: 50.0,
        a: &[
            ("kick", "X..X..X.X...X.X."),
            ("clap", "....X.......X..."),
            ("closed", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X..X..X.X.X.X.X.X..X..X.X...XXXX"),
            ("clap", "....X.......X.......X.......X.X."),
            ("closed", "..x...x...x....."),
            ("open", "..............x."),
        ],
        fill: &[
            ("kick", "X.X.X.X.XXXXXXXX"),
            ("clap", "....X.......X.X."),
            ("closed", "..x...x........."),
        ],
    },
    Rhythm {
        name: "Dark trap",
        genre: "Trap and drill",
        bpm: 135,
        swing: 50.0,
        a: &[
            ("kick2", "X.........x.....X...........x..."),
            ("snare", "........X......."),
            ("closed", "x...x...x...x.x.x...x...x...rrrr"),
            ("open", "......x........."),
        ],
        b: &[
            ("kick2", "X.....x...x.....X.....x.....x.x."),
            ("snare", "........X......."),
            ("clap", "........X......."),
            ("closed", "x.x.x...x.tttx..x...x.x.x.rrx.x."),
        ],
        fill: &[
            ("kick2", "X.....x...x.x.x."),
            ("snare", "........X..XXXXX"),
            ("closed", "x.x.tttttttt...."),
        ],
    },
    // R&B and pop
    Rhythm {
        name: "Modern R&B",
        genre: "R&B and pop",
        bpm: 72,
        swing: 54.0,
        a: &[
            ("kick", "X......x..x.....X.....x...x....."),
            ("snare", "....X.......X..."),
            ("rim", "..........o....."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.x.rrx.x."),
        ],
        b: &[
            ("kick", "X......x..x..x..X.x...x...x....."),
            ("snare", "....X.......X..o"),
            ("rim", "..o.......o....."),
            ("closed", "x.x.x.x.x.ttx.x.x.x.x.x.x.x.rrrr"),
        ],
        fill: &[
            ("kick", "X......x..x....."),
            ("snare", "....X.......XoXX"),
            ("closed", "x.x.x.x.tttttt.."),
        ],
    },
    Rhythm {
        name: "90s R&B",
        genre: "R&B and pop",
        bpm: 92,
        swing: 58.0,
        a: &[
            ("kick", "X..x....x.x.....X..x......x....."),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
            ("tambourine", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X..x....x.x..x..X..x....X.x....."),
            ("snare", "....X..o....X.......X..o....X..."),
            ("closed", "x.x.x.x.x.x.x..."),
            ("open", "..............x."),
            ("tambourine", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X..x....x.x....."),
            ("snare", "....X.....o.XoXX"),
            ("closed", "x.x.x.x.x.x....."),
        ],
    },
    Rhythm {
        name: "New jack swing",
        genre: "R&B and pop",
        bpm: 104,
        swing: 64.0,
        a: &[
            ("kick", "X..x..x...X..x.."),
            ("snare", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("tambourine", "....x.......x..."),
        ],
        b: &[
            ("kick", "X..x..x...X..x..X..x....X.x..x.."),
            ("snare", "....X.......X.......X.......X.X."),
            ("closed", "xoxoxo.oxoxoxoxo"),
            ("open", "......x........."),
        ],
        fill: &[
            ("kick", "X..x..x...X....."),
            ("snare", "....X...XoXoX.XX"),
            ("closed", "xoxoxoxo........"),
        ],
    },
    Rhythm {
        name: "Slow jam",
        genre: "R&B and pop",
        bpm: 66,
        swing: 56.0,
        a: &[
            ("kick", "X.........x.....X......x..x....."),
            ("rim", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
        ],
        b: &[
            ("kick", "X.........x..x..X......x..x....."),
            ("snare", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxo.o"),
            ("open", "..............x."),
        ],
        fill: &[
            ("kick", "X..............."),
            ("rim", "....X..........."),
            ("high_tom", "........x.x....."),
            ("mid_tom", "...........x.x.."),
            ("low_tom", "..............XX"),
            ("closed", "xoxoxoxo........"),
        ],
    },
    Rhythm {
        name: "Pop",
        genre: "R&B and pop",
        bpm: 100,
        swing: 50.0,
        a: &[
            ("kick", "X.......X.X....."),
            ("snare", "....X.......X..."),
            ("clap", "....x.......x..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.....x.X.x.....X.......X..x...."),
            ("snare", "....X.......X..."),
            ("clap", "....x.......x..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X.......X.X....."),
            ("snare", "....X.......XXXX"),
            ("clap", "....x..........."),
            ("closed", "x.x.x.x.x.x....."),
        ],
    },
    Rhythm {
        name: "Dance pop",
        genre: "R&B and pop",
        bpm: 122,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("snare", "....x.......x..."),
            ("closed", "xo..xo..xo..xo.."),
            ("open", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X..........."),
            ("snare", "x.x.x.x.xxxxXXXX"),
            ("closed", "..x...x........."),
        ],
    },
    // House
    Rhythm {
        name: "Deep house",
        genre: "House",
        bpm: 122,
        swing: 54.0,
        a: &[
            ("kick", "X...x...X...x..."),
            ("clap", "....X.......X..."),
            ("closed", "o...o...o...o..."),
            ("open", "..x...x...x...x."),
            ("rim", "......o.......o."),
        ],
        b: &[
            ("kick", "X...x...X...x..o"),
            ("clap", "....X.......X..."),
            ("closed", "oxo.oxo.oxo.oxo."),
            ("open", "..x...x...x...x."),
            ("rim", "...x..x....x...."),
        ],
        fill: &[
            ("kick", "X...x...X...x..."),
            ("clap", "....X.......X.X."),
            ("snare", "........o.o.oxxX"),
            ("open", "..x...x...x....."),
        ],
    },
    Rhythm {
        name: "Chicago house",
        genre: "House",
        bpm: 124,
        swing: 56.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "xo.oxo.oxo.oxo.o"),
            ("open", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("snare", "...o..o....o..o."),
            ("closed", "xo.oxo.oxo.oxo.o"),
            ("open", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("snare", "x...x.x.xxxxrrrr"),
            ("open", "..x...x........."),
        ],
    },
    Rhythm {
        name: "Garage house",
        genre: "House",
        bpm: 122,
        swing: 60.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "o.x.o.x.o.x.o.x."),
            ("rim", "...o..o....o...."),
        ],
        b: &[
            ("kick", "X...X...X.x.X..."),
            ("clap", "....X.......X..."),
            ("closed", "o.x.o...o.x.o..."),
            ("open", "......x.......x."),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.....X.X.X."),
            ("closed", "o.x.o.x.o.x....."),
            ("open", "..............x."),
        ],
    },
    Rhythm {
        name: "Tech house",
        genre: "House",
        bpm: 126,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "xo..xo.oxo..xo.o"),
            ("open", "..x...x...x...x."),
            ("rim", "...x..x....x..x."),
        ],
        b: &[
            ("kick", "X...X...X...X..o"),
            ("clap", "....X.......X..."),
            ("closed", "xo..xo.oxo..xo.o"),
            ("open", "..x...x...x...x."),
            ("rim", "..x..x....x..x.."),
            ("cowbell", ".......x.......x"),
        ],
        fill: &[
            ("kick", "X...X...X......."),
            ("clap", "....X.....x.X.XX"),
            ("open", "..x...x...x....."),
            ("rim", "...x..x..x.x.xxx"),
        ],
    },
    Rhythm {
        name: "Disco house",
        genre: "House",
        bpm: 124,
        swing: 54.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "xo..xo..xo..xo.."),
            ("open", "..X...X...X...X."),
            ("tambourine", "....x.......x..."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "xo.oxo.oxo.oxo.o"),
            ("open", "..X...X...X...X."),
            ("tambourine", "xoxoxoxoxoxoxoxo"),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("snare", "........o.o.xxXX"),
            ("open", "..X...X...X....."),
        ],
    },
    Rhythm {
        name: "Progressive house",
        genre: "House",
        bpm: 128,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "x.o.x.o.x.o.x.o."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("snare", "...o..o....o..o."),
            ("closed", "x...x...x...x..."),
            ("open", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....x.......x..."),
            ("snare", "x.x.x.x.x.x.xxxx"),
        ],
    },
    Rhythm {
        name: "Acid house",
        genre: "House",
        bpm: 124,
        swing: 52.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("rim", "...x..x....x..x."),
        ],
        b: &[
            ("kick", "X...X...X...X.x."),
            ("clap", "....X.......X..."),
            ("closed", "xo.oxo.oxo.oxo.o"),
            ("open", "..x...x...x...x."),
            ("cowbell", "...x.....x...x.."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X...X.X.XXXX"),
            ("closed", "xoxoxoxo........"),
            ("open", "..x...x........."),
        ],
    },
    // Techno
    Rhythm {
        name: "Detroit techno",
        genre: "Techno",
        bpm: 128,
        swing: 54.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("ride", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X...X...X...X..x"),
            ("clap", "....X.......X..."),
            ("snare", "......o..o....o."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("ride", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("snare", "....o.o.o.ooxxXX"),
            ("closed", "xoxoxoxoxoxo...."),
        ],
    },
    Rhythm {
        name: "Berlin techno",
        genre: "Techno",
        bpm: 132,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("kick2", "...o...o...o...o"),
            ("open", "..x...x...x...x."),
            ("rim", "......x.....x..x"),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("kick2", "..oo..oo..oo..oo"),
            ("clap", "....X.......X..."),
            ("closed", "xo..xo..xo..xo.."),
            ("open", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....x..x..x.x.xx"),
            ("open", "..x...x...x...x."),
            ("rim", "......x.....x..x"),
        ],
    },
    Rhythm {
        name: "Minimal",
        genre: "Techno",
        bpm: 126,
        swing: 54.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("closed", "..x...x...x...x."),
            ("rim", "...o..x.....o.x."),
            ("clave", "x.........x....."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....x.......x..."),
            ("closed", "..x...x...x...x."),
            ("rim", "...o..x..o..o.x."),
            ("tambourine", "oxoooxoooxoooxoo"),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....x.......x.x."),
            ("closed", "..x...x...x...x."),
            ("rim", "...o..x..o.ox.xx"),
        ],
    },
    Rhythm {
        name: "Dub techno",
        genre: "Techno",
        bpm: 122,
        swing: 52.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("closed", "..x...x...x...x."),
            ("rim", "......o.......o."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....o.......o..."),
            ("open", "..x...x...x...x."),
            ("rim", "......o..o....o."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....o.......o..."),
            ("closed", "..x...x...x...x."),
            ("rim", "......o..o..o.oo"),
        ],
    },
    Rhythm {
        name: "Hard techno",
        genre: "Techno",
        bpm: 145,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "xo..xo..xo..xo.."),
            ("open", "..X...X...X...X."),
        ],
        b: &[
            ("kick", "X...X...X...X.X."),
            ("clap", "....X.......X..."),
            ("snare", "...x..x....x..x."),
            ("closed", "xo..xo..xo..xo.."),
            ("open", "..X...X...X...X."),
        ],
        fill: &[
            ("kick", "X...X...X.X.XXXX"),
            ("clap", "....X.......X..."),
            ("snare", "........xxxxXXXX"),
            ("open", "..X...X........."),
        ],
    },
    Rhythm {
        name: "Industrial",
        genre: "Techno",
        bpm: 135,
        swing: 50.0,
        a: &[
            ("kick", "X..xX...X..xX..."),
            ("snare", "....X.......X..."),
            ("snare2", "..x.......x.x..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
        ],
        b: &[
            ("kick", "X..xX..xX..xX.xx"),
            ("snare", "....X.......X..."),
            ("snare2", "..x...x...x.x.x."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("cymbal", "X..............................."),
        ],
        fill: &[
            ("kick", "X..xX..xX.XXXXXX"),
            ("snare", "....X...X.X.XXXX"),
            ("snare2", "..x...x...x.x.x."),
        ],
    },
    // Electro and breaks
    Rhythm {
        name: "808 electro",
        genre: "Electro and breaks",
        bpm: 118,
        swing: 54.0,
        a: &[
            ("kick", "X.....x...x.....X.....x..x..x..."),
            ("clap", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
            ("cowbell", "...x......x........x......x..x.."),
        ],
        b: &[
            ("kick", "X.....x...x.x...X..x..x...x..x.."),
            ("clap", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("clave", "x..x..x...x.x..."),
        ],
        fill: &[
            ("kick", "X.....x...x....."),
            ("clap", "....X..........."),
            ("closed", "x.x.x.x........."),
            ("high_tom", "........x.x....."),
            ("mid_tom", "...........x.x.."),
            ("low_tom", "..............XX"),
        ],
    },
    Rhythm {
        name: "Breakbeat",
        genre: "Electro and breaks",
        bpm: 130,
        swing: 52.0,
        a: &[
            ("kick", "X.........X.....X.X.......X....."),
            ("snare", "....X..o.o..X..o....X..o....X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.X.......X..x..X.X...x...X....."),
            ("snare", "....X..o.o..X...o...X..o.o..X.o."),
            ("ride", "x.x.x.x.x.x.x.x."),
            ("open", "......x........."),
        ],
        fill: &[
            ("kick", "X.X.......X....."),
            ("snare", "....X..o.oX.XrXX"),
            ("closed", "x.x.x.x.x......."),
        ],
    },
    Rhythm {
        name: "Big beat",
        genre: "Electro and breaks",
        bpm: 125,
        swing: 54.0,
        a: &[
            ("kick", "X.....X...X..X.."),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
            ("tambourine", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X.....X...X..X..X.X...X...X....."),
            ("snare", "....X.......X.......X.......X.XX"),
            ("closed", "x...x...x...x..."),
            ("open", "..x...x...x...x."),
            ("tambourine", "xoxoxoxoxoxoxoxo"),
        ],
        fill: &[
            ("kick", "X.....X........."),
            ("snare", "x.x.x.x.xxxxrrrr"),
            ("tambourine", "xoxoxoxo........"),
        ],
    },
    Rhythm {
        name: "Miami bass",
        genre: "Electro and breaks",
        bpm: 128,
        swing: 50.0,
        a: &[
            ("kick2", "X......X..X.....X.....X.X......."),
            ("clap", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
        ],
        b: &[
            ("kick2", "X......X..X..X..X.X...X.X..X...."),
            ("clap", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("cowbell", "..x..x....x..x.."),
        ],
        fill: &[
            ("kick2", "X......X..X.X.X."),
            ("clap", "....X...X.X.XXXX"),
            ("closed", "xoxoxoxo........"),
        ],
    },
    Rhythm {
        name: "Freestyle",
        genre: "Electro and breaks",
        bpm: 118,
        swing: 50.0,
        a: &[
            ("kick", "X..x..x.X...x..."),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
            ("clave", "x..x..x...x.x..."),
        ],
        b: &[
            ("kick", "X..x..x.X...x...X..x..x.X..x.x.."),
            ("snare", "....X.......X..."),
            ("clap", "....x.......x..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
        ],
        fill: &[
            ("kick", "X..x..x.X......."),
            ("snare", "....X.......rrrr"),
            ("high_tom", "........x.x....."),
        ],
    },
    Rhythm {
        name: "Funky breaks",
        genre: "Electro and breaks",
        bpm: 110,
        swing: 56.0,
        a: &[
            ("kick", "X.X....x..X.....X.X.......X..x.."),
            ("snare", "....X..o.o..X.o.....X..o.o..X..."),
            ("closed", "x.x.x...x.x.x.x."),
            ("open", "......x........."),
        ],
        b: &[
            ("kick", "X.X....x..X..x..X..x..x...X....."),
            ("snare", "....X..o.o..X..o.o..X..o.o..X.oo"),
            ("ride", "x.x.x.x.x.x.x.x."),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X.X....x..X....."),
            ("snare", "....X..o.o......"),
            ("high_tom", "..........xx...."),
            ("mid_tom", "............xx.."),
            ("low_tom", "..............XX"),
        ],
    },
    // Drum and bass
    Rhythm {
        name: "Liquid",
        genre: "Drum and bass",
        bpm: 174,
        swing: 50.0,
        a: &[
            ("kick", "X.........x.....X.........x....."),
            ("snare", "....X.....o.X.......X..o....X..o"),
            ("closed", "x.x.x.x.x.x.x.x."),
            ("tambourine", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X.........x.....X.x.......x....."),
            ("snare", "....X..o..o.X.......X.o.o...X..."),
            ("ride", "x.xox.xox.xox.xo"),
            ("tambourine", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X.........x....."),
            ("snare", "....X.o.X.oXXrrr"),
            ("closed", "x.x.x.x.x.x....."),
        ],
    },
    Rhythm {
        name: "Rollers",
        genre: "Drum and bass",
        bpm: 174,
        swing: 50.0,
        a: &[
            ("kick", "X.........X..x.."),
            ("snare", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("rim", "..o.....o.o....."),
        ],
        b: &[
            ("kick", "X.........X..x..X.X.......X....."),
            ("snare", "....X..o....X.......X..o....X..o"),
            ("closed", "xoxoxoxoxoxoxo.o"),
            ("open", "..............x."),
        ],
        fill: &[
            ("kick", "X.........X....."),
            ("snare", "....X.......rrrr"),
            ("closed", "xoxoxoxoxoxo...."),
        ],
    },
    Rhythm {
        name: "Neurofunk",
        genre: "Drum and bass",
        bpm: 174,
        swing: 50.0,
        a: &[
            ("kick", "X.........X.....X.....X....X...."),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X..X......X.....X..X..X...X.X..."),
            ("snare", "....X.......X..."),
            ("snare2", "..........x....."),
            ("closed", "x.xxx.x.x.xxx.x."),
            ("cymbal", "X..............................."),
        ],
        fill: &[
            ("kick", "X.........X....."),
            ("snare", "....X..X..X.XXXX"),
            ("snare2", "........x.x....."),
            ("closed", "x.x.x.x........."),
        ],
    },
    Rhythm {
        name: "Jump up",
        genre: "Drum and bass",
        bpm: 174,
        swing: 50.0,
        a: &[
            ("kick", "X.........X.....X......X..X....."),
            ("snare", "....X.......X..."),
            ("clap", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.X.......X.....X.X....X..X....."),
            ("snare", "....X.......X..."),
            ("clap", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x..."),
            ("open", "..............x."),
        ],
        fill: &[
            ("kick", "X.........X....."),
            ("snare", "....X...XXXXrrrr"),
            ("clap", "....X..........."),
            ("closed", "x.x.x.x........."),
        ],
    },
    Rhythm {
        name: "Jungle",
        genre: "Drum and bass",
        bpm: 165,
        swing: 52.0,
        a: &[
            ("kick", "X.X.......XX....X.X.......X....."),
            ("snare", "....X..o.X..o..X....X..o.X....X."),
            ("ride", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.X.......XX....X.X...X...X..X.."),
            ("snare", "....X..o.X..o..X.o..X..o.X.X.XoX"),
            ("ride", "x.x.x.x.x.x.x.x."),
            ("closed", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X.X.......X....."),
            ("snare", "....X..o.XrX.XrX"),
            ("ride", "x.x.x.x.x......."),
        ],
    },
    Rhythm {
        name: "Halftime",
        genre: "Drum and bass",
        bpm: 170,
        swing: 50.0,
        a: &[
            ("kick", "X......x..X.....X.........X..x.."),
            ("snare", "........X......."),
            ("closed", "x...x...x.x.x...x...x.x.x...x.x."),
            ("rim", "...o......o....."),
        ],
        b: &[
            ("kick", "X..x......X..x..X.x.......X....."),
            ("snare", "........X.....o.........X...o.o."),
            ("closed", "x.x.x.xxx.x.x.xxx.x.x.xxx.x.rrrr"),
        ],
        fill: &[
            ("kick", "X......x..X....."),
            ("snare", "........X..X.XrX"),
            ("closed", "x...x...x.x.rrrr"),
        ],
    },
    // UK garage and bass
    Rhythm {
        name: "2-step",
        genre: "UK garage and bass",
        bpm: 132,
        swing: 62.0,
        a: &[
            ("kick", "X......x..X.....X.........X....."),
            ("snare", "....X.......X..o"),
            ("rim", "..x.....x.x...x."),
            ("closed", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X.........X..x..X......x..X....."),
            ("snare", "....X.......X.......X..o....X..."),
            ("rim", "..x.....x.x...x."),
            ("closed", "..x.......x...x."),
            ("open", "......x........."),
        ],
        fill: &[
            ("kick", "X......x..X....."),
            ("snare", "....X..o..o.X.XX"),
            ("rim", "..x.....x.x....."),
            ("closed", "..x...x........."),
        ],
    },
    Rhythm {
        name: "Speed garage",
        genre: "UK garage and bass",
        bpm: 130,
        swing: 56.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X.......X..."),
            ("open", "..X...X...X...X."),
            ("rim", "...o.....o..o..."),
        ],
        b: &[
            ("kick", "X...X...X..xX..."),
            ("snare", "....X.......X..o"),
            ("closed", "xo..xo..xo..xo.."),
            ("open", "..X...X...X...X."),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X...x.x.XXXX"),
            ("open", "..X...X........."),
        ],
    },
    Rhythm {
        name: "Grime",
        genre: "UK garage and bass",
        bpm: 140,
        swing: 50.0,
        a: &[
            ("kick", "X.........X.....X.....X........."),
            ("snare", "........X......."),
            ("clap", "........X......."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.....X...X.....X..X......X....."),
            ("snare", "........X.....o.........X..X...."),
            ("clap", "........X......."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.x.x.rrrr"),
        ],
        fill: &[
            ("kick", "X.........X....."),
            ("snare", "........X..XX.XX"),
            ("clap", "........X......."),
            ("closed", "x.x.x.x.rrrr...."),
        ],
    },
    Rhythm {
        name: "Dubstep",
        genre: "UK garage and bass",
        bpm: 140,
        swing: 50.0,
        a: &[
            ("kick", "X.........x.....X.........X..x.."),
            ("snare", "........X......."),
            ("closed", "x...x.x.x...x.x."),
        ],
        b: &[
            ("kick", "X.....x...x.....X.x.......X....."),
            ("snare", "........X...............X..o.o.."),
            ("closed", "x.x.x.x.x.x.x.x."),
            ("rim", "...o.......o...."),
            ("cymbal", "X..............................."),
        ],
        fill: &[
            ("kick", "X.........x....."),
            ("snare", "........X.xxXXXX"),
            ("closed", "x...x.x.x......."),
        ],
    },
    Rhythm {
        name: "Bassline",
        genre: "UK garage and bass",
        bpm: 138,
        swing: 56.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("open", "..x...x...x...x."),
            ("tambourine", "xo.oxo.oxo.oxo.o"),
        ],
        b: &[
            ("kick", "X...X...X.x.X..."),
            ("clap", "....X.......X..."),
            ("snare", "...o..o....o..o."),
            ("open", "..x...x...x...x."),
            ("tambourine", "xo.oxo.oxo.oxo.o"),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X...X.X.X.XX"),
            ("snare", "........o.o.o.o."),
            ("open", "..x...x........."),
        ],
    },
    Rhythm {
        name: "UK funky",
        genre: "UK garage and bass",
        bpm: 128,
        swing: 58.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("rim", "..x..x.x..x..x.."),
            ("closed", "x.o.x.o.x.o.x.o."),
        ],
        b: &[
            ("kick", "X...X..xX...X..."),
            ("clap", "....X.......X..."),
            ("rim", "..x..x.x..x..x.."),
            ("closed", "x.o.x.o.x.o.x.o."),
            ("low_tom", "......x.......x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("rim", "..x..x.x........"),
            ("low_tom", "..x..x....x.x.xx"),
        ],
    },
    // Latin and Caribbean
    Rhythm {
        name: "Reggaeton",
        genre: "Latin and Caribbean",
        bpm: 95,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("snare", "...X..x....X..x."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("snare", "...X..x..x.X..x."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("tambourine", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X...X...X......."),
            ("snare", "...x..x..xxxXXXX"),
            ("closed", "x.x.x.x........."),
        ],
    },
    Rhythm {
        name: "Dominican dembow",
        genre: "Latin and Caribbean",
        bpm: 120,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("snare", "..x..x.x..x..x.x"),
            ("clap", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("snare", "..x..x.x..x..xxx"),
            ("clap", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("tambourine", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("snare", "..x..x.xrrrrrrrr"),
            ("clap", "....X..........."),
        ],
    },
    Rhythm {
        name: "Dancehall",
        genre: "Latin and Caribbean",
        bpm: 98,
        swing: 52.0,
        a: &[
            ("kick", "X.....x.X.....x."),
            ("rim", "...x..x....x..x."),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X..x..x.X.....x."),
            ("rim", "...x..x....x..x."),
            ("snare", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
        ],
        fill: &[
            ("kick", "X.....x.X......."),
            ("rim", "...x..x....x...."),
            ("snare", "....X.......X.XX"),
        ],
    },
    Rhythm {
        name: "Reggae one drop",
        genre: "Latin and Caribbean",
        bpm: 76,
        swing: 60.0,
        a: &[
            ("kick", "........X......."),
            ("rim", "........X......."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "........X......."),
            ("snare", "........X......."),
            ("rim", "..o...........o."),
            ("closed", "x.x.x...x.x.x..."),
            ("open", "......x.......x."),
        ],
        fill: &[
            ("kick", "........X......."),
            ("snare", "....o.o.X.x....."),
            ("high_tom", "..........xx...."),
            ("mid_tom", "............xx.."),
            ("low_tom", "..............XX"),
        ],
    },
    Rhythm {
        name: "Moombahton",
        genre: "Latin and Caribbean",
        bpm: 108,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("rim", "...x..x....x..x."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("rim", "...x..x....x..x."),
            ("closed", "x...x...x...x..."),
            ("open", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....x.x.xxxxXXXX"),
            ("rim", "...x..x........."),
        ],
    },
    Rhythm {
        name: "Cumbia",
        genre: "Latin and Caribbean",
        bpm: 92,
        swing: 50.0,
        a: &[
            ("kick", "X.......X......."),
            ("rim", "....x.......x..."),
            ("tambourine", "X.xxX.xxX.xxX.xx"),
            ("open", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X.....x.X.....x."),
            ("rim", "....x.......x..."),
            ("tambourine", "X.xxX.xxX.xxX.xx"),
            ("open", "..x...x...x...x."),
            ("low_tom", "......x.......x."),
        ],
        fill: &[
            ("kick", "X.......X......."),
            ("tambourine", "X.xxX.xx........"),
            ("high_tom", "........x.x....."),
            ("low_tom", "............x.xx"),
        ],
    },
    Rhythm {
        name: "Bossa nova",
        genre: "Latin and Caribbean",
        bpm: 134,
        swing: 50.0,
        a: &[
            ("kick", "X..xX..xX..xX..x"),
            ("rim", "x..x..x...x..x....x..x...x..x..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
        ],
        b: &[
            ("kick", "X..xX..xX..xX..x"),
            ("rim", "..x..x...x..x...x..x..x...x..x.."),
            ("ride", "x.x.x.x.x.x.x.x."),
        ],
        fill: &[
            ("kick", "X..xX..xX..xX..x"),
            ("rim", "x..x..x........."),
            ("snare", "........o.o.xoxo"),
        ],
    },
    Rhythm {
        name: "Samba",
        genre: "Latin and Caribbean",
        bpm: 100,
        swing: 50.0,
        a: &[
            ("kick", "x..xX..xx..xX..x"),
            ("tambourine", "XoxxXoxxXoxxXoxx"),
            ("rim", "x.x.x..x.x.x..x."),
        ],
        b: &[
            ("kick", "x..xX..xx..xX..x"),
            ("tambourine", "XoxxXoxxXoxxXoxx"),
            ("rim", "x.x..x.x.x..x.x."),
            ("cowbell", "X.x..x.xX.x..x.x"),
        ],
        fill: &[
            ("kick", "x..xX..xx..xX..X"),
            ("snare", "xxoxxxoxXXXXrrrr"),
            ("tambourine", "XoxxXoxx........"),
        ],
    },
    // Afro
    Rhythm {
        name: "Afrobeats",
        genre: "Afro",
        bpm: 104,
        swing: 54.0,
        a: &[
            ("kick", "X.....x.X.....x."),
            ("rim", "...x..x...x..x.."),
            ("clap", "....X.......X..."),
            ("closed", "x.xxx.xxx.xxx.xx"),
            ("tambourine", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X.....x...x.X..."),
            ("rim", "...x..x...x..x.."),
            ("clap", "....X.......X..."),
            ("closed", "x.xxx.xxx.xxx.xx"),
            ("high_tom", "......x.......xx"),
        ],
        fill: &[
            ("kick", "X.....x.X......."),
            ("clap", "....X..........."),
            ("high_tom", "........xx..x..."),
            ("mid_tom", "..........x..x.."),
            ("low_tom", "..............XX"),
        ],
    },
    Rhythm {
        name: "Afro swing",
        genre: "Afro",
        bpm: 100,
        swing: 60.0,
        a: &[
            ("kick", "X.....x...x.....X.....x...x.x..."),
            ("clap", "....X.......X..."),
            ("rim", "..x..x....x..x.."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X..x..x...x.....X.....x...x..x.."),
            ("clap", "....X.......X..."),
            ("rim", "..x..x...x..x..."),
            ("closed", "x.xox.xox.xox.xo"),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X.....x...x....."),
            ("clap", "....X.....X.X.XX"),
            ("rim", "..x..x....x.x.x."),
            ("closed", "x.x.x.x........."),
        ],
    },
    Rhythm {
        name: "Amapiano",
        genre: "Afro",
        bpm: 113,
        swing: 56.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....x.......x..."),
            ("tambourine", "xxoxxxoxxxoxxxox"),
            ("rim", "..x..x....x..x.."),
            ("low_tom", "X.....x...x.X.......x...x.X...x."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....x.......x..."),
            ("tambourine", "xxoxxxoxxxoxxxox"),
            ("rim", "..x..x..x..x..x."),
            ("low_tom", "X..x..x...x.X.......X.x..x.X..x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....x.......x..."),
            ("tambourine", "xxoxxxoxxxoxxxox"),
            ("low_tom", "X..x..x.x.x.XXXX"),
        ],
    },
    Rhythm {
        name: "Afro house",
        genre: "Afro",
        bpm: 122,
        swing: 54.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....x.......x..."),
            ("closed", "..x...x...x...x."),
            ("high_tom", "..x..x.x...x.x.."),
            ("low_tom", "......x.......x."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....x.......x..."),
            ("open", "..x...x...x...x."),
            ("high_tom", "..x..x.x..xx.x.x"),
            ("low_tom", "x.....x...x...x."),
            ("cowbell", "x.x..x.x.x..x.x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("high_tom", "..x..x.xx.x....."),
            ("mid_tom", "........x..x.x.."),
            ("low_tom", "..............XX"),
        ],
    },
    Rhythm {
        name: "Afrobeat",
        genre: "Afro",
        bpm: 112,
        swing: 54.0,
        a: &[
            ("kick", "X......xX.x.....X..x...xX......."),
            ("snare", "....X..o.o..X..o"),
            ("closed", "xoxoxo.oxoxoxo.o"),
            ("open", "......x.......x."),
            ("cowbell", "x.x.xx.x.x.xx.x."),
        ],
        b: &[
            ("kick", "X......xX.x.....X..x...xX.x..x.."),
            ("snare", "....X..o.o..X..o..o.X..o.o..X.oo"),
            ("closed", "xoxoxo.oxoxoxo.o"),
            ("open", "......x.......x."),
            ("clave", "x..x..x...x.x..."),
        ],
        fill: &[
            ("kick", "X......xX.x....."),
            ("snare", "....X..o.o..XoXX"),
            ("closed", "xoxoxo.o........"),
        ],
    },
    Rhythm {
        name: "Gqom",
        genre: "Afro",
        bpm: 126,
        swing: 50.0,
        a: &[
            ("kick", "X..x..x...x..x.."),
            ("clap", "....X.......X..."),
            ("tambourine", "xoxoxoxoxoxoxoxo"),
            ("low_tom", "......x.......x."),
        ],
        b: &[
            ("kick", "X..x..x...x..x..X..x..x...x.X.x."),
            ("clap", "....X.......X..."),
            ("tambourine", "xoxoxoxoxoxoxoxo"),
            ("low_tom", "......x...x...x."),
            ("rim", "...x...x...x...x"),
        ],
        fill: &[
            ("kick", "X..x..x...x....."),
            ("clap", "....X.......XXXX"),
            ("low_tom", "......x...xxxxxx"),
        ],
    },
    // Funk, soul and disco
    Rhythm {
        name: "Funk",
        genre: "Funk, soul and disco",
        bpm: 100,
        swing: 54.0,
        a: &[
            ("kick", "X..x..x...x..x..X..x......x..x.."),
            ("snare", "....X..o.o..X..o....X..o.o..X.o."),
            ("closed", "xoxoxoxoxoxoxo.o"),
            ("open", "..............x."),
        ],
        b: &[
            ("kick", "X.x...x...x..x..X..x..x.x....x.."),
            ("snare", "....X..o.o..X.o.o.o.X..o.o..X..."),
            ("closed", "xoxoxo.oxoxoxoxo"),
            ("open", "......x........."),
        ],
        fill: &[
            ("kick", "X..x..x........."),
            ("snare", "....X..o.o......"),
            ("high_tom", "..........Xx...."),
            ("mid_tom", "............Xx.."),
            ("low_tom", "..............XX"),
            ("closed", "xoxoxoxoxo......"),
        ],
    },
    Rhythm {
        name: "Tight funk",
        genre: "Funk, soul and disco",
        bpm: 108,
        swing: 52.0,
        a: &[
            ("kick", "X.X.......X..X..X.X.......X....."),
            ("snare", "....X..o.o.oX..o....X..o.o.oX.o."),
            ("closed", "xoxoxoxoxoxoxoxo"),
        ],
        b: &[
            ("kick", "X.X.......X..X..X.X...X...X..x.."),
            ("snare", "....X..o.o.oX..o.o..X..o.o.oX..o"),
            ("closed", "xoxoxo.oxoxoxoxo"),
            ("open", "......x........."),
        ],
        fill: &[
            ("kick", "X.X.......X....."),
            ("snare", "....X..o.o.oXrXr"),
            ("closed", "xoxoxoxoxo......"),
        ],
    },
    Rhythm {
        name: "Motown",
        genre: "Funk, soul and disco",
        bpm: 120,
        swing: 52.0,
        a: &[
            ("kick", "X..x....X..x...."),
            ("snare", "x...X...x...X..."),
            ("tambourine", "x...X...x...X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X..x....X..x..x."),
            ("snare", "x...X...x...X..."),
            ("tambourine", "x.x.X.x.x.x.X.x."),
            ("ride", "x.x.x.x.x.x.x.x."),
        ],
        fill: &[
            ("kick", "X..x....X......."),
            ("snare", "x...X...xxxxXXXX"),
            ("tambourine", "x...X..........."),
        ],
    },
    Rhythm {
        name: "Northern soul",
        genre: "Funk, soul and disco",
        bpm: 116,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X.......X..."),
            ("tambourine", "X.x.X.x.X.x.X.x."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X.......X..o"),
            ("open", "..x...x...x...x."),
            ("tambourine", "X...X...X...X..."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X...x.x.xxXX"),
            ("tambourine", "X.x.X.x........."),
        ],
    },
    Rhythm {
        name: "Disco",
        genre: "Funk, soul and disco",
        bpm: 120,
        swing: 52.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X.......X..."),
            ("closed", "xo..xo..xo..xo.."),
            ("open", "..X...X...X...X."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X.......X..."),
            ("clap", "....X.......X..."),
            ("closed", "xo.oxo.oxo.oxo.o"),
            ("open", "..X...X...X...X."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X..........."),
            ("high_tom", "........X.X....."),
            ("mid_tom", "...........X.X.."),
            ("low_tom", "..............XX"),
            ("open", "..X...X........."),
        ],
    },
    Rhythm {
        name: "Nu-disco",
        genre: "Funk, soul and disco",
        bpm: 116,
        swing: 54.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("open", "..x...x...x...x."),
            ("tambourine", "xoxoxoxoxoxoxoxo"),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("snare", "....x.......x..."),
            ("open", "..x...x...x...x."),
            ("tambourine", "xoxoxoxoxoxoxoxo"),
            ("cowbell", "...x..x....x...."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("mid_tom", "........x.x.x..."),
            ("low_tom", ".............xxX"),
            ("open", "..x...x........."),
        ],
    },
    Rhythm {
        name: "Boogie",
        genre: "Funk, soul and disco",
        bpm: 112,
        swing: 56.0,
        a: &[
            ("kick", "X.....x.X..x...."),
            ("clap", "....X.......X..."),
            ("snare", "....x.......x..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.....x.X..x..x."),
            ("clap", "....X.......X..."),
            ("snare", "....x.......x..."),
            ("closed", "x.x.x.x.x.x.x..."),
            ("open", "..............x."),
        ],
        fill: &[
            ("kick", "X.....x.X......."),
            ("clap", "....X.......X.X."),
            ("snare", "....x.......xxXX"),
            ("closed", "x.x.x.x.x.x....."),
        ],
    },
    // Rock
    Rhythm {
        name: "Rock",
        genre: "Rock",
        bpm: 110,
        swing: 50.0,
        a: &[
            ("kick", "X.......x.x.....X.......x.x....."),
            ("snare", "....X.......X..."),
            ("closed", "X.x.X.x.X.x.X.x."),
        ],
        b: &[
            ("kick", "X.......x.x..x..X..x....x.x....."),
            ("snare", "....X.......X..."),
            ("ride", "X.x.X.x.X.x.X.x."),
            ("cymbal", "X..............................."),
        ],
        fill: &[
            ("kick", "X.......x......."),
            ("snare", "....X...XxXx...."),
            ("high_tom", "............Xx.."),
            ("low_tom", "..............XX"),
            ("closed", "X.x.X.x........."),
        ],
    },
    Rhythm {
        name: "Pop rock",
        genre: "Rock",
        bpm: 120,
        swing: 50.0,
        a: &[
            ("kick", "X......xX......."),
            ("snare", "....X.......X..."),
            ("closed", "Xoxoxoxoxoxoxoxo"),
            ("tambourine", "....x.......x..."),
        ],
        b: &[
            ("kick", "X......xX..x...."),
            ("snare", "....X.......X..."),
            ("open", "x.x.x.x.x.x.x.x."),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X......xX......."),
            ("snare", "....X...x.xxXXXX"),
            ("closed", "Xoxoxoxo........"),
        ],
    },
    Rhythm {
        name: "Hard rock",
        genre: "Rock",
        bpm: 128,
        swing: 50.0,
        a: &[
            ("kick", "X.X.....X.X....."),
            ("snare", "....X.......X..."),
            ("open", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.X...X.X.X...X."),
            ("snare", "....X.......X..."),
            ("ride", "X.x.X.x.X.x.X.x."),
            ("cymbal", "X..............................."),
        ],
        fill: &[
            ("kick", "X.X.....X.X....."),
            ("snare", "....X..........."),
            ("high_tom", "........xxx....."),
            ("mid_tom", "...........xx..."),
            ("low_tom", ".............xXX"),
        ],
    },
    Rhythm {
        name: "Punk",
        genre: "Rock",
        bpm: 180,
        swing: 50.0,
        a: &[
            ("kick", "X.......X.X....."),
            ("snare", "....X.......X..."),
            ("open", "X.x.X.x.X.x.X.x."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("snare", "..X...X...X...X."),
            ("ride", "X.x.X.x.X.x.X.x."),
        ],
        fill: &[("kick", "X.......X......."), ("snare", "X.X.X.X.XXXXXXXX")],
    },
    Rhythm {
        name: "Half-time rock",
        genre: "Rock",
        bpm: 76,
        swing: 50.0,
        a: &[
            ("kick", "X.........X....."),
            ("snare", "........X......."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.....x...X..x.."),
            ("snare", "........X......."),
            ("ride", "X.x.X.x.X.x.X.x."),
            ("cymbal", "X..............................."),
        ],
        fill: &[
            ("kick", "X.........X....."),
            ("snare", "........X.x....."),
            ("high_tom", "............X..."),
            ("mid_tom", ".............X.."),
            ("low_tom", "..............XX"),
        ],
    },
    Rhythm {
        name: "Half-time shuffle",
        genre: "Rock",
        bpm: 86,
        swing: 66.0,
        a: &[
            ("kick", "X......x..x.....X.........x..x.."),
            ("snare", ".o.o.o.oX..o.o.o"),
            ("closed", "x.xxx.xxx.xxx.xx"),
        ],
        b: &[
            ("kick", "X......x..x..x..X.....x...x....."),
            ("snare", ".o.o.o.oX..o.o.o.o.o.o.oX.o..o.o"),
            ("ride", "x.xxx.xxx.xxx.xx"),
        ],
        fill: &[
            ("kick", "X......x..x....."),
            ("snare", ".o.o.o.oX.xxXxXX"),
            ("closed", "x.xxx.xxx......."),
        ],
    },
    Rhythm {
        name: "Ballad",
        genre: "Rock",
        bpm: 70,
        swing: 50.0,
        a: &[
            ("kick", "X.........x....."),
            ("rim", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.......X.x....."),
            ("snare", "....X.......X..."),
            ("ride", "x.x.x.x.x.x.x.x."),
            ("cymbal", "X..............................."),
        ],
        fill: &[
            ("kick", "X.........x....."),
            ("snare", "....X...x.x....."),
            ("high_tom", "...........xx..."),
            ("mid_tom", ".............x.."),
            ("low_tom", "..............XX"),
            ("closed", "x.x.x.x........."),
        ],
    },
    Rhythm {
        name: "Four on the floor",
        genre: "Rock",
        bpm: 124,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X.......X..."),
            ("closed", "x.X.x.X.x.X.x.X."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X.......X..."),
            ("closed", "x...x...x...x..."),
            ("open", "..X...X...X...X."),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X...X.XXX.XX"),
            ("closed", "x.X.x.X........."),
        ],
    },
    // Synthwave and 80s
    Rhythm {
        name: "Synthwave",
        genre: "Synthwave and 80s",
        bpm: 100,
        swing: 50.0,
        a: &[
            ("kick", "X.......X..x...."),
            ("snare", "....X.......X..."),
            ("clap", "....x.......x..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.......X..x....X..x....X..x...."),
            ("snare", "....X.......X..."),
            ("clap", "....x.......x..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
        ],
        fill: &[
            ("kick", "X.......X......."),
            ("snare", "....X..........."),
            ("high_tom", "........X.X....."),
            ("mid_tom", "...........X.X.."),
            ("low_tom", "..............XX"),
        ],
    },
    Rhythm {
        name: "Outrun",
        genre: "Synthwave and 80s",
        bpm: 118,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X.......X..."),
            ("clap", "....X.......X..."),
            ("closed", "x...x...x...x..."),
            ("open", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("snare", "....X..........."),
            ("high_tom", "........XX......"),
            ("mid_tom", "..........XX...."),
            ("low_tom", "............XXXX"),
        ],
    },
    Rhythm {
        name: "Darkwave",
        genre: "Synthwave and 80s",
        bpm: 128,
        swing: 50.0,
        a: &[
            ("kick", "X.....X.X......."),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.....X.X.....X."),
            ("snare", "....X.......X..."),
            ("open", "x.x.x.x.x.x.x.x."),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X.....X.X......."),
            ("snare", "....X.......X..."),
            ("low_tom", "........X.X.X.XX"),
        ],
    },
    Rhythm {
        name: "80s pop",
        genre: "Synthwave and 80s",
        bpm: 116,
        swing: 50.0,
        a: &[
            ("kick", "X......xX.x....."),
            ("snare", "....X.......X..."),
            ("clap", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("tambourine", "....x.......x..."),
        ],
        b: &[
            ("kick", "X......xX.x..x.."),
            ("snare", "....X.......X..."),
            ("clap", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxo.o"),
            ("open", "..............x."),
            ("cowbell", "..x...x...x...x."),
        ],
        fill: &[
            ("kick", "X......xX......."),
            ("snare", "....X..........."),
            ("high_tom", "........XX.X...."),
            ("mid_tom", "............XX.."),
            ("low_tom", "..............XX"),
            ("closed", "xoxoxoxo........"),
        ],
    },
    Rhythm {
        name: "Italo disco",
        genre: "Synthwave and 80s",
        bpm: 122,
        swing: 50.0,
        a: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("closed", "xo..xo..xo..xo.."),
            ("open", "..x...x...x...x."),
        ],
        b: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("open", "..x...x...x...x."),
            ("tambourine", "xoxoxoxoxoxoxoxo"),
            ("high_tom", "......x.......x."),
        ],
        fill: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....X.......X..."),
            ("high_tom", "........X.X.X.X."),
            ("low_tom", ".........X.X.X.X"),
        ],
    },
    // Lo-fi and chill
    Rhythm {
        name: "Lo-fi",
        genre: "Lo-fi and chill",
        bpm: 80,
        swing: 62.0,
        a: &[
            ("kick", "X......x.x......X.......x.x....."),
            ("snare", "....X.......X..o....X.......X..."),
            ("rim", "..............o...............o."),
            ("closed", "x.o.x.o.x.o.x.o.x.o.x.o.x.o.x.xo"),
        ],
        b: &[
            ("kick", "X......x.x..x...X.....x.x.x....."),
            ("snare", "....X.......X..o....X..o....X..."),
            ("rim", "..o.......o....."),
            ("closed", "x.o.x.o.x.o.x..."),
            ("open", "..............x."),
        ],
        fill: &[
            ("kick", "X......x.x......"),
            ("snare", "....X......oX.oX"),
            ("closed", "x.o.x.o.x.o....."),
        ],
    },
    Rhythm {
        name: "Chillhop",
        genre: "Lo-fi and chill",
        bpm: 86,
        swing: 58.0,
        a: &[
            ("kick", "X.......x.x.....X.x.....x......."),
            ("snare", "....X.......X..."),
            ("closed", "x.x.x.xox.x.x.xo"),
        ],
        b: &[
            ("kick", "X.......x.x..x..X.x.....x..x...."),
            ("snare", "....X.......X..o....X..o....X..."),
            ("closed", "x.x.x..ox.x.x.xo"),
            ("open", "......x........."),
        ],
        fill: &[
            ("kick", "X.......x.x....."),
            ("snare", "....X.....o.X.oX"),
            ("closed", "x.x.x.xox.x....."),
        ],
    },
    Rhythm {
        name: "Jazzy lo-fi",
        genre: "Lo-fi and chill",
        bpm: 84,
        swing: 64.0,
        a: &[
            ("kick", "X.........x.....X......x........"),
            ("rim", "....X.......X..."),
            ("ride", "x...x..xx...x..x"),
            ("closed", "....x.......x..."),
        ],
        b: &[
            ("kick", "X.........x..x..X.....x...x....."),
            ("snare", "....o..o....X.......o..o....X..o"),
            ("ride", "x...x..xx...x..x"),
            ("closed", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X.........x....."),
            ("snare", "....o..o..o.XoXo"),
            ("ride", "x...x..x........"),
        ],
    },
    Rhythm {
        name: "Trip-hop",
        genre: "Lo-fi and chill",
        bpm: 82,
        swing: 56.0,
        a: &[
            ("kick", "X.........X..x..X.......X......."),
            ("snare", "....X.......X..."),
            ("rim", "..o.......o....."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
        b: &[
            ("kick", "X.........X..x..X..x....X..x...."),
            ("snare", "....X.......X.......X.......X.o."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X.........X....."),
            ("snare", "....X.......XrXr"),
            ("closed", "xoxoxoxoxoxo...."),
        ],
    },
    Rhythm {
        name: "Downtempo",
        genre: "Lo-fi and chill",
        bpm: 95,
        swing: 54.0,
        a: &[
            ("kick", "X.....x...x.....X.....x.x......."),
            ("rim", "....X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x."),
            ("clave", "x..x..x...x.x..."),
        ],
        b: &[
            ("kick", "X.....x...x..x..X.....x.x..x...."),
            ("snare", "....X.......X..."),
            ("closed", "xoxoxoxoxoxoxoxo"),
            ("clave", "x..x..x...x.x..."),
            ("high_tom", "..x.....x.x....."),
        ],
        fill: &[
            ("kick", "X.....x...x....."),
            ("snare", "....X.....x.XxXX"),
            ("high_tom", "........x.x.x..."),
        ],
    },
    Rhythm {
        name: "Chillwave",
        genre: "Lo-fi and chill",
        bpm: 104,
        swing: 50.0,
        a: &[
            ("kick", "X.......X...x..."),
            ("clap", "....X.......X..."),
            ("closed", "..x...x...x...x."),
            ("tambourine", "xoxoxoxoxoxoxoxo"),
        ],
        b: &[
            ("kick", "X.......X...x..."),
            ("clap", "....X.......X..."),
            ("snare", "....x.......x..."),
            ("closed", "x.x.x.x.x.x.x.x."),
            ("tambourine", "....x.......x..."),
        ],
        fill: &[
            ("kick", "X.......X...x..."),
            ("clap", "....X.......X..."),
            ("snare", "........x.x.xxxx"),
            ("closed", "..x...x........."),
        ],
    },
];

/// The steps of a variation, by pad.
type Lines = &'static [(&'static str, &'static str)];

/// Velocities of the steps written: accented, normal, ghost.
const ACCENT: u8 = 120;
const NORMAL: u8 = 95;
const GHOST: u8 = 55;

/// What each bar of a pattern plays, in a cycle of four bars: the groove (its two bars when
/// it has two), the variation, then the fill; and the variation each bar is edited as.
fn plan(rhythm: &Rhythm, bars: usize) -> Vec<(Lines, usize, u8)> {
    use crate::drum_sequencer::{A, B, FILL};
    let length = |lines: &[(&str, &str)]| {
        lines
            .iter()
            .map(|l| l.1.len().div_ceil(16))
            .max()
            .unwrap_or(1)
    };
    let (la, lb) = (length(rhythm.a), length(rhythm.b));
    let has_b = !rhythm.b.is_empty();
    let has_fill = !rhythm.fill.is_empty();
    // Each distinct bar its own letter, in the order they come; the fill is the fill.
    let mut seen: Vec<(u8, usize)> = Vec::new();
    let mut letters = [A, B].into_iter().chain(3..=26);
    (0..bars)
        .map(|bar| {
            let (which, part) = match bar % 4 {
                0 => (0, 0),
                1 => (0, 1 % la),
                2 if has_b => (1, 0),
                2 => (0, 0),
                _ if has_fill => (2, 0),
                _ if has_b => (1, 1 % lb),
                _ => (0, 1 % la),
            };
            let lines = [rhythm.a, rhythm.b, rhythm.fill][which];
            let variation = if which == 2 {
                FILL
            } else if let Some(&(v, _)) = seen.iter().find(|s| s.1 == which * 2 + part) {
                v
            } else {
                let v = letters.next().unwrap_or(A);
                seen.push((v, which * 2 + part));
                v
            };
            (lines, part, variation)
        })
        .collect()
}

/// The notes of `bars` bars of `rhythm` from a bar start, a sixteenth being `step` samples,
/// swung by the rhythm's swing; and the variation of each bar.
pub(crate) fn rhythm_notes(rhythm: &Rhythm, bars: usize, step: f64) -> (Vec<Note>, Vec<u8>) {
    let delay = step * f64::from((rhythm.swing / 100.0 * 2.0 - 1.0).clamp(0.0, 0.5));
    let length = (step / 2.0) as u64;
    let mut notes = Vec::new();
    let plan = plan(rhythm, bars);
    for (bar, (lines, part, _)) in plan.iter().enumerate() {
        for (voice, steps) in lines.iter() {
            let Some(key) = VOICES.iter().find(|v| v.0 == *voice).map(|v| v.2) else {
                continue;
            };
            let steps: Vec<char> = steps.chars().collect();
            for k in 0..16 {
                let index = (part * 16 + k) % steps.len().max(1);
                let at = (bar * 16 + k) as f64 * step + if k % 2 == 1 { delay } else { 0.0 };
                let mut hit = |start: f64, velocity: u8| {
                    if start >= 0.0 {
                        notes.push(Note {
                            start: start as u64,
                            length,
                            key,
                            velocity,
                            channel: 9,
                            ..Default::default()
                        });
                    }
                };
                match steps[index] {
                    'X' => hit(at, ACCENT),
                    'x' => hit(at, NORMAL),
                    'o' => hit(at, GHOST),
                    'f' => {
                        hit(at - 0.15 * step, GHOST);
                        hit(at, NORMAL);
                    }
                    'r' => {
                        hit(at, 85);
                        hit(at + step / 2.0, 70);
                    }
                    't' => {
                        for (j, velocity) in [85, 65, 75].into_iter().enumerate() {
                            hit(at + step * j as f64 / 3.0, velocity);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    notes.sort_by_key(|n| (n.start, n.key));
    (notes, plan.into_iter().map(|p| p.2).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PADS: [&str; 16] = [
        "kick",
        "rim",
        "snare",
        "clap",
        "closed",
        "open",
        "low_tom",
        "mid_tom",
        "high_tom",
        "cymbal",
        "cowbell",
        "clave",
        "kick2",
        "snare2",
        "ride",
        "tambourine",
    ];

    fn known_steps(steps: &str) -> bool {
        steps.chars().all(|c| "Xxofrt.".contains(c))
    }

    #[test]
    fn every_pad_used_exists() {
        for rhythm in RHYTHMS {
            for (pad, _) in rhythm.a.iter().chain(rhythm.b).chain(rhythm.fill) {
                assert!(PADS.contains(pad), "{}: no pad {pad}", rhythm.name);
            }
        }
    }

    #[test]
    fn grooves_are_one_or_two_bars_of_known_steps() {
        for rhythm in RHYTHMS {
            for (pad, steps) in rhythm.a.iter().chain(rhythm.b) {
                assert!(matches!(steps.len(), 16 | 32), "{} {pad}", rhythm.name);
                assert!(known_steps(steps), "{} {pad}", rhythm.name);
            }
        }
    }

    #[test]
    fn fills_are_one_bar_of_known_steps() {
        for rhythm in RHYTHMS {
            for (pad, steps) in rhythm.fill {
                assert_eq!(steps.len(), 16, "{} {pad}", rhythm.name);
                assert!(known_steps(steps), "{} {pad}", rhythm.name);
            }
        }
    }

    #[test]
    fn every_genre_is_listed_and_has_at_least_four_rhythms() {
        for rhythm in RHYTHMS {
            assert!(GENRES.contains(&rhythm.genre), "{}", rhythm.name);
        }
        for genre in GENRES {
            let count = RHYTHMS.iter().filter(|r| r.genre == genre).count();
            assert!(count >= 4, "{genre}: {count}");
        }
    }

    #[test]
    fn names_are_unique_within_a_genre() {
        for (i, rhythm) in RHYTHMS.iter().enumerate() {
            let twin = RHYTHMS[i + 1..]
                .iter()
                .any(|r| r.genre == rhythm.genre && r.name == rhythm.name);
            assert!(!twin, "{} twice in {}", rhythm.name, rhythm.genre);
        }
    }

    #[test]
    fn every_rhythm_has_a_beat_and_a_fill() {
        for rhythm in RHYTHMS {
            let beat = ["kick", "kick2", "snare", "snare2", "clap", "rim"];
            assert!(
                rhythm.a.iter().any(|(pad, _)| beat.contains(pad)),
                "{}",
                rhythm.name
            );
            assert!(!rhythm.fill.is_empty(), "{}", rhythm.name);
        }
    }

    #[test]
    fn a_rhythm_plays_its_groove_its_variation_and_its_fill_over_four_bars() {
        use crate::drum_sequencer::{A, B, FILL};
        let rhythm = Rhythm {
            name: "Test",
            genre: "House",
            bpm: 120,
            swing: 60.0,
            a: &[("kick", "X..............."), ("closed", ".x..............")],
            b: &[("kick", "X.......X.......")],
            fill: &[("snare", "f..t............")],
        };
        let (notes, chain) = rhythm_notes(&rhythm, 8, 250.0);
        assert_eq!(chain, [A, A, B, FILL, A, A, B, FILL]);
        let kicks: Vec<u64> = notes
            .iter()
            .filter(|n| n.key == 36)
            .map(|n| n.start)
            .collect();
        assert_eq!(&kicks[..4], [0, 4000, 8000, 10_000]);
        let hat = notes.iter().find(|n| n.key == 42).unwrap();
        assert_eq!(hat.start, 250 + 50, "the off sixteenth swings");
        let snares: Vec<u64> = notes
            .iter()
            .filter(|n| n.key == 38 && n.start < 16_000)
            .map(|n| n.start)
            .collect();
        assert_eq!(snares.len(), 5, "a flam's two hits, a triplet's three");
        assert_eq!(snares[0], 11_962, "the flam's grace note just before");
    }

    #[test]
    fn a_two_bar_groove_takes_two_letters() {
        use crate::drum_sequencer::{A, B, FILL};
        let rhythm = RHYTHMS
            .iter()
            .find(|r| r.a.iter().any(|l| l.1.len() == 32) && !r.b.is_empty() && !r.fill.is_empty())
            .unwrap();
        let (_, chain) = rhythm_notes(rhythm, 4, 250.0);
        assert_eq!(&chain[..2], [A, B]);
        assert_ne!(chain[2], A);
        assert_eq!(chain[3], FILL);
    }
}
