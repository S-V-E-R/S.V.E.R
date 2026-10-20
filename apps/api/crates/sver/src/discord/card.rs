//! The `/rank` card (docs/DISCORD_BOT.md "Levels and ranks"): a 950×300 PNG in the member's faction
//! colours, drawn from the self-hosted Cinzel and Barlow fonts.
use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use image::{Rgba, RgbaImage, imageops};

const WIDTH: u32 = 950;
const HEIGHT: u32 = 300;
static CINZEL: &[u8] = include_bytes!("../../assets/fonts/Cinzel.ttf");
static BARLOW: &[u8] = include_bytes!("../../assets/fonts/Barlow-Regular.ttf");
static BARLOW_BOLD: &[u8] = include_bytes!("../../assets/fonts/Barlow-SemiBold.ttf");

pub struct Card {
    pub name: String,
    pub rank: String,
    pub faction: Option<String>,
    pub level: i64,
    pub total: i64,
    /// XP where the current rank starts, and where the next one does (None at the top rank).
    pub from: i64,
    pub to: Option<i64>,
    pub position: i64,
    pub members: i64,
    pub today: i64,
    pub cap: i64,
    /// A square avatar image, any size.
    pub avatar: Option<RgbaImage>,
}

/// Background from, to and accent per faction.
fn palette(faction: Option<&str>) -> ([u8; 3], [u8; 3], [u8; 3]) {
    match faction {
        Some("myria") => ([0x1a, 0, 0], [0x3d, 0, 0], [0xe7, 0x4c, 0x3c]),
        Some("aetheron") => ([0, 0x0a, 0x1a], [0, 0x1a, 0x3d], [0x44, 0x88, 0xff]),
        Some("glint") => ([0x1a, 0x15, 0], [0x3d, 0x30, 0], [0xff, 0xd7, 0]),
        _ => ([0x1a, 0x1a, 0x1a], [0x2d, 0x2d, 0x2d], [0x88, 0x88, 0x88]),
    }
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}
/// Paints `color` over a pixel with `alpha` coverage.
fn blend(img: &mut RgbaImage, x: i64, y: i64, color: [u8; 3], alpha: f32) {
    if x < 0 || y < 0 || x >= WIDTH as i64 || y >= HEIGHT as i64 || alpha <= 0.0 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    let a = alpha.min(1.0);
    for (channel, c) in p.0.iter_mut().zip(color) {
        *channel = (*channel as f32 * (1.0 - a) + c as f32 * a).round() as u8;
    }
}
/// How much of the pixel at (x, y) lies inside a rounded rectangle (anti-aliased by distance).
fn inside_round(x: f32, y: f32, rect: (f32, f32, f32, f32), r: f32) -> f32 {
    let (left, top, w, h) = rect;
    let cx = x.clamp(left + r, left + w - r);
    let cy = y.clamp(top + r, top + h - r);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    (r - d + 0.5).clamp(0.0, 1.0)
}
fn round_rect(
    img: &mut RgbaImage,
    rect: (f32, f32, f32, f32),
    r: f32,
    color: impl Fn(f32) -> [u8; 3],
    alpha: f32,
) {
    let (left, top, w, h) = rect;
    for y in top.floor() as i64..(top + h).ceil() as i64 {
        for x in left.floor() as i64..(left + w).ceil() as i64 {
            let cover = inside_round(x as f32 + 0.5, y as f32 + 0.5, rect, r);
            blend(img, x, y, color((x as f32 - left) / w), cover * alpha);
        }
    }
}

/// Width of a line of text.
fn measure(font: &FontRef, px: f32, text: &str) -> f32 {
    let f = font.as_scaled(PxScale::from(px));
    let mut width = 0.0;
    let mut last = None;
    for c in text.chars() {
        let id = f.glyph_id(c);
        if let Some(prev) = last {
            width += f.kern(prev, id);
        }
        width += f.h_advance(id);
        last = Some(id);
    }
    width
}
/// One line of text: font, size, baseline position, colour and the widest it may be (cut with "…").
struct Line<'a> {
    font: &'a FontRef<'a>,
    px: f32,
    x: f32,
    y: f32,
    color: [u8; 3],
    max: f32,
}
fn text(img: &mut RgbaImage, l: Line, line: &str) {
    let mut line = line.to_string();
    while measure(l.font, l.px, &line) > l.max && line.chars().count() > 1 {
        line = line.trim_end_matches('…').to_string();
        line.pop();
        line.push('…');
    }
    let f = l.font.as_scaled(PxScale::from(l.px));
    let mut caret = l.x;
    let mut last = None;
    for c in line.chars() {
        let id = f.glyph_id(c);
        if let Some(prev) = last {
            caret += f.kern(prev, id);
        }
        let glyph = id.with_scale_and_position(PxScale::from(l.px), point(caret, l.y));
        caret += f.h_advance(id);
        last = Some(id);
        if let Some(outline) = l.font.outline_glyph(glyph) {
            let b = outline.px_bounds();
            outline.draw(|gx, gy, cover| {
                blend(
                    img,
                    b.min.x as i64 + gx as i64,
                    b.min.y as i64 + gy as i64,
                    l.color,
                    cover,
                );
            });
        }
    }
}
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

pub fn render(card: &Card) -> Vec<u8> {
    let cinzel = FontRef::try_from_slice(CINZEL).expect("bundled font");
    let barlow = FontRef::try_from_slice(BARLOW).expect("bundled font");
    let bold = FontRef::try_from_slice(BARLOW_BOLD).expect("bundled font");
    let (from, to, accent) = palette(card.faction.as_deref());
    let white = [0xff, 0xff, 0xff];
    let mut img = RgbaImage::from_pixel(WIDTH, HEIGHT, Rgba([0, 0, 0, 0]));
    // Background: a diagonal gradient with rounded corners, and the accent line on top.
    let whole = (0.0, 0.0, WIDTH as f32, HEIGHT as f32);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let cover = inside_round(x as f32 + 0.5, y as f32 + 0.5, whole, 20.0);
            let c = mix(
                from,
                to,
                (x as f32 / WIDTH as f32 + y as f32 / HEIGHT as f32) / 2.0,
            );
            img.put_pixel(x, y, Rgba([c[0], c[1], c[2], (cover * 255.0) as u8]));
        }
    }
    round_rect(
        &mut img,
        (0.0, 0.0, WIDTH as f32, 4.0),
        2.0,
        |_| accent,
        1.0,
    );

    // Avatar in a circle with a ring in the faction colour; a plain circle without one.
    let (size, ax, ay) = (128.0f32, 40.0f32, (HEIGHT as f32 - 128.0) / 2.0);
    let (cx, cy, r) = (ax + size / 2.0, ay + size / 2.0, size / 2.0);
    let avatar = card
        .avatar
        .as_ref()
        .map(|a| imageops::resize(a, size as u32, size as u32, imageops::FilterType::Triangle));
    for y in (ay - 6.0) as i64..(ay + size + 6.0) as i64 {
        for x in (ax - 6.0) as i64..(ax + size + 6.0) as i64 {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let fill = (r - d + 0.5).clamp(0.0, 1.0);
            match &avatar {
                Some(a) => {
                    let ix = (x as f32 - ax).clamp(0.0, size - 1.0) as u32;
                    let iy = (y as f32 - ay).clamp(0.0, size - 1.0) as u32;
                    let p = a.get_pixel(ix, iy).0;
                    blend(
                        &mut img,
                        x,
                        y,
                        [p[0], p[1], p[2]],
                        fill * p[3] as f32 / 255.0,
                    );
                }
                None => blend(&mut img, x, y, [0x44, 0x44, 0x44], fill),
            }
            let ring = (2.0 - (d - (r + 2.0)).abs()).clamp(0.0, 1.0);
            blend(&mut img, x, y, accent, ring);
        }
    }

    // Name, then rank and level.
    let tx = ax + size + 30.0;
    let room = WIDTH as f32 - tx - 40.0;
    let line = |font, px, x, y, color, max| Line {
        font,
        px,
        x,
        y,
        color,
        max,
    };
    text(
        &mut img,
        line(&bold, 36.0, tx, 82.0, white, room),
        &card.name,
    );
    let level = format!("LEVEL {}", card.level);
    let lw = measure(&bold, 20.0, &level);
    text(
        &mut img,
        line(&cinzel, 26.0, tx, 124.0, accent, room - lw - 20.0),
        &card.rank.to_uppercase(),
    );
    text(
        &mut img,
        line(
            &bold,
            20.0,
            WIDTH as f32 - 40.0 - lw,
            122.0,
            [0xdd; 3],
            lw + 1.0,
        ),
        &level,
    );

    // Progress to the next rank.
    let (bx, by, bw, bh) = (tx, 160.0, room, 26.0);
    round_rect(&mut img, (bx, by, bw, bh), bh / 2.0, |_| white, 0.12);
    let pct = match card.to {
        Some(next) if next > card.from => {
            ((card.total - card.from) as f32 / (next - card.from) as f32).clamp(0.0, 1.0)
        }
        _ => 1.0,
    };
    let light = mix(accent, white, 0.35);
    round_rect(
        &mut img,
        (bx, by, (bw * pct).max(bh), bh),
        bh / 2.0,
        |t| mix(light, accent, t),
        1.0,
    );
    let progress = match card.to {
        Some(next) => format!("{} / {} XP", thousands(card.total), thousands(next)),
        None => format!("{} XP · top rank", thousands(card.total)),
    };
    let pw = measure(&bold, 16.0, &progress);
    text(
        &mut img,
        line(&bold, 16.0, bx + (bw - pw) / 2.0, by + 19.0, white, bw),
        &progress,
    );

    // Server position, today's XP, and the percentage.
    let position = format!(
        "#{} of {} in the server  ·  {} / {} XP today",
        card.position,
        card.members,
        thousands(card.today),
        thousands(card.cap)
    );
    text(
        &mut img,
        line(&barlow, 18.0, bx, 224.0, [0xcc; 3], bw - 80.0),
        &position,
    );
    let percent = format!("{}%", (pct * 100.0).round() as i64);
    let w = measure(&bold, 18.0, &percent);
    text(
        &mut img,
        line(&bold, 18.0, bx + bw - w, 224.0, accent, 80.0),
        &percent,
    );

    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("PNG encoding to memory");
    out.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_card_is_a_950_by_300_png() {
        let card = Card {
            name: "A very long display name that has to be cut short somewhere".into(),
            rank: "Warblade".into(),
            faction: Some("myria".into()),
            level: 4,
            total: 2_500,
            from: 2_000,
            to: Some(4_000),
            position: 3,
            members: 28,
            today: 340,
            cap: 2_000,
            avatar: Some(RgbaImage::from_pixel(64, 64, Rgba([10, 200, 30, 255]))),
        };
        let png = render(&card);
        // DISCORD_CARD_PREVIEW=path writes the card, to look at it.
        if let Ok(path) = std::env::var("DISCORD_CARD_PREVIEW") {
            std::fs::write(path, &png).unwrap();
        }
        let back = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), (950, 300));
        // The avatar is drawn inside its circle, and the corners are transparent.
        assert_eq!(back.get_pixel(104, 150).0[1], 200);
        assert_eq!(back.get_pixel(0, 0).0[3], 0);
        assert_eq!(thousands(1_234_567), "1,234,567");
        let plain = Card {
            faction: None,
            to: None,
            avatar: None,
            ..card
        };
        assert!(render(&plain).len() > 1000);
    }
}
