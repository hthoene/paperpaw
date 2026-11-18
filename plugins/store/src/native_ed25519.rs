//! An Ed25519 check that fits a plugin file's stack: a file takes no frame
//! of 2,048 B or more (`abi/ppn5.md`), and the check PaperPaw's kernel makes
//! (`paperpaw_sdk::native::signature::verify_message`) takes one of about
//! 4 KiB. A store packed as a file checks a community source's signature
//! ([`crate::native_sources`]) with this one; the firmware's own store keeps
//! the kernel's.
//!
//! It says yes and no exactly where the kernel's does (`ed25519-compact`,
//! tested against it): a signature whose `S` is the group order or more, a
//! key or an `R` that is no canonical point or has a small order are
//! refused, and what holds is `4 (S B - h A - R) = 0`, `h` being
//! SHA-512(`R`, key, message) below the group order.
//!
//! Small before fast: a field element is 16 limbs of 16 bits (64 B), a point
//! four of them, the two products are added up bit by bit together (Shamir),
//! and nothing is kept secret here, so nothing takes constant time. The
//! functions with large values have frames of their own and use the same few
//! values again, so the deepest path of a check stays near 2.5 KiB; one check
//! is about 4,800 field products.

#[cfg(test)]
mod tests;

/// A number below 2^255 - 19, as 16 limbs of 16 bits, the lowest first. A
/// product's limbs lie in `-38..2^16`; sums and differences of two or three
/// of them are multiplied without a carry between.
type Fe = [i32; 16];

const ZERO: Fe = [0; 16];
const ONE: Fe = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
/// The curve's `d`, -121665/121666.
const D: Fe = [
    0x78a3, 0x1359, 0x4dca, 0x75eb, 0xd8ab, 0x4141, 0x0a4d, 0x0070, 0xe898, 0x7779, 0x4079, 0x8cc7,
    0xfe73, 0x2b6f, 0x6cee, 0x5203,
];
/// Twice [`D`].
const D2: Fe = [
    0xf159, 0x26b2, 0x9b94, 0xebd6, 0xb156, 0x8283, 0x149a, 0x00e0, 0xd130, 0xeef3, 0x80f2, 0x198e,
    0xfce7, 0x56df, 0xd9dc, 0x2406,
];
/// A square root of -1.
const SQRT_M1: Fe = [
    0xa0b0, 0x4a0e, 0x1b27, 0xc4ee, 0xe478, 0xad2f, 0x1806, 0x2f43, 0xd7a7, 0x3dfb, 0x0099, 0x2b4d,
    0xdf0b, 0x4fc1, 0x2480, 0x2b83,
];
/// The base point.
const BASE_X: Fe = [
    0xd51a, 0x8f25, 0x2d60, 0xc956, 0xa7b2, 0x9525, 0xc760, 0x692c, 0xdc5c, 0xfdd6, 0xe231, 0xc0a4,
    0x53fe, 0xcd6e, 0x36d3, 0x2169,
];
const BASE_Y: Fe = [
    0x6658, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666,
    0x6666, 0x6666, 0x6666, 0x6666,
];
/// The base point, kept in the program.
static BASE: Point = Point {
    x: BASE_X,
    y: BASE_Y,
    z: ONE,
    t: [
        0xdda3, 0xa5b7, 0x8ab3, 0x6dde, 0x52f5, 0x7751, 0x9f80, 0x20f0, 0xe37d, 0x64ab, 0x4e8e,
        0x66ea, 0x7665, 0xd78b, 0x5f0f, 0x6787,
    ],
};
/// The group order, the lowest byte first.
const ORDER: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
];

fn plus(a: &Fe, b: &Fe) -> Fe {
    core::array::from_fn(|i| a[i] + b[i])
}

fn minus(a: &Fe, b: &Fe) -> Fe {
    core::array::from_fn(|i| a[i] - b[i])
}

/// `a - b` into `a`.
fn take(a: &mut Fe, b: &Fe) {
    for (limb, less) in a.iter_mut().zip(b) {
        *limb -= less;
    }
}

/// Brings every limb into `0..2^16`, what is above 2^256 folded in as 38
/// times itself; the lowest limb takes the last fold and may leave the range
/// by it.
#[inline(never)]
fn carry(limbs: &mut [i64; 16]) {
    for i in 0..16 {
        let over = limbs[i] >> 16;
        limbs[i] -= over << 16;
        if i < 15 {
            limbs[i + 1] += over;
        } else {
            limbs[0] += 38 * over;
        }
    }
}

#[inline(never)]
fn times(a: &Fe, b: &Fe) -> Fe {
    let mut wide = [0i64; 32];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            wide[i + j] += i64::from(*x) * i64::from(*y);
        }
    }
    let mut out = ZERO;
    if let [low, high] = wide.as_chunks_mut::<16>().0 {
        for (limb, above) in low.iter_mut().zip(high.iter()) {
            *limb += 38 * above;
        }
        carry(low);
        carry(low);
        // After two carries a limb is in `-38..2^16`.
        for (limb, kept) in out.iter_mut().zip(low.iter()) {
            *limb = *kept as i32;
        }
    }
    out
}

fn square(a: &Fe) -> Fe {
    times(a, a)
}

/// The 32 bytes of `a`, the one number below 2^255 - 19 it stands for.
#[inline(never)]
fn bytes(a: &Fe) -> [u8; 32] {
    let mut t = a.map(i64::from);
    carry(&mut t);
    carry(&mut t);
    carry(&mut t);
    // Twice: below 2^256 there are two of the prime at most.
    for _ in 0..2 {
        let mut less = [0i64; 16];
        let mut borrow = 0;
        for (i, limb) in less.iter_mut().enumerate() {
            let prime = match i {
                0 => 0xffed,
                15 => 0x7fff,
                _ => 0xffff,
            };
            *limb = t[i] - prime - borrow;
            borrow = (*limb >> 16) & 1;
            if i < 15 {
                *limb &= 0xffff;
            }
        }
        if borrow == 0 {
            t = less;
        }
    }
    let mut out = [0; 32];
    for (pair, limb) in out.as_chunks_mut::<2>().0.iter_mut().zip(t) {
        *pair = (limb as u16).to_le_bytes();
    }
    out
}

fn same(a: &Fe, b: &Fe) -> bool {
    bytes(a) == bytes(b)
}

/// `a` to the power (2^255 - 19 - 5) / 8, which is 2^252 - 3.
#[inline(never)]
fn power_252_3(a: &Fe) -> Fe {
    let mut out = *a;
    for bit in (0..=250).rev() {
        out = square(&out);
        if bit != 1 {
            out = times(&out, a);
        }
    }
    out
}

/// A point of the curve, `x = X/Z`, `y = Y/Z`, `T = XY/Z`.
#[derive(Clone, Copy)]
struct Point {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

const NEUTRAL: Point = Point {
    x: ZERO,
    y: ONE,
    z: ONE,
    t: ZERO,
};

/// The end of a sum and of a doubling alike.
fn set(p: &mut Point, e: &Fe, f: &Fe, g: &Fe, h: &Fe) {
    p.x = times(e, f);
    p.y = times(g, h);
    p.z = times(f, g);
    p.t = times(e, h);
}

/// `p + q` into `p`. The formula holds for every pair of points of this
/// curve.
#[inline(never)]
fn add(p: &mut Point, q: &Point) {
    let mut e = minus(&p.y, &p.x);
    let mut f = minus(&q.y, &q.x);
    let a = times(&e, &f);
    e = plus(&p.x, &p.y);
    f = plus(&q.x, &q.y);
    let b = times(&e, &f);
    e = times(&p.t, &q.t);
    let c = times(&e, &D2);
    e = times(&p.z, &q.z);
    let d = plus(&e, &e);
    e = minus(&b, &a);
    f = minus(&d, &c);
    let g = plus(&d, &c);
    let h = plus(&b, &a);
    set(p, &e, &f, &g, &h);
}

/// Twice `p` into `p`, whatever point of the curve it is.
#[inline(never)]
fn double(p: &mut Point) {
    let a = square(&p.x);
    let b = square(&p.y);
    let mut f = plus(&p.x, &p.y);
    let mut e = square(&f);
    take(&mut e, &a);
    take(&mut e, &b);
    let g = minus(&b, &a);
    let mut h = minus(&ZERO, &a);
    take(&mut h, &b);
    f = square(&p.z);
    let c = plus(&f, &f);
    f = minus(&g, &c);
    set(p, &e, &f, &g, &h);
}

/// Whether eight times `p` is the neutral point: four times it has `x` 0.
#[inline(never)]
fn small_order(p: &Point) -> bool {
    let mut q = *p;
    double(&mut q);
    double(&mut q);
    bytes(&q.x) == [0; 32]
}

/// The point `encoded` stands for, negated, into `out`; `false` where its
/// `y` is not below the prime or no point of the curve has it (`out` is then
/// no point).
#[inline(never)]
fn negated(out: &mut Point, encoded: &[u8; 32]) -> bool {
    let [low, middle @ .., high] = encoded;
    if *low >= 0xed && middle.iter().all(|byte| *byte == 0xff) && high & 0x7f == 0x7f {
        return false;
    }
    for (limb, pair) in out.y.iter_mut().zip(encoded.as_chunks::<2>().0) {
        *limb = i32::from(u16::from_le_bytes(*pair));
    }
    out.y[15] &= 0x7fff;
    out.z = ONE;
    // x^2 = u / v, u = y^2 - 1, v = d y^2 + 1; x = u v^3 (u v^7)^(2^252 - 3).
    let mut w = square(&out.y);
    let u = minus(&w, &ONE);
    let mut v = times(&w, &D);
    v[0] += 1;
    w = square(&v);
    let v3 = times(&w, &v);
    w = square(&v3);
    let mut x = times(&w, &v);
    w = times(&x, &u);
    x = power_252_3(&w);
    w = times(&x, &u);
    x = times(&w, &v3);
    for root in [&ONE, &SQRT_M1] {
        x = times(&x, root);
        w = square(&x);
        w = times(&w, &v);
        if same(&w, &u) {
            if (bytes(&x)[0] & 1) == (high >> 7) {
                x = minus(&ZERO, &x);
            }
            out.t = times(&x, &out.y);
            out.x = x;
            return true;
        }
    }
    false
}

/// [`negated`], for a key or a signature's `R`: no point of a small order
/// (so none with `x` 0 either).
fn negated_large(out: &mut Point, encoded: &[u8; 32]) -> bool {
    negated(out, encoded) && !small_order(out)
}

/// The 64 bytes of `hash` as a number, below the group order.
#[inline(never)]
fn reduced(hash: &[u8; 64]) -> [u8; 32] {
    let order = |at: usize| i64::from(ORDER[at]);
    let mut x = hash.map(i64::from);
    for i in (32..64).rev() {
        let mut over = 0;
        for j in i - 32..i - 12 {
            x[j] += over - 16 * x[i] * order(j + 32 - i);
            over = (x[j] + 128) >> 8;
            x[j] -= over << 8;
        }
        x[i - 12] += over;
        x[i] = 0;
    }
    let mut over = 0;
    for j in 0..32 {
        let top = x[31] >> 4;
        x[j] += over - top * order(j);
        over = x[j] >> 8;
        x[j] &= 255;
    }
    let mut out = [0; 32];
    for j in 0..32 {
        x[j] -= over * order(j);
        x[j + 1] += x[j] >> 8;
        out[j] = (x[j] & 255) as u8;
    }
    out
}

/// Whether `scalar` is below the group order.
fn below_order(scalar: &[u8; 32]) -> bool {
    for (byte, order) in scalar.iter().zip(ORDER).rev() {
        if *byte != order {
            return *byte < order;
        }
    }
    false
}

/// SHA-512's round constants.
const ROUNDS: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

/// SHA-512 of what is given piece by piece.
struct Sha512 {
    state: [u64; 8],
    block: [u8; 128],
    filled: usize,
    bytes: u64,
}

impl Sha512 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667f3bcc908,
                0xbb67ae8584caa73b,
                0x3c6ef372fe94f82b,
                0xa54ff53a5f1d36f1,
                0x510e527fade682d1,
                0x9b05688c2b3e6c1f,
                0x1f83d9abfb41bd6b,
                0x5be0cd19137e2179,
            ],
            block: [0; 128],
            filled: 0,
            bytes: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.bytes = self.bytes.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let free = &mut self.block[self.filled..];
            let (now, rest) = data.split_at(free.len().min(data.len()));
            free[..now.len()].copy_from_slice(now);
            self.filled += now.len();
            data = rest;
            if self.filled == 128 {
                self.round();
                self.filled = 0;
            }
        }
    }

    /// The block into the state.
    #[inline(never)]
    fn round(&mut self) {
        let mut w = [0u64; 16];
        for (word, eight) in w.iter_mut().zip(self.block.as_chunks::<8>().0) {
            *word = u64::from_be_bytes(*eight);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for (i, constant) in ROUNDS.iter().enumerate() {
            if i >= 16 {
                let (before, late) = (w[(i + 1) & 15], w[(i + 14) & 15]);
                w[i & 15] = w[i & 15]
                    .wrapping_add(before.rotate_right(1) ^ before.rotate_right(8) ^ (before >> 7))
                    .wrapping_add(w[(i + 9) & 15])
                    .wrapping_add(late.rotate_right(19) ^ late.rotate_right(61) ^ (late >> 6));
            }
            let one = h
                .wrapping_add(e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(*constant)
                .wrapping_add(w[i & 15]);
            let two = (a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));
            (h, g, f, e, d, c, b, a) =
                (g, f, e, d.wrapping_add(one), c, b, a, one.wrapping_add(two));
        }
        for (kept, new) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *kept = kept.wrapping_add(new);
        }
    }

    fn finish(mut self) -> [u8; 64] {
        let bits = self.bytes.wrapping_mul(8);
        self.update(&[0x80]);
        while self.filled != 112 {
            self.update(&[0]);
        }
        self.update(&[0; 8]);
        self.update(&bits.to_be_bytes());
        let mut out = [0; 64];
        for (eight, word) in out.as_chunks_mut::<8>().0.iter_mut().zip(self.state) {
            *eight = word.to_be_bytes();
        }
        out
    }
}

/// What a signature's `R`, the key and the message make of the key's part.
#[inline(never)]
fn challenge(r: &[u8; 32], key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut hash = Sha512::new();
    hash.update(r);
    hash.update(key);
    hash.update(message);
    reduced(&hash.finish())
}

fn bit(scalar: &[u8; 32], at: usize) -> bool {
    (scalar[at >> 3] >> (at & 7)) & 1 == 1
}

/// `h` times `minus_a` plus `s` times the base point into `sum`, both
/// numbers below the group order (253 bits).
#[inline(never)]
fn combine(sum: &mut Point, h: &[u8; 32], minus_a: &Point, s: &[u8; 32]) {
    let mut both = BASE;
    add(&mut both, minus_a);
    *sum = NEUTRAL;
    for at in (0..253).rev() {
        double(sum);
        match (bit(h, at), bit(s, at)) {
            (true, true) => add(sum, &both),
            (true, false) => add(sum, minus_a),
            (false, true) => add(sum, &BASE),
            (false, false) => {}
        }
    }
}

/// Whether `signature` is `key`'s over `message` (Ed25519), as
/// `paperpaw_sdk::native::signature::verify_message` says it.
#[inline(never)]
#[must_use]
pub fn verify(key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let (Some((r, _)), Some((_, s))) = (
        signature.split_first_chunk::<32>(),
        signature.split_last_chunk::<32>(),
    ) else {
        return false;
    };
    // The key's point, then `R`'s.
    let mut point = NEUTRAL;
    if !below_order(s) || !negated_large(&mut point, key) {
        return false;
    }
    let mut sum = NEUTRAL;
    combine(&mut sum, &challenge(r, key, message), &point, s);
    if !negated_large(&mut point, r) {
        return false;
    }
    add(&mut sum, &point);
    small_order(&sum)
}
