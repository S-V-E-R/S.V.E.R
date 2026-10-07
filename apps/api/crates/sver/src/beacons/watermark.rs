//! The moving watermark (docs/BEACONS.md "Watermark"). The public copies carry a small
//! "@username · sver.tv" mark that hops between the four corners of the safe area. Each Beacon's
//! seed decides the corner order and how long each corner is held, between the private tuning
//! bounds, so there is no fixed pattern to crop or blur away.

/// Corners inside the safe area: clear of the feed's right-side rail and the bottom captions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}
const CORNERS: [Corner; 4] = [
    Corner::TopLeft,
    Corner::TopRight,
    Corner::BottomRight,
    Corner::BottomLeft,
];
impl Corner {
    /// drawtext x/y expressions on a 1080×1920 frame (`w`,`h` frame; `tw`,`th` text).
    fn x(self) -> &'static str {
        match self {
            // The rail takes the right ~18% of the frame on phones.
            Corner::TopRight | Corner::BottomRight => "w-tw-w*0.2",
            Corner::TopLeft | Corner::BottomLeft => "w*0.06",
        }
    }
    fn y(self) -> &'static str {
        match self {
            Corner::TopLeft | Corner::TopRight => "h*0.07",
            // Above the title, creator line and captions.
            Corner::BottomLeft | Corner::BottomRight => "h-th-h*0.25",
        }
    }
}

/// splitmix64: a small, well-mixed generator; the plan only has to be unpredictable per Beacon.
struct Mix(u64);
impl Mix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// (start_ms, corner) steps covering `duration_ms`. Every corner is visited once per cycle in a
/// shuffled order, and no corner repeats across a cycle boundary.
pub fn plan(seed: i64, duration_ms: i64, min_ms: i64, max_ms: i64) -> Vec<(i64, Corner)> {
    let mut rng = Mix(seed as u64);
    let span = (max_ms - min_ms).max(0) as u64 + 1;
    let mut steps = Vec::new();
    let mut at = 0;
    let mut last: Option<Corner> = None;
    while at < duration_ms.max(1) {
        let mut order = CORNERS;
        for i in (1..order.len()).rev() {
            order.swap(i, rng.below(i as u64 + 1) as usize);
        }
        if last == Some(order[0]) {
            order.swap(0, 1 + rng.below(3) as usize);
        }
        for corner in order {
            if at >= duration_ms.max(1) {
                break;
            }
            steps.push((at, corner));
            last = Some(corner);
            at += min_ms + rng.below(span) as i64;
        }
    }
    steps
}

/// Nested `if(lt(t,…),…)` expression selecting each step's coordinate.
fn select(steps: &[(i64, Corner)], pick: fn(Corner) -> &'static str) -> String {
    let mut expression = pick(steps.last().map_or(Corner::TopLeft, |s| s.1)).to_string();
    for window in steps.windows(2).rev() {
        expression = format!(
            "if(lt(t,{:.3}),{},{expression})",
            window[1].0 as f64 / 1000.0,
            pick(window[0].1)
        );
    }
    expression
}

/// The drawtext filter for one Beacon. `username` is already restricted to [A-Za-z0-9_].
pub fn filter(username: &str, font: &str, steps: &[(i64, Corner)]) -> String {
    let text: String = username
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let font: String = font
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "/._-".contains(*c))
        .collect();
    format!(
        "drawtext=fontfile='{font}':text='@{text} · sver.tv':fontsize=h*0.022:fontcolor=white@0.5:shadowcolor=black@0.4:shadowx=2:shadowy=2:x='{}':y='{}'",
        select(steps, Corner::x),
        select(steps, Corner::y)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_corner_is_visited_and_no_corner_repeats() {
        for seed in [0, 1, -7, 42, i64::MAX] {
            let steps = plan(seed, 60000, 3000, 6000);
            assert!(steps.len() >= 10);
            assert_eq!(steps[0].0, 0);
            for w in steps.windows(2) {
                assert_ne!(w[0].1, w[1].1, "a corner was held twice in a row");
                let gap = w[1].0 - w[0].0;
                assert!((3000..=6000).contains(&gap));
            }
            for corner in CORNERS {
                assert!(steps[..4].iter().any(|s| s.1 == corner));
            }
        }
    }
    #[test]
    fn plans_differ_per_beacon() {
        assert_ne!(plan(1, 60000, 3000, 6000), plan(2, 60000, 3000, 6000));
        assert_eq!(plan(9, 60000, 3000, 6000), plan(9, 60000, 3000, 6000));
    }
    #[test]
    fn the_filter_cannot_be_escaped_by_text() {
        let f = filter("Bad':x=0", "/fonts/a b'.ttf", &plan(3, 9000, 3000, 6000));
        assert!(f.contains("text='@Badx0 · sver.tv'"));
        assert!(f.contains("fontfile='/fonts/ab.ttf'"));
        assert_eq!(f.matches('\'').count(), 8);
    }
}
