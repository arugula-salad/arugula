//! The link preview's card for a shared Doc (#255): one picture for every
//! Doc, which Slack, iMessage and X show beside the link. Control can't
//! read a Doc, so the card says only what made it: the Arugula leaf, the
//! name, and "a shared Doc", on the Doc's dark paper (style.css's `.doc`).
//!
//! It's drawn here, once, and kept: the repo holds no pictures
//! (AGENTS.md, "Binaries"), and a preview bot can't read an SVG. The leaf
//! is the site's mark (its `#leaf` symbol, a 40-unit box); the letters are
//! strokes on a condensed grid, close to the Doc's display face.

use std::sync::OnceLock;

pub const W: u32 = 1200;
pub const H: u32 = 630;

const PAPER: [u8; 3] = [0x1a, 0x1b, 0x18];
const INK: [u8; 3] = [0xec, 0xeb, 0xe4];
const FAINT: [u8; 3] = [0x91, 0x8f, 0x85];
const LEAF: [u8; 3] = [0x9c, 0xc3, 0x6b];
const PEPPER: [u8; 3] = [0xd9, 0x7a, 0x52];

/// The card as a PNG, drawn the first time it's asked for.
pub fn png() -> &'static [u8] {
    static CARD: OnceLock<Vec<u8>> = OnceLock::new();
    CARD.get_or_init(|| encode(W, H, &draw()))
}

// ---------------------------------------------------------------- shapes

#[derive(Clone, Copy)]
enum Shape {
    /// A segment with round ends, `r` its half width.
    Stroke {
        a: (f32, f32),
        b: (f32, f32),
        r: f32,
    },
    /// An ellipse turned by `turn` radians.
    Ellipse {
        c: (f32, f32),
        rx: f32,
        ry: f32,
        turn: f32,
    },
    Rect {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
    },
}

impl Shape {
    /// About how far `p` is outside the shape, in pixels (inside is below 0).
    fn distance(&self, (px, py): (f32, f32)) -> f32 {
        match *self {
            Shape::Stroke { a, b, r } => {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len = dx * dx + dy * dy;
                let t = if len == 0.0 { 0.0 } else { (((px - a.0) * dx + (py - a.1) * dy) / len).clamp(0.0, 1.0) };
                let (qx, qy) = (a.0 + t * dx - px, a.1 + t * dy - py);
                (qx * qx + qy * qy).sqrt() - r
            }
            Shape::Ellipse { c, rx, ry, turn } => {
                let (s, k) = turn.sin_cos();
                let (x, y) = (px - c.0, py - c.1);
                let (u, v) = (x * k + y * s, -x * s + y * k);
                let f = (u / rx).powi(2) + (v / ry).powi(2) - 1.0;
                let g = 2.0 * ((u / (rx * rx)).powi(2) + (v / (ry * ry)).powi(2)).sqrt();
                if g == 0.0 { -rx.min(ry) } else { f / g }
            }
            Shape::Rect { x0, y0, x1, y1 } => (x0 - px).max(px - x1).max(y0 - py).max(py - y1),
        }
    }

    fn bounds(&self) -> (f32, f32, f32, f32) {
        match *self {
            Shape::Stroke { a, b, r } => (a.0.min(b.0) - r, a.1.min(b.1) - r, a.0.max(b.0) + r, a.1.max(b.1) + r),
            Shape::Ellipse { c, rx, ry, .. } => {
                let m = rx.max(ry);
                (c.0 - m, c.1 - m, c.0 + m, c.1 + m)
            }
            Shape::Rect { x0, y0, x1, y1 } => (x0, y0, x1, y1),
        }
    }
}

/// Everything on the card, in its colours, drawn in order.
fn scene() -> Vec<([u8; 3], Shape)> {
    let mut out = Vec::new();
    // The leaf: the site's symbol, 40 units scaled to 360px.
    let (lx, ly, s) = (118.0, 135.0, 9.0);
    let at = |x: f32, y: f32| (lx + x * s, ly + y * s);
    out.push((LEAF, Shape::Stroke { a: at(20.0, 38.0), b: at(20.0, 8.0), r: 0.8 * s }));
    let leaves: [(f32, f32, f32, f32, f32); 7] = [
        (20.0, 8.5, 6.2, 7.0, 0.0),
        (14.0, 18.0, 5.0, 3.0, -28.0),
        (26.0, 18.0, 5.0, 3.0, 28.0),
        (15.2, 25.5, 3.8, 2.3, -28.0),
        (24.8, 25.5, 3.8, 2.3, 28.0),
        (16.8, 31.5, 2.6, 1.6, -28.0),
        (23.2, 31.5, 2.6, 1.6, 28.0),
    ];
    for (cx, cy, rx, ry, deg) in leaves {
        out.push((LEAF, Shape::Ellipse { c: at(cx, cy), rx: rx * s, ry: ry * s, turn: deg.to_radians() }));
    }
    // The name, the Doc head's rule under it (pepper, then leaf), and what this is.
    let x = 556.0;
    let end = word(&mut out, INK, "ARUGULA", x, 190.0, 128.0);
    out.push((PEPPER, Shape::Rect { x0: x, y0: 350.0, x1: x + 64.0, y1: 356.0 }));
    out.push((LEAF, Shape::Rect { x0: x + 64.0, y0: 350.0, x1: end, y1: 356.0 }));
    word(&mut out, FAINT, "A SHARED DOC", x, 388.0, 44.0);
    out
}

/// A glyph's strokes on a box `G` wide and 1 tall, `y` down.
const G: f32 = 0.46;
/// Stroke width, of the cap height.
const WEIGHT: f32 = 0.14;

fn glyph(c: char) -> Vec<Vec<(f32, f32)>> {
    use std::f32::consts::{FRAC_PI_2 as Q, PI};
    let r = G / 2.0;
    // An arc of the ellipse at `c`, from angle `t0` to `t1` (y down).
    let arc = |c: (f32, f32), rx: f32, ry: f32, t0: f32, t1: f32| -> Vec<(f32, f32)> {
        (0..=24).map(|i| t0 + (t1 - t0) * i as f32 / 24.0).map(|t| (c.0 + rx * t.cos(), c.1 + ry * t.sin())).collect()
    };
    let join = |parts: Vec<Vec<(f32, f32)>>| parts.concat();
    match c {
        'A' => vec![vec![(0.0, 1.0), (G / 2.0, 0.0), (G, 1.0)], vec![(G * 0.2, 0.68), (G * 0.8, 0.68)]],
        'R' => vec![
            join(vec![vec![(0.0, 1.0), (0.0, 0.0)], arc((G - 0.26, 0.26), 0.26, 0.26, -Q, Q), vec![(0.0, 0.52)]]),
            vec![(G * 0.5, 0.52), (G, 1.0)],
        ],
        'U' => vec![join(vec![vec![(0.0, 0.0)], arc((r, 1.0 - r), r, r, PI, 0.0), vec![(G, 0.0)]])],
        'G' => vec![join(vec![
            arc((r, r), r, r, -0.35, -PI),
            arc((r, 1.0 - r), r, r, PI, 0.0),
            vec![(G, 0.56), (G * 0.52, 0.56)],
        ])],
        'L' => vec![vec![(0.0, 0.0), (0.0, 1.0), (G, 1.0)]],
        'S' => vec![join(vec![arc((r, 0.26), r, 0.26, -0.3, -1.5 * PI), arc((r, 0.76), r, 0.24, -Q, PI + 0.3)])],
        'H' => vec![vec![(0.0, 0.0), (0.0, 1.0)], vec![(G, 0.0), (G, 1.0)], vec![(0.0, 0.5), (G, 0.5)]],
        'E' => vec![vec![(G, 0.0), (0.0, 0.0), (0.0, 1.0), (G, 1.0)], vec![(0.0, 0.5), (G * 0.82, 0.5)]],
        'D' => vec![join(vec![
            vec![(0.0, 0.0), (0.0, 1.0)],
            arc((G - r, 1.0 - r), r, r, Q, 0.0),
            arc((G - r, r), r, r, 0.0, -Q),
            vec![(0.0, 0.0)],
        ])],
        'O' => vec![join(vec![arc((r, r), r, r, PI, 2.0 * PI), arc((r, 1.0 - r), r, r, 0.0, PI), vec![(0.0, r)]])],
        'C' => vec![join(vec![arc((r, r), r, r, -0.35, -PI), arc((r, 1.0 - r), r, r, PI, 0.35)])],
        _ => vec![],
    }
}

/// `text` in strokes, its caps `cap` tall from `top`; where it ends.
fn word(out: &mut Vec<([u8; 3], Shape)>, colour: [u8; 3], text: &str, mut x: f32, top: f32, cap: f32) -> f32 {
    let r = WEIGHT * cap / 2.0;
    // The strokes' round ends stand out of the box by `r`: inset it.
    let (inner, gap) = (cap - 2.0 * r, 0.16 * cap);
    let mut end = x;
    for c in text.chars() {
        if c == ' ' {
            x += 0.42 * cap;
            continue;
        }
        let w = G * inner;
        for line in glyph(c) {
            for p in line.windows(2) {
                let pt = |(gx, gy): (f32, f32)| (x + r + gx * inner, top + r + gy * inner);
                out.push((colour, Shape::Stroke { a: pt(p[0]), b: pt(p[1]), r }));
            }
        }
        end = x + w + 2.0 * r;
        x = end + gap;
    }
    end
}

/// The card's pixels, RGB, row by row.
fn draw() -> Vec<u8> {
    let mut px: Vec<u8> = PAPER.repeat((W * H) as usize);
    for (colour, shape) in scene() {
        let (x0, y0, x1, y1) = shape.bounds();
        let (x0, y0) = ((x0 - 1.0).max(0.0) as u32, (y0 - 1.0).max(0.0) as u32);
        let (x1, y1) = (((x1 + 2.0) as u32).min(W), ((y1 + 2.0) as u32).min(H));
        for y in y0..y1 {
            for x in x0..x1 {
                // Coverage from the distance: a pixel's width of edge.
                let a = (0.5 - shape.distance((x as f32 + 0.5, y as f32 + 0.5))).clamp(0.0, 1.0);
                if a == 0.0 {
                    continue;
                }
                let i = ((y * W + x) * 3) as usize;
                for k in 0..3 {
                    let (old, new) = (px[i + k] as f32, colour[k] as f32);
                    px[i + k] = (old + (new - old) * a).round() as u8;
                }
            }
        }
    }
    px
}

// ---------------------------------------------------------------- PNG

/// An 8-bit RGB PNG of `rgb`. Each row is Sub-filtered, so flat colour is
/// runs of zeros, and the zlib stream is one fixed-Huffman block whose only
/// matches are runs (distance 1): small for a card that is mostly paper,
/// with no compression crate.
pub fn encode(w: u32, h: u32, rgb: &[u8]) -> Vec<u8> {
    let stride = (w * 3) as usize;
    let mut raw = Vec::with_capacity((stride + 1) * h as usize);
    for row in rgb.chunks(stride) {
        raw.push(1);
        raw.extend(row.iter().enumerate().map(|(i, &b)| if i < 3 { b } else { b.wrapping_sub(row[i - 3]) }));
    }
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = !0u32;
    for &x in b {
        c ^= x as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// Bits, least significant first, as deflate packs them.
#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, n: u32) {
        self.acc |= v << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code, which deflate writes most significant bit first.
    fn code(&mut self, v: u32, n: u32) {
        self.put(v.reverse_bits() >> (32 - n), n);
    }

    /// A literal byte or end of block (256), in the fixed code.
    fn lit(&mut self, v: u32) {
        match v {
            0..=143 => self.code(0x30 + v, 8),
            144..=255 => self.code(0x190 + v - 144, 9),
            256..=279 => self.code(v - 256, 7),
            _ => self.code(0xc0 + v - 280, 8),
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// Deflate's length codes from 257: the first length of each, and its extra bits.
const LENGTHS: [(u32, u32); 29] = [
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 1),
    (13, 1),
    (15, 1),
    (17, 1),
    (19, 2),
    (23, 2),
    (27, 2),
    (31, 2),
    (35, 3),
    (43, 3),
    (51, 3),
    (59, 3),
    (67, 4),
    (83, 4),
    (99, 4),
    (115, 4),
    (131, 5),
    (163, 5),
    (195, 5),
    (227, 5),
    (258, 0),
];

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut b = Bits::default();
    // The zlib header (deflate, 32K window, no dictionary), then one
    // final block in the fixed code.
    b.put(0x78, 8);
    b.put(0x01, 8);
    b.put(1, 1);
    b.put(1, 2);
    let mut i = 0;
    while i < data.len() {
        let run = if i == 0 { 0 } else { data[i..].iter().take(258).take_while(|&&x| x == data[i - 1]).count() };
        if run >= 3 {
            let k = LENGTHS.iter().rposition(|&(base, _)| base as usize <= run).unwrap();
            let (base, extra) = LENGTHS[k];
            b.lit(257 + k as u32);
            b.put(run as u32 - base, extra);
            // Distance 1: code 0, five bits, no extra.
            b.code(0, 5);
            i += run;
        } else {
            b.lit(data[i] as u32);
            i += 1;
        }
    }
    b.lit(256);
    let mut out = b.finish();
    let (mut s1, mut s2) = (1u32, 0u32);
    for &x in data {
        s1 = (s1 + x as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    out.extend(((s2 << 16) | s1).to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inflates what [`zlib`] writes (one fixed-Huffman block), to read the
    /// card back.
    fn inflate(z: &[u8]) -> Vec<u8> {
        assert_eq!(&z[..2], &[0x78, 0x01]);
        let d = &z[2..z.len() - 4];
        let mut pos = 0usize;
        let mut bit = |n: u32| -> u32 {
            let mut v = 0;
            for k in 0..n {
                v |= (((d[pos / 8] >> (pos % 8)) & 1) as u32) << k;
                pos += 1;
            }
            v
        };
        assert_eq!((bit(1), bit(2)), (1, 1), "a final fixed block");
        let mut out: Vec<u8> = Vec::new();
        loop {
            // Read a code most significant bit first, 7 to 9 bits.
            let mut c = 0;
            let mut n = 0;
            let sym = loop {
                c = (c << 1) | bit(1);
                n += 1;
                match (n, c) {
                    (7, 0..=0x17) => break 256 + c,
                    (8, 0x30..=0xbf) => break c - 0x30,
                    (8, 0xc0..=0xc7) => break 280 + c - 0xc0,
                    (9, 0x190..=0x1ff) => break 144 + c - 0x190,
                    (9, _) => panic!("bad code {c:#x}"),
                    _ => {}
                }
            };
            match sym {
                0..=255 => out.push(sym as u8),
                256 => break,
                _ => {
                    let (base, extra) = LENGTHS[(sym - 257) as usize];
                    let len = base + bit(extra);
                    let mut dc = 0;
                    for _ in 0..5 {
                        dc = (dc << 1) | bit(1);
                    }
                    assert_eq!(dc, 0, "only distance 1");
                    for _ in 0..len {
                        out.push(*out.last().unwrap());
                    }
                }
            }
        }
        let a = u32::from_be_bytes(z[z.len() - 4..].try_into().unwrap());
        let (mut s1, mut s2) = (1u32, 0u32);
        for &x in &out {
            s1 = (s1 + x as u32) % 65521;
            s2 = (s2 + s1) % 65521;
        }
        assert_eq!(a, (s2 << 16) | s1, "adler32");
        out
    }

    /// The pixels of a PNG [`encode`] wrote, checking its chunks.
    fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let (mut at, mut size, mut idat) = (8, (0, 0), Vec::new());
        while at < png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let body = &png[at + 4..at + 8 + len];
            let crc = u32::from_be_bytes(png[at + 8 + len..at + 12 + len].try_into().unwrap());
            assert_eq!(crc, crc32(body), "a chunk's CRC");
            match &body[..4] {
                b"IHDR" => {
                    size = (
                        u32::from_be_bytes(body[4..8].try_into().unwrap()),
                        u32::from_be_bytes(body[8..12].try_into().unwrap()),
                    );
                    assert_eq!(&body[12..17], &[8, 2, 0, 0, 0]);
                }
                b"IDAT" => idat.extend(&body[4..]),
                _ => {}
            }
            at += 12 + len;
        }
        let raw = inflate(&idat);
        let stride = (size.0 * 3) as usize;
        let mut px = Vec::new();
        for row in raw.chunks(stride + 1) {
            assert_eq!(row[0], 1, "Sub");
            let start = px.len();
            for (i, &b) in row[1..].iter().enumerate() {
                let left = if i < 3 { 0 } else { px[start + i - 3] };
                px.push(b.wrapping_add(left));
            }
        }
        (size.0, size.1, px)
    }

    #[test]
    fn a_small_picture_survives_the_round_trip() {
        // Noise, then runs of every length a match can have.
        let mut rgb: Vec<u8> = (0..7 * 5 * 3).map(|i| (i * 37 % 251) as u8).collect();
        assert_eq!(decode(&encode(7, 5, &rgb)), (7, 5, rgb.clone()));
        rgb = [0x1a, 0x1b, 0x18].repeat(400 * 3);
        let len = rgb.len();
        for n in 1..300 {
            rgb[n * 7 % len] = n as u8;
        }
        assert_eq!(decode(&encode(400, 3, &rgb)), (400, 3, rgb));
    }

    #[test]
    fn the_card_is_the_leaf_and_the_name_on_paper() {
        let png = png();
        assert!(png.len() < 200 * 1024, "{} bytes", png.len());
        let (w, h, px) = decode(png);
        assert_eq!((w, h), (W, H));
        let at = |x: u32, y: u32| -> [u8; 3] { px[((y * W + x) * 3) as usize..][..3].try_into().unwrap() };
        assert_eq!(at(10, 10), PAPER, "a corner is paper");
        // The leaf's top, the middle of its first ellipse (the site's 20, 8.5).
        assert_eq!(at(118 + 180, 135 + 76), LEAF);
        // The rule under the name: pepper, then leaf.
        assert_eq!(at(580, 353), PEPPER);
        assert_eq!(at(700, 353), LEAF);
        // The name's first stroke, the A's crossbar.
        assert!(px.chunks(3).any(|p| p == INK), "the name is in ink");
    }
}
