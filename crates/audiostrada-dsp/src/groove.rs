//! Groove: moves the hits of some audio onto those of a reference (a bass onto the kick, a
//! rhythm guitar onto the snare) by stretching the audio between them. Every channel of a
//! group is cut and stretched at the very same places, chosen on their sum, so the phase
//! between them is kept (a bass DI and its amp, the microphones of a kit).

/// Where each target hit goes: `(source, destination)` in samples, sorted, starting with
/// `(0, 0)` and ending with `(length, length)`. A target hit with a reference hit within
/// `tolerance` moves toward it by `strength` (0 to 1). A hit less than `gap` after another
/// belongs to the same note and stays with it. Where the reference hits are close together,
/// the tolerance shrinks to 40 % of their spacing, so a hit never jumps to the wrong one.
/// Hits closer than `gap` after moving,
/// or that would stretch the audio between them beyond half or twice its length, are left
/// where they are (the one moving most gives way).
pub fn anchors(
    reference: &[usize],
    target: &[usize],
    tolerance: usize,
    strength: f64,
    gap: usize,
    length: usize,
) -> Vec<(usize, usize)> {
    let mut reference = reference.to_vec();
    reference.sort_unstable();
    let notes = notes(target, gap);
    // Nearest reference hit of each target hit, each reference hit taken once, nearest first.
    let mut pairs: Vec<(usize, usize)> = notes
        .iter()
        .filter(|&&t| t > 0 && t < length)
        .filter_map(|&t| {
            let i = reference.partition_point(|&r| r < t);
            let nearest = [i.checked_sub(1), Some(i)]
                .into_iter()
                .flatten()
                .filter(|&i| i < reference.len())
                .min_by_key(|&i| reference[i].abs_diff(t))?;
            let r = reference[nearest];
            let spacing = [nearest.checked_sub(1), Some(nearest + 1)]
                .into_iter()
                .flatten()
                .filter_map(|j| reference.get(j))
                .map(|&n| n.abs_diff(r))
                .min()
                .unwrap_or(usize::MAX);
            let reach = tolerance.min(spacing.saturating_mul(2) / 5);
            (r.abs_diff(t) <= reach).then_some((t, r))
        })
        .collect();
    pairs.sort_by_key(|&(t, r)| t.abs_diff(r));
    let mut used = std::collections::HashSet::new();
    pairs.retain(|&(_, r)| used.insert(r));
    pairs.sort_unstable();
    let strength = strength.clamp(0.0, 1.0);
    let moved = |(t, r): (usize, usize)| {
        let to = t as f64 + (r as f64 - t as f64) * strength;
        (t, (to.round().max(0.0) as usize).min(length))
    };
    let fits = |a: (usize, usize), b: (usize, usize)| {
        let (source, destination) = (b.0.saturating_sub(a.0), b.1.saturating_sub(a.1));
        b.0 >= a.0 + gap
            && b.1 >= a.1 + gap
            && destination * 2 >= source
            && destination <= source * 2
    };
    let distance = |a: (usize, usize)| a.0.abs_diff(a.1);
    let mut kept: Vec<(usize, usize)> = vec![(0, 0)];
    for anchor in pairs.into_iter().map(moved).chain([(length, length)]) {
        let last_one = anchor == (length, length);
        loop {
            let previous = *kept.last().unwrap_or(&(0, 0));
            if fits(previous, anchor) {
                kept.push(anchor);
                break;
            }
            // The start never gives way; the end never does either.
            if kept.len() > 1 && (last_one || distance(previous) > distance(anchor)) {
                kept.pop();
            } else {
                break;
            }
        }
    }
    if kept.last() != Some(&(length, length)) {
        kept.push((length, length));
    }
    kept
}

/// The first hit of each note: a hit less than `gap` after another belongs to its note.
pub fn notes(hits: &[usize], gap: usize) -> Vec<usize> {
    let mut notes = hits.to_vec();
    notes.sort_unstable();
    let mut last = None;
    notes.retain(|&t| {
        let first = last.is_none_or(|l| t >= l + gap);
        last = Some(t);
        first
    });
    notes
}

/// Stretches `channels` so that each anchor's source sample lands on its destination (see
/// [`anchors`]); the result lasts as long as the last destination. Each anchored hit is copied
/// whole, from a few milliseconds before it; in between, the audio is stretched by WSOLA:
/// windows of 60 ms laid 30 ms apart, each taken where it best continues the previous one
/// within 13 ms of where the stretch puts it (half a period of the lowest bass notes, so a
/// seamless place is always in reach). The time left over is made up in the short fade before
/// the next anchored hit, which that hit masks. Every channel is cut at the same places.
///
/// No jump skips, repeats or blends the start of one of `notes` (the attacks of the notes in
/// between, which go along): there the windows just go on, no longer than the gap between two
/// notes so that the stretch can jump between them. A jump into louder or quieter audio of a decaying
/// note is brought to the level the note had reached, until the next attack.
pub fn warp(
    channels: &[&[f32]],
    anchors: &[(usize, usize)],
    notes: &[usize],
    rate: u32,
) -> Vec<Vec<f32>> {
    let seconds = |s: f32| ((rate as f32 * s) as usize).max(1);
    let (hop, reach, fade) = (seconds(0.03).max(16), seconds(0.013), seconds(0.003));
    // The loud start of a note, from its attack.
    let guard = seconds(0.02);
    let mut notes = notes.to_vec();
    notes.sort_unstable();
    let attack_in = |from: usize, to: usize| {
        let i = notes.partition_point(|&n| n + guard <= from);
        notes.get(i).is_some_and(|&n| n < to)
    };
    let length = anchors.last().map_or(0, |a| a.1);
    let frames = channels.iter().map(|c| c.len()).max().unwrap_or(0);
    let guide: Vec<f32> = (0..frames)
        .map(|i| channels.iter().filter_map(|c| c.get(i)).sum())
        .collect();
    let coarse: Vec<f32> = guide
        .chunks(DECIMATION)
        .map(|c| c.iter().sum::<f32>() / DECIMATION as f32)
        .collect();
    let mut out = vec![vec![0.0f32; length]; channels.len()];
    for (i, pair) in anchors.windows(2).enumerate() {
        let ((a0, b0), (a1, b1)) = (pair[0], pair[1]);
        if b1 <= b0 || a1 <= a0 {
            continue;
        }
        // From a little before the hit, fading in while the previous part fades out.
        let lead = if i > 0 { fade.min(a0).min(b0) } else { 0 };
        let tail = if i + 2 < anchors.len() { fade } else { 0 };
        let speed = (a1 - a0) as f64 / (b1 - b0) as f64;
        let (from, to) = (a0 - lead, b0 - lead);
        // Windows short enough that the first one ends before the next hit, and that the
        // stretch finds room to jump between two notes.
        let inside = notes.partition_point(|&n| n <= a0)..notes.partition_point(|&n| n < a1);
        let closest = notes[inside]
            .iter()
            .chain([&a1])
            .scan(a0, |last, &n| {
                let gap = n - *last;
                *last = n;
                Some(gap)
            })
            .min()
            .unwrap_or(a1 - a0);
        let hop = hop.min((a1 - from) / 2).min(closest).max(16);
        let search = Search {
            guide: &guide,
            coarse: &coarse,
            hop,
            reach,
        };
        let span = (b1 - to).min(length - to);
        let gain = |u: usize| {
            let rise = if u < lead {
                (u as f32 + 0.5) / lead as f32
            } else {
                1.0
            };
            let fall = match (span - u).checked_sub(1) {
                Some(left) if tail > 0 && left < tail => (left as f32 + 0.5) / tail as f32,
                _ => 1.0,
            };
            rise * fall
        };
        let mut previous: Option<usize> = None;
        let mut level = 1.0f32;
        let mut u = 0;
        while u < span {
            // Never the start of the hit again; a window reaching the next hit only where that
            // lands after this part.
            let limits = (
                (a0 + guard).min(a1),
                a1.saturating_sub(2 * hop)
                    .max((a1 + u).saturating_sub(span)),
            );
            let source = match previous {
                None => from,
                Some(p) => {
                    let natural = p + hop;
                    let avoid =
                        |s: usize| s != natural && attack_in(s.min(natural), s.max(natural) + hop);
                    let best = search.best(from as f64 + u as f64 * speed, natural, limits, avoid);
                    // Near the end, a window ending right on the next hit, when it fits about as
                    // well, makes the fade before that hit mix the same samples.
                    let exact = (a1 + u).saturating_sub(span);
                    let near_end = tail > 0 && u + 2 * hop >= span;
                    if near_end
                        && exact >= limits.0
                        && exact != best
                        && !avoid(exact)
                        && search.similarity(exact, natural)
                            >= 0.85 * search.similarity(best, natural)
                    {
                        exact
                    } else {
                        best
                    }
                }
            };
            if let Some(p) = previous {
                let natural = p + hop;
                if attack_in(source, source + 2 * hop) {
                    level = 1.0;
                } else if source != natural {
                    let energy = |at: usize| {
                        guide[at.min(guide.len())..(at + hop).min(guide.len())]
                            .iter()
                            .map(|v| v * v)
                            .sum::<f32>()
                            + 1e-9
                    };
                    level = (level * (energy(natural) / energy(source)).sqrt()).clamp(0.5, 2.0);
                }
            }
            for j in 0..(2 * hop).min(span - u) {
                // The first window holds the hit whole, the others overlap by half.
                let w = if previous.is_none() && j < hop {
                    1.0
                } else {
                    let x = std::f32::consts::PI * j as f32 / hop as f32;
                    0.5 - 0.5 * x.cos()
                };
                let w = w * gain(u + j) * level;
                let at = to + u + j;
                for (o, c) in out.iter_mut().zip(channels) {
                    o[at] += w * c.get(source + j).copied().unwrap_or(0.0);
                }
            }
            previous = Some(source);
            u += hop;
        }
    }
    out
}

const DECIMATION: usize = 4;

/// Where to take the next WSOLA window: the most similar to the natural continuation.
struct Search<'a> {
    guide: &'a [f32],
    coarse: &'a [f32],
    hop: usize,
    reach: usize,
}

impl Search<'_> {
    /// Similarity of `x[a..a + n]` with `x[b..b + n]`, normalized by the energy of the first.
    fn score(x: &[f32], a: usize, b: usize, n: usize) -> f32 {
        let (mut dot, mut energy) = (0.0f32, 1e-9f32);
        for j in 0..n {
            let (u, v) = (
                x.get(a + j).copied().unwrap_or(0.0),
                x.get(b + j).copied().unwrap_or(0.0),
            );
            dot += u * v;
            energy += u * u;
        }
        dot / energy.sqrt()
    }

    /// How much the window at `at` looks like the one at `natural`.
    fn similarity(&self, at: usize, natural: usize) -> f32 {
        Self::score(self.guide, at, natural, self.hop)
    }

    /// The start of the next window, between `limits` when they leave room, where `avoid`
    /// allows; when it allows none, the window just goes on.
    fn best(
        &self,
        nominal: f64,
        natural: usize,
        limits: (usize, usize),
        avoid: impl Fn(usize) -> bool,
    ) -> usize {
        let nominal = nominal.max(0.0);
        let center = nominal.round() as usize;
        if limits.0 > limits.1 {
            return natural.min(limits.1);
        }
        if (nominal - natural as f64).abs() < 0.5 {
            return center;
        }
        let low = center.saturating_sub(self.reach).max(limits.0);
        let high = (center + self.reach).min(limits.1);
        if low > high {
            return center.clamp(limits.0, limits.1);
        }
        let d = DECIMATION;
        let n = self.hop / d;
        let fallback = natural.clamp(limits.0, limits.1);
        let mut best = (fallback, f32::MIN);
        for c in low / d..=high / d {
            if avoid(c * d) {
                continue;
            }
            let score = Self::score(self.coarse, c, natural / d, n);
            if score > best.1 {
                best = (c * d, score);
            }
        }
        let rough = best.0;
        let mut best = (fallback, f32::MIN);
        for s in rough.saturating_sub(d).max(low)..=(rough + d).min(high) {
            if avoid(s) {
                continue;
            }
            let score = Self::score(self.guide, s, natural, self.hop);
            if score > best.1 {
                best = (s, score);
            }
        }
        best.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// Plucked notes: a decaying tone with a sharp start at each of `hits`.
    fn plucks(hits: &[usize], length: usize, delay: usize, gain: f32) -> Vec<f32> {
        let mut x = vec![0.0f32; length];
        for &h in hits {
            for i in 0..length.saturating_sub(h + delay) {
                let t = i as f32 / RATE as f32;
                x[h + delay + i] += gain
                    * (-t * 12.0).exp()
                    * ((t * 110.0 * std::f32::consts::TAU).sin()
                        + 0.4 * (t * 330.0 * std::f32::consts::TAU).sin());
            }
        }
        x
    }

    #[test]
    fn hits_move_toward_the_nearest_reference_hit() {
        let ms = |m: usize| m * 48;
        let reference = [ms(500), ms(1000), ms(1500), ms(2000)];
        // Late by 20 ms, early by 30 ms, too far (90 ms), and one fighting for 2000 ms.
        let target = [ms(520), ms(970), ms(1590), ms(1990), ms(2030)];
        let a = anchors(&reference, &target, ms(50), 1.0, ms(30), ms(3000));
        assert_eq!(
            a,
            [
                (0, 0),
                (ms(520), ms(500)),
                (ms(970), ms(1000)),
                (ms(1990), ms(2000)),
                (ms(3000), ms(3000))
            ]
        );
        let half = anchors(&reference, &target[..1], ms(50), 0.5, ms(30), ms(3000));
        assert_eq!(half[1], (ms(520), ms(510)));
        // Two hits 40 ms apart pulled 50 ms apart in opposite ways: the bigger move goes.
        let crossed = anchors(
            &[ms(450), ms(600)],
            &[ms(500), ms(540)],
            ms(60),
            1.0,
            ms(30),
            ms(1000),
        );
        assert_eq!(crossed, [(0, 0), (ms(500), ms(450)), (ms(1000), ms(1000))]);
        // A buzz 18 ms after the attack is the same note: the attack follows the kick.
        let buzz = anchors(
            &[ms(500)],
            &[ms(475), ms(493)],
            ms(50),
            1.0,
            ms(30),
            ms(1000),
        );
        assert_eq!(buzz, [(0, 0), (ms(475), ms(500)), (ms(1000), ms(1000))]);
        assert_eq!(
            notes(&[ms(493), ms(475), ms(750)], ms(30)),
            [ms(475), ms(750)]
        );
    }

    #[test]
    fn fast_notes_between_the_anchors_are_neither_doubled_nor_lost() {
        // Sixteenths every 90 ms; one in four follows a reference hit, the rest go along.
        let hits: Vec<usize> = (1..40).map(|i| i * 4320).collect();
        let length = 41 * 4320;
        let x = plucks(&hits, length, 0, 0.3);
        let mut anchors = vec![(0, 0)];
        for (k, &h) in hits.iter().enumerate().step_by(4) {
            // Alternately 20 ms late and 15 ms early.
            let to = if k % 8 == 0 { h + 960 } else { h - 720 };
            anchors.push((h, to));
        }
        anchors.push((length, length));
        let out = warp(&[&x], &anchors, &hits, RATE);
        let before = crate::analysis::onsets(&[&x], RATE, 0.5).len();
        let found = crate::analysis::onsets(&[&out[0]], RATE, 0.5);
        assert_eq!(before, hits.len());
        assert_eq!(found.len(), hits.len(), "{found:?}");
        // The moved hits are copied whole where they go.
        for &(h, to) in &anchors[1..anchors.len() - 1] {
            let error = (0..480)
                .map(|i| (out[0][to + i] - x[h + i]).abs())
                .fold(0.0, f32::max);
            assert!(error < 1e-4, "the hit at {h}, at {to}: {error}");
        }
    }

    #[test]
    fn without_moves_the_audio_is_unchanged() {
        let x = plucks(&[1000, 30_000], 60_000, 0, 0.5);
        let out = warp(&[&x], &[(0, 0), (60_000, 60_000)], &[], RATE);
        let error = x
            .iter()
            .zip(&out[0])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(error < 1e-5, "{error}");
        // A hit that stays put through an anchor is unchanged too.
        let out = warp(
            &[&x],
            &[(0, 0), (30_000, 30_000), (60_000, 60_000)],
            &[],
            RATE,
        );
        let error = x
            .iter()
            .zip(&out[0])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(error < 1e-4, "{error}");
    }

    #[test]
    fn hits_land_on_their_destination_and_the_phase_is_kept() {
        let hits = [9_600, 33_600, 57_600, 81_600];
        let moved = [8_640, 34_560, 56_160, 82_800];
        let length = 110_000;
        // A DI and its amp: the same notes, quieter and 7 samples later.
        let di = plucks(&hits, length, 0, 0.5);
        let amp = plucks(&hits, length, 7, 0.25);
        let mut anchors = vec![(0, 0)];
        anchors.extend(hits.iter().copied().zip(moved));
        anchors.push((length, length));
        let out = warp(&[&di, &amp], &anchors, &hits, RATE);
        assert_eq!(out[0].len(), length);
        let found = crate::analysis::onsets(&[&out[0]], RATE, 0.5);
        assert_eq!(found.len(), moved.len(), "no doubled attack: {found:?}");
        for m in moved {
            let nearest = found.iter().map(|&f| f.abs_diff(m)).min().unwrap();
            assert!(nearest < 48, "a hit for {m}: {found:?}");
        }
        // Each moved hit is copied whole: the attack matches the original one.
        for (&h, &m) in hits.iter().zip(&moved) {
            let error = (0..480)
                .map(|i| (out[0][m + i] - di[h + i]).abs())
                .fold(0.0, f32::max);
            assert!(error < 1e-4, "attack at {m}: {error}");
        }
        // Both tracks were cut at the same places: the amp still follows the DI by 7 samples.
        let correlation = |lag: usize| {
            let (a, b) = (&out[0][..length - lag], &out[1][lag..]);
            let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>().sqrt();
            dot / (energy(a) * energy(b))
        };
        let best = (0..20).max_by(|&a, &b| correlation(a).total_cmp(&correlation(b)));
        assert_eq!(best, Some(7));
        assert!(correlation(7) > 0.999, "{}", correlation(7));
        let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
        let level = rms(&out[0]) / rms(&di);
        assert!((level - 1.0).abs() < 0.1, "{level}");
    }
}
