//! Battle royale counters read from a recorded segment for MAGNet endgame moments (docs/MAGNET.md
//! "What counts as a moment"): Fortnite's players left and Apex Legends' squads left. CLIP can't
//! read numbers, so each game's digits are matched against 10 digit templates instead.
//!
//! A game's `area` is where its counter can be, as fractions of the game area. It is cut at the
//! stream's own resolution and scaled to 44×18 (the counter's size at 720p). The reader tries each
//! place the last digit can start, matches it against the templates (normalized cross-correlation,
//! so any background and brightness works), then walks left one digit at a time, searching the gap
//! so a narrow 1 fits, until nothing scores like a digit. It keeps the best-scoring number.
//!
//! The templates (`<game>_digits.bin`: 10 × the slot size in little-endian f32, each z-normalized,
//! row-major) are averages of hand-labeled counters from long recordings of each game. Training on
//! half the labels and reading the other half: Fortnite 175 of 181 (5 of the 6 misses read nothing),
//! Apex 191 of 191. `Endgame::read` turns reads into moments a lone misread can't fake.
use std::ops::Range;

pub struct Counter {
    /// Where the counter can be, as `[x0, y0, x1, y1]` fractions of the game area.
    pub area: [f32; 4],
    /// Counts at or below this are the endgame.
    pub endgame: u32,
    /// What is counted, for the moment's title.
    pub unit: &'static str,
    /// Slot sizes for the HUD scales seen; patches are resampled to the first (the templates').
    sizes: &'static [(usize, usize)],
    /// Where the last digit's slot can start, and its top.
    xs: Range<i32>,
    ys: Range<i32>,
    /// From one digit's slot to the next one left.
    steps: Range<i32>,
    max_digits: usize,
    /// The score a digit after the first needs (Apex's box edge passes for a 1 at lower scores).
    next: f32,
    /// The space right of the number is empty (Fortnite) rather than a label (Apex).
    right_blank: bool,
    templates: &'static [u8],
}

pub const WIDTH: u32 = 44;
pub const HEIGHT: u32 = 18;
const MIN_MATCH: f32 = 0.7;

/// Right-aligned under the kills count, left of the minimap, at two HUD scales.
pub static FORTNITE: Counter = Counter {
    area: [1060.0 / 1280.0, 40.0 / 720.0, 1104.0 / 1280.0, 58.0 / 720.0],
    endgame: 10,
    unit: "players",
    sizes: &[(8, 10), (7, 9)],
    xs: 23..34,
    ys: 3..7,
    steps: 6..10,
    max_digits: 3,
    next: MIN_MATCH,
    right_blank: true,
    templates: include_bytes!("fortnite_digits.bin"),
};
/// Right-aligned against the "SQUADS LEFT" label, top right.
pub static APEX: Counter = Counter {
    area: [1030.0 / 1280.0, 35.0 / 720.0, 1074.0 / 1280.0, 53.0 / 720.0],
    endgame: 3,
    unit: "squads",
    sizes: &[(6, 11)],
    xs: 22..33,
    ys: 2..6,
    steps: 5..11,
    max_digits: 2,
    next: 0.88,
    right_blank: false,
    templates: include_bytes!("apex_digits.bin"),
};

/// The counter for a stream category, if it has one.
pub fn for_category(name: &str) -> Option<&'static Counter> {
    match name.trim_end_matches(['™', '®']) {
        "Fortnite" => Some(&FORTNITE),
        "Apex Legends" => Some(&APEX),
        _ => None,
    }
}

/// Grayscale crop, row-major.
struct Gray {
    px: Vec<f32>,
    w: usize,
    h: usize,
}
impl Gray {
    /// The `w`×`h` patch at (x, y) resampled to `tw`×`th`; None when it leaves the crop.
    fn patch(
        &self,
        x: i32,
        y: i32,
        (w, h): (usize, usize),
        (tw, th): (usize, usize),
    ) -> Option<Vec<f32>> {
        if x < 0 || y < 0 || x as usize + w > self.w || y as usize + h > self.h {
            return None;
        }
        let (x, y) = (x as usize, y as usize);
        let at = |c: usize, r: usize| self.px[(y + r) * self.w + x + c];
        let mut out = Vec::with_capacity(tw * th);
        for r in 0..th {
            // Bilinear, pixel centres aligned (an exact copy when the sizes match).
            let sy = ((r as f32 + 0.5) * h as f32 / th as f32 - 0.5).clamp(0.0, (h - 1) as f32);
            let (y0, fy) = (sy as usize, sy.fract());
            let y1 = (y0 + 1).min(h - 1);
            for c in 0..tw {
                let sx = ((c as f32 + 0.5) * w as f32 / tw as f32 - 0.5).clamp(0.0, (w - 1) as f32);
                let (x0, fx) = (sx as usize, sx.fract());
                let x1 = (x0 + 1).min(w - 1);
                let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
                let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
                out.push(top * (1.0 - fy) + bottom * fy);
            }
        }
        Some(out)
    }
}

fn std_dev(p: &[f32]) -> f32 {
    let mean = p.iter().sum::<f32>() / p.len() as f32;
    (p.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / p.len() as f32).sqrt()
}

impl Counter {
    fn templates(&self) -> Vec<Vec<f32>> {
        let values: Vec<f32> = self
            .templates
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        values
            .chunks(values.len() / 10)
            .map(<[f32]>::to_vec)
            .collect()
    }

    /// The count in a `WIDTH`×`HEIGHT` RGB crop of `area`, or None when no counter is readable.
    pub fn read(&self, rgb: &[u8]) -> Option<u32> {
        let (w, h) = (WIDTH as usize, HEIGHT as usize);
        if rgb.len() != w * h * 3 {
            return None;
        }
        let g = Gray {
            px: rgb
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| p.iter().map(|&v| f32::from(v)).sum::<f32>() / 3.0)
                .collect(),
            w,
            h,
        };
        let templates = self.templates();
        // The closest digit and how well it matches (-1 to 1).
        let best = |p: &[f32]| -> (u32, f32) {
            let mean = p.iter().sum::<f32>() / p.len() as f32;
            let sd = std_dev(p);
            if sd < 1e-3 {
                return (0, 0.0);
            }
            let score = |t: &[f32]| {
                p.iter()
                    .zip(t)
                    .map(|(v, t)| (v - mean) / sd * t)
                    .sum::<f32>()
                    / p.len() as f32
            };
            (0..10u32)
                .map(|d| (d, score(&templates[d as usize])))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap_or((0, 0.0))
        };
        let to = self.sizes[0];
        let mut top: Option<(f32, u32)> = None;
        for &size in self.sizes {
            let at = |x: i32, y: i32| g.patch(x, y, size, to);
            for y in self.ys.clone() {
                for x in self.xs.clone() {
                    let Some(last) = at(x, y) else { continue };
                    let (digit, score) = best(&last);
                    let contrast = std_dev(&last);
                    if score < MIN_MATCH || contrast < 15.0 {
                        continue;
                    }
                    if self.right_blank
                        && self.right_taken(&g, x, y, size, contrast, &last, &at, &best)
                    {
                        continue;
                    }
                    let (mut digits, mut scores, mut px) = (vec![digit], vec![score], x);
                    let mut too_long = false;
                    // The best-scoring digit one gap further left, while there is one.
                    while let Some(((d, s), nx, p)) = self
                        .steps
                        .clone()
                        .filter_map(|step| at(px - step, y).map(|p| (best(&p), px - step, p)))
                        .max_by(|a, b| a.0.1.total_cmp(&b.0.1))
                    {
                        if s < self.next || std_dev(&p) < 0.3 * contrast {
                            break;
                        }
                        if digits.len() == self.max_digits {
                            too_long = true;
                            break;
                        }
                        digits.insert(0, d);
                        scores.insert(0, s);
                        px = nx;
                    }
                    if too_long || (digits.len() > 1 && digits[0] == 0) {
                        continue;
                    }
                    let mean = scores.iter().sum::<f32>() / scores.len() as f32;
                    if top.is_none_or(|t| mean > t.0) {
                        top = Some((mean, digits.iter().fold(0, |v, d| v * 10 + d)));
                    }
                }
            }
        }
        top.map(|t| t.1)
    }

    /// Something digit-like, or strokes as bright as the digit, right of a guessed last digit: the
    /// number is longer than the guess.
    #[allow(clippy::too_many_arguments)]
    fn right_taken(
        &self,
        g: &Gray,
        x: i32,
        y: i32,
        (w, h): (usize, usize),
        contrast: f32,
        last: &[f32],
        at: &dyn Fn(i32, i32) -> Option<Vec<f32>>,
        best: &dyn Fn(&[f32]) -> (u32, f32),
    ) -> bool {
        let digit_like = self.steps.clone().any(|step| {
            at(x + step, y).is_some_and(|p| std_dev(&p) > 0.3 * contrast && best(&p).1 >= 0.6)
        });
        let peak = last.iter().copied().fold(0.0, f32::max);
        let (from, to) = (
            (x + w as i32 + 1) as usize,
            ((x + w as i32 + self.steps.end - 1).max(0) as usize).min(g.w),
        );
        let bright = (from..to).any(|c| {
            (y as usize..(y as usize + h).min(g.h))
                .filter(|&r| g.px[r * g.w + c] >= 0.92 * peak)
                .count()
                >= 5
        });
        digit_like || bright
    }
}

/// One recording's recent reads. Within a match the count only falls, so a read counts only when
/// it is at most 8 below the last plausible one; a dropped digit (30 read as 3) never does. A
/// clearly higher read is a new match. A plausible read goes stale after 30 seconds, so a real
/// fall of more than 8 at once (or a long stretch with the map open) costs one read, not the moment.
/// 1 is never a count: the game is over and its win screen covers the counter.
#[derive(Default)]
pub struct Endgame {
    last: Option<(u32, std::time::Instant)>,
}
impl Endgame {
    /// The count, if this read is a plausible endgame count (`endgame` or fewer).
    pub fn read(&mut self, count: Option<u32>, endgame: u32) -> Option<u32> {
        let count = count.filter(|&v| v > 1)?;
        let now = std::time::Instant::now();
        let fresh = self
            .last
            .filter(|(_, at)| now.duration_since(*at) < std::time::Duration::from_secs(30));
        match fresh {
            Some((last, _)) if count + 8 >= last && count <= last + 2 => {
                self.last = Some((count, now));
                (count <= endgame).then_some(count)
            }
            // A new match (or the first read): remember it, but it can't be a moment yet.
            Some((last, _)) if count > last + 2 => {
                self.last = Some((count, now));
                None
            }
            Some(_) => None,
            None => {
                self.last = Some((count, now));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A crop with `number` drawn right-aligned from the game's templates on a busy background,
    /// last digit at `x`, digits `step` apart.
    fn crop(game: &Counter, number: Option<u32>, x: usize, y: usize, step: usize) -> Vec<u8> {
        let (w, h) = (WIDTH as usize, HEIGHT as usize);
        let (tw, th) = game.sizes[0];
        let templates = game.templates();
        let mut px: Vec<f32> = (0..w * h)
            .map(|i| 70.0 + ((i * 37) % 23) as f32 + (i % w) as f32)
            .collect();
        if let Some(n) = number {
            for (k, d) in n.to_string().bytes().rev().enumerate() {
                let t = &templates[usize::from(d - b'0')];
                let x0 = x - step * k;
                for r in 0..th {
                    for c in 0..tw {
                        px[(y + r) * w + x0 + c] = (140.0 + 45.0 * t[r * tw + c]).clamp(0.0, 255.0);
                    }
                }
            }
        }
        px.iter().flat_map(|&v| [v as u8; 3]).collect()
    }

    #[test]
    fn the_shipped_templates_parse() {
        for game in [&FORTNITE, &APEX] {
            let (tw, th) = game.sizes[0];
            assert_eq!(game.templates.len(), 10 * tw * th * 4);
            for t in game.templates() {
                assert!(
                    (std_dev(&t) - 1.0).abs() < 0.01,
                    "templates are z-normalized"
                );
            }
        }
    }

    #[test]
    fn reads_one_to_three_digits_and_nothing_from_a_blank() {
        for n in [2, 7, 10, 12, 38, 64, 99, 100] {
            assert_eq!(
                FORTNITE.read(&crop(&FORTNITE, Some(n), 30, 4, 8)),
                Some(n),
                "Fortnite {n}"
            );
        }
        for n in [2, 3, 9, 14, 20] {
            assert_eq!(
                APEX.read(&crop(&APEX, Some(n), 27, 4, 8)),
                Some(n),
                "Apex {n}"
            );
        }
        for game in [&FORTNITE, &APEX] {
            assert_eq!(game.read(&crop(game, None, 0, 0, 0)), None);
            assert_eq!(game.read(&[0; 10]), None);
        }
    }

    #[test]
    fn categories_with_a_counter() {
        assert!(for_category("Fortnite").is_some_and(|c| c.unit == "players"));
        assert!(for_category("Apex Legends™").is_some_and(|c| c.unit == "squads"));
        assert!(for_category("Apex Legends Mobile").is_none());
        assert!(for_category("Call of Duty: Modern Warfare 4").is_none());
    }

    #[test]
    fn endgame_needs_a_plausible_fall_and_ignores_dropped_digits() {
        let mut e = Endgame::default();
        assert_eq!(
            e.read(Some(9), 10),
            None,
            "the first read only sets the level"
        );
        assert_eq!(e.read(Some(9), 10), Some(9));
        assert_eq!(e.read(None, 10), None, "no counter on this frame");
        assert_eq!(e.read(Some(7), 10), Some(7));
        assert_eq!(
            e.read(Some(1), 10),
            None,
            "1 is the win screen, never a count"
        );
        assert_eq!(e.read(Some(70), 10), None, "a new match");
        let mut e = Endgame::default();
        for v in [33, 32, 30] {
            assert_eq!(e.read(Some(v), 10), None);
        }
        assert_eq!(e.read(Some(3), 10), None, "30 read as 3");
        assert_eq!(
            e.read(Some(3), 10),
            None,
            "still 27 below the last plausible read"
        );
        for v in [28, 22, 15, 11] {
            assert_eq!(e.read(Some(v), 10), None);
        }
        assert_eq!(e.read(Some(10), 10), Some(10));
        let mut e = Endgame::default();
        for v in [5, 4] {
            e.read(Some(v), 3);
        }
        assert_eq!(e.read(Some(3), 3), Some(3), "Apex: 3 squads left");
    }

    /// A real MPEG-TS segment through the production crop: HUD_GAME is the category, HUD_SEGMENT
    /// the segment's path, HUD_COUNT the count on its first frame.
    #[tokio::test]
    #[ignore = "needs ffmpeg and a local segment; run explicitly with --ignored"]
    async fn reads_a_real_segment() {
        let game = for_category(&std::env::var("HUD_GAME").unwrap()).unwrap();
        let ts = std::fs::read(std::env::var("HUD_SEGMENT").unwrap()).unwrap();
        let crop = crate::probe::region(ts, game.area, WIDTH, HEIGHT)
            .await
            .unwrap();
        let count: u32 = std::env::var("HUD_COUNT").unwrap().parse().unwrap();
        assert_eq!(game.read(&crop), Some(count));
    }

    /// Hand-labeled counters from real footage. Not shipped (the footage isn't ours): HUD_GAME is
    /// the category, HUD_CROPS a file of 44×18 RGB crops of its `area`, HUD_LABELS a CSV of
    /// `index,value` (an empty value: no counter on screen).
    #[test]
    #[ignore = "needs local footage crops; run explicitly with --ignored"]
    fn reads_labeled_footage() {
        let game = for_category(&std::env::var("HUD_GAME").unwrap()).unwrap();
        let crops = std::fs::read(std::env::var("HUD_CROPS").unwrap()).unwrap();
        let labels = std::fs::read_to_string(std::env::var("HUD_LABELS").unwrap()).unwrap();
        let size = (WIDTH * HEIGHT * 3) as usize;
        let (mut same, mut total) = (0, 0);
        for line in labels.lines().skip(1) {
            let (i, v) = line.split_once(',').unwrap();
            let i: usize = i.parse().unwrap();
            let expected = v.parse::<u32>().ok();
            let got = game.read(&crops[i * size..(i + 1) * size]);
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
