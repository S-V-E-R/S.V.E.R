//! Fortnite's players-left counter, read from a recorded segment for MAGNet endgame moments
//! (docs/MAGNET.md "What counts as a moment"). CLIP can't read numbers, so the counter's digits are
//! matched against 10 digit templates instead.
//!
//! The counter sits right-aligned under the kills count, left of the minimap. `COUNTER` is the area
//! it can occupy at any HUD scale, as fractions of the game area; it is cut at the stream's own
//! resolution and scaled to 44×18 (its size at 720p), where digits are about 6×8 pixels on 7 or 8
//! pixel slots. The reader tries each slot position and size, matches every slot against the
//! templates (normalized cross-correlation, so any background and brightness works) and keeps the
//! digit count that fits best, refusing one that leaves a digit-like shape just outside it.
//!
//! The templates (`fortnite_digits.bin`, 10 × 80 little-endian f32, each z-normalized, row-major
//! 10 rows of 8) are averages of hand-labeled counter digits from 7 hours of Fortnite footage; on
//! frames they weren't averaged from, 174 of 181 read exactly, and the rest mostly drop a digit
//! (40 read as 4). `Endgame::read` turns those reads into moments that a lone misread can't fake.

/// Where the counter can be, as `[x0, y0, x1, y1]` fractions of the game area.
pub const COUNTER: [f32; 4] = [1060.0 / 1280.0, 40.0 / 720.0, 1104.0 / 1280.0, 58.0 / 720.0];
pub const WIDTH: u32 = 44;
pub const HEIGHT: u32 = 18;
/// Players left at or below which a stream is in its endgame.
pub const ENDGAME: u32 = 10;

const TW: usize = 8;
const TH: usize = 10;
const MIN_MATCH: f32 = 0.7;
static DIGITS: &[u8] = include_bytes!("fortnite_digits.bin");

fn templates() -> &'static [[f32; TW * TH]; 10] {
    static PARSED: std::sync::LazyLock<[[f32; TW * TH]; 10]> = std::sync::LazyLock::new(|| {
        let mut out = [[0.0; TW * TH]; 10];
        for (i, v) in DIGITS.as_chunks::<4>().0.iter().enumerate() {
            out[i / (TW * TH)][i % (TW * TH)] = f32::from_le_bytes(*v);
        }
        out
    });
    &PARSED
}

/// Grayscale crop, row-major.
struct Gray<'a> {
    px: &'a [f32],
    w: usize,
    h: usize,
}
impl Gray<'_> {
    /// The `w`×`h` patch at (x, y), resampled to the template size; None when it leaves the crop.
    fn patch(&self, x: i32, y: i32, w: usize, h: usize) -> Option<[f32; TW * TH]> {
        if x < 0 || y < 0 || x as usize + w > self.w || y as usize + h > self.h {
            return None;
        }
        let (x, y) = (x as usize, y as usize);
        let at = |c: usize, r: usize| self.px[(y + r) * self.w + x + c];
        let mut out = [0.0; TW * TH];
        for r in 0..TH {
            // Bilinear, pixel centres aligned (an exact copy when the patch is template-sized).
            let sy = ((r as f32 + 0.5) * h as f32 / TH as f32 - 0.5).clamp(0.0, (h - 1) as f32);
            let (y0, fy) = (sy as usize, sy.fract());
            let y1 = (y0 + 1).min(h - 1);
            for c in 0..TW {
                let sx = ((c as f32 + 0.5) * w as f32 / TW as f32 - 0.5).clamp(0.0, (w - 1) as f32);
                let (x0, fx) = (sx as usize, sx.fract());
                let x1 = (x0 + 1).min(w - 1);
                let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
                let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
                out[r * TW + c] = top * (1.0 - fy) + bottom * fy;
            }
        }
        Some(out)
    }
}

fn std_dev(p: &[f32]) -> f32 {
    let mean = p.iter().sum::<f32>() / p.len() as f32;
    (p.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / p.len() as f32).sqrt()
}
/// The closest digit and how well it matches (-1 to 1).
fn best_digit(p: &[f32; TW * TH]) -> (u32, f32) {
    let mean = p.iter().sum::<f32>() / p.len() as f32;
    let sd = std_dev(p);
    if sd < 1e-3 {
        return (0, 0.0);
    }
    (0..10)
        .map(|d| {
            let ncc = p
                .iter()
                .zip(&templates()[d])
                .map(|(v, t)| (v - mean) / sd * t)
                .sum::<f32>()
                / p.len() as f32;
            (d as u32, ncc)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0, 0.0))
}
/// The slot patch moved by up to a pixel sideways to where it best matches a digit.
fn aligned(g: &Gray, x: i32, y: i32, w: usize, h: usize) -> Option<[f32; TW * TH]> {
    (-1..=1)
        .filter_map(|dx| g.patch(x + dx, y, w, h))
        .max_by(|a, b| best_digit(a).1.total_cmp(&best_digit(b).1))
}
/// A digit sits in the slot at `x` (within 2 pixels): the number is longer than the guess.
fn digit_at(g: &Gray, x: i32, y: i32, w: usize, h: usize, contrast: f32) -> bool {
    (-2..=2).any(|dx| {
        g.patch(x + dx, y, w, h)
            .is_some_and(|p| std_dev(&p) > 0.3 * contrast && best_digit(&p).1 >= 0.6)
    })
}
/// Bright strokes in the slot at `x` as bright as the number's own (a thin 1 matches poorly).
fn bright_at(g: &Gray, x: i32, y: i32, w: usize, h: usize, peak: f32) -> bool {
    let (from, to) = (
        (x + 1).max(0) as usize,
        ((x + w as i32 - 1).max(0) as usize).min(g.w),
    );
    (from..to).any(|c| {
        (y as usize..(y as usize + h).min(g.h))
            .filter(|&r| g.px[r * g.w + c] >= 0.92 * peak)
            .count()
            >= 5
    })
}

/// Players left in a `WIDTH`×`HEIGHT` RGB crop of `COUNTER`, or None when no counter is readable.
pub fn players_left(rgb: &[u8]) -> Option<u32> {
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    if rgb.len() != w * h * 3 {
        return None;
    }
    let px: Vec<f32> = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| p.iter().map(|&v| f32::from(v)).sum::<f32>() / 3.0)
        .collect();
    let g = Gray { px: &px, w, h };
    let mut best: Option<(f32, Vec<u32>)> = None;
    // Slot size (pitch, height) for the two HUD scales seen, the units digit's left edge, the top.
    for (pitch, height) in [(8usize, 10usize), (7, 9)] {
        for x in 23..34 {
            for y in 3..7 {
                for n in 1..=3i32 {
                    let xs: Vec<i32> = (0..n).rev().map(|k| x - pitch as i32 * k).collect();
                    let Some(patches) = xs
                        .iter()
                        .map(|&sx| aligned(&g, sx, y, pitch, height))
                        .collect::<Option<Vec<_>>>()
                    else {
                        break;
                    };
                    let reads: Vec<(u32, f32)> = patches.iter().map(best_digit).collect();
                    let weakest = reads.iter().map(|r| r.1).fold(f32::MAX, f32::min);
                    let faintest = patches.iter().map(|p| std_dev(p)).fold(f32::MAX, f32::min);
                    if weakest < MIN_MATCH || faintest < 15.0 || (n > 1 && reads[0].0 == 0) {
                        continue;
                    }
                    let contrast = std_dev(patches.last()?);
                    let peak = patches.iter().flatten().copied().fold(0.0, f32::max);
                    let outside = [xs[0] - pitch as i32, xs[n as usize - 1] + pitch as i32];
                    if outside.iter().any(|&ox| {
                        digit_at(&g, ox, y, pitch, height, contrast)
                            || bright_at(&g, ox, y, pitch, height, peak)
                    }) {
                        continue;
                    }
                    let score = reads.iter().map(|r| r.1).sum::<f32>() / n as f32;
                    if best.as_ref().is_none_or(|b| score > b.0) {
                        best = Some((score, reads.iter().map(|r| r.0).collect()));
                    }
                }
            }
        }
    }
    best.map(|(_, digits)| digits.iter().fold(0, |v, d| v * 10 + d))
}

/// One recording's recent reads. Within a match the count only falls, so a read counts only when
/// it is at most 8 below the last plausible one; a dropped digit (30 read as 3) never does. A
/// clearly higher read is a new match. A plausible read goes stale after 30 seconds, so a real
/// fall of more than 8 at once (or a long stretch with the map open) costs one read, not the moment.
#[derive(Default)]
pub struct Endgame {
    last: Option<(u32, std::time::Instant)>,
}
impl Endgame {
    /// Players left, if this read is a plausible endgame count.
    pub fn read(&mut self, left: Option<u32>) -> Option<u32> {
        let left = left.filter(|&v| v > 0)?;
        let now = std::time::Instant::now();
        let fresh = self
            .last
            .filter(|(_, at)| now.duration_since(*at) < std::time::Duration::from_secs(30));
        match fresh {
            Some((last, _)) if left + 8 >= last && left <= last + 2 => {
                self.last = Some((left, now));
                (left <= ENDGAME).then_some(left)
            }
            // A new match (or the first read): remember it, but it can't be a moment yet.
            Some((last, _)) if left > last + 2 => {
                self.last = Some((left, now));
                None
            }
            Some(_) => None,
            None => {
                self.last = Some((left, now));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A crop with `number` drawn right-aligned from the templates on a busy background.
    fn crop(number: Option<u32>) -> Vec<u8> {
        let (w, h) = (WIDTH as usize, HEIGHT as usize);
        let mut px: Vec<f32> = (0..w * h)
            .map(|i| 70.0 + ((i * 37) % 23) as f32 + (i % w) as f32)
            .collect();
        if let Some(n) = number {
            for (k, d) in n.to_string().bytes().rev().enumerate() {
                let t = &templates()[usize::from(d - b'0')];
                let x0 = 30 - 8 * k;
                for r in 0..TH {
                    for c in 0..TW {
                        px[(4 + r) * w + x0 + c] = (140.0 + 45.0 * t[r * TW + c]).clamp(0.0, 255.0);
                    }
                }
            }
        }
        px.iter().flat_map(|&v| [v as u8; 3]).collect()
    }

    #[test]
    fn the_shipped_templates_parse() {
        assert_eq!(DIGITS.len(), 10 * TW * TH * 4);
        for t in templates() {
            assert!(
                (std_dev(t) - 1.0).abs() < 0.01,
                "templates are z-normalized"
            );
        }
    }

    #[test]
    fn reads_one_to_three_digits_and_nothing_from_a_blank() {
        for n in [1, 2, 7, 10, 12, 38, 64, 99, 100] {
            assert_eq!(players_left(&crop(Some(n))), Some(n), "{n}");
        }
        assert_eq!(players_left(&crop(None)), None);
        assert_eq!(players_left(&[0; 10]), None);
    }

    #[test]
    fn endgame_needs_a_plausible_fall_and_ignores_dropped_digits() {
        let mut e = Endgame::default();
        assert_eq!(e.read(Some(9)), None, "the first read only sets the level");
        assert_eq!(e.read(Some(9)), Some(9));
        assert_eq!(e.read(None), None, "no counter on this frame");
        assert_eq!(e.read(Some(7)), Some(7));
        assert_eq!(e.read(Some(70)), None, "a new match");
        let mut e = Endgame::default();
        for v in [33, 32, 30] {
            assert_eq!(e.read(Some(v)), None);
        }
        assert_eq!(e.read(Some(3)), None, "30 read as 3");
        assert_eq!(
            e.read(Some(3)),
            None,
            "still 27 below the last plausible read"
        );
        for v in [28, 22, 15, 11] {
            assert_eq!(e.read(Some(v)), None);
        }
        assert_eq!(e.read(Some(10)), Some(10));
        assert_eq!(e.read(Some(0)), None, "0 is never a count");
    }

    /// A real MPEG-TS segment through the production crop: FORTNITE_SEGMENT is its path,
    /// FORTNITE_LEFT the players left on its first frame.
    #[tokio::test]
    #[ignore = "needs ffmpeg and a local segment; run explicitly with --ignored"]
    async fn reads_a_real_segment() {
        let ts = std::fs::read(std::env::var("FORTNITE_SEGMENT").unwrap()).unwrap();
        let crop = crate::probe::region(ts, COUNTER, WIDTH, HEIGHT)
            .await
            .unwrap();
        let left: u32 = std::env::var("FORTNITE_LEFT").unwrap().parse().unwrap();
        assert_eq!(players_left(&crop), Some(left));
    }

    /// Hand-labeled counters from real footage. Not shipped (the footage isn't ours): point
    /// FORTNITE_CROPS at a file of 44×18 RGB crops of `COUNTER` and FORTNITE_LABELS at a CSV of
    /// `index,value` (an empty value: no counter on screen).
    #[test]
    #[ignore = "needs local footage crops; run explicitly with --ignored"]
    fn reads_labeled_footage() {
        let crops = std::fs::read(std::env::var("FORTNITE_CROPS").unwrap()).unwrap();
        let reads = std::fs::read_to_string(std::env::var("FORTNITE_LABELS").unwrap()).unwrap();
        let size = (WIDTH * HEIGHT * 3) as usize;
        let (mut same, mut total) = (0, 0);
        for line in reads.lines().skip(1) {
            let (i, v) = line.split_once(',').unwrap();
            let i: usize = i.parse().unwrap();
            let expected = v.parse::<u32>().ok();
            let got = players_left(&crops[i * size..(i + 1) * size]);
            total += 1;
            if got == expected {
                same += 1;
            } else {
                eprintln!("{i}: labeled {expected:?}, read {got:?}");
            }
        }
        eprintln!("{same}/{total} read as labeled");
        assert!(same as f64 >= 0.95 * total as f64);
    }
}
