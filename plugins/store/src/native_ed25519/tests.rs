//! The lean check against the kernel's (`ed25519-compact`), which signs here.

use alloc::vec::Vec;

use paperpaw_digest::hex;
use paperpaw_native::signature::{public_key, sign_message, verify_message};
use proptest::prelude::*;

use super::*;

fn fe(small: i32) -> Fe {
    let mut out = ZERO;
    out[0] = small;
    out
}

fn sha512(pieces: &[&[u8]]) -> [u8; 64] {
    let mut hash = Sha512::new();
    for piece in pieces {
        hash.update(piece);
    }
    hash.finish()
}

/// `1 / a`: `a` to the power 2^255 - 21.
fn inverse(a: &Fe) -> Fe {
    let mut out = *a;
    for bit in (0..=253).rev() {
        out = square(&out);
        if bit != 2 && bit != 4 {
            out = times(&out, a);
        }
    }
    out
}

fn encoded(p: &Point) -> [u8; 32] {
    let z = inverse(&p.z);
    let mut out = bytes(&times(&p.y, &z));
    out[31] |= (bytes(&times(&p.x, &z))[0] & 1) << 7;
    out
}

fn point(encoded: &[u8; 32]) -> Point {
    let mut p = NEUTRAL;
    assert!(negated(&mut p, encoded));
    p.x = minus(&ZERO, &p.x);
    p.t = minus(&ZERO, &p.t);
    p
}

/// `scalar` (below the group order) times the base point.
fn times_base(scalar: &[u8; 32]) -> Point {
    let mut out = NEUTRAL;
    combine(&mut out, &[0; 32], &NEUTRAL, scalar);
    out
}

/// `h a + r` below the group order.
fn answer(h: &[u8; 32], a: &[u8; 32], r: &[u8; 32]) -> [u8; 32] {
    let mut wide = [0u32; 65];
    for (i, x) in h.iter().enumerate() {
        for (j, y) in a.iter().enumerate() {
            wide[i + j] += u32::from(*x) * u32::from(*y);
        }
    }
    for (limb, byte) in wide.iter_mut().zip(r) {
        *limb += u32::from(*byte);
    }
    let mut out = [0; 64];
    for i in 0..64 {
        wide[i + 1] += wide[i] >> 8;
        out[i] = wide[i] as u8;
    }
    reduced(&out)
}

/// A point of order 8.
const EIGHTH: &str = "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa";

/// A signature by the secret number made from `seed` under the key `a B`
/// plus `key_part`, with `r B` plus `r_part` as its `R`.
fn signed_with_parts(
    seed: &[u8],
    message: &[u8],
    key_part: &Point,
    r_part: &Point,
) -> ([u8; 32], [u8; 64]) {
    let a = reduced(&sha512(&[seed, b"key"]));
    let r = reduced(&sha512(&[seed, message]));
    let mut key = times_base(&a);
    add(&mut key, key_part);
    let key = encoded(&key);
    let mut big_r = times_base(&r);
    add(&mut big_r, r_part);
    let big_r = encoded(&big_r);
    let s = answer(&challenge(&big_r, &key, message), &a, &r);
    let mut signature = [0; 64];
    signature[..32].copy_from_slice(&big_r);
    signature[32..].copy_from_slice(&s);
    (key, signature)
}

/// What both checks say, which must be the same.
fn both(key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let lean = verify(key, message, signature);
    assert_eq!(
        lean,
        verify_message(key, message, signature),
        "key {} signature {}",
        hex::encode(key),
        hex::encode(signature)
    );
    lean
}

#[test]
fn the_constants_are_the_curves() {
    assert!(same(&times(&D, &fe(121_666)), &fe(-121_665)));
    assert!(same(&D2, &plus(&D, &D)));
    assert!(same(&square(&SQRT_M1), &fe(-1)));
    assert!(same(&times(&BASE_Y, &fe(5)), &fe(4)));
    // -x^2 + y^2 = 1 + d x^2 y^2, and the x that is even.
    let (xx, yy) = (square(&BASE_X), square(&BASE_Y));
    assert!(same(
        &minus(&yy, &xx),
        &plus(&ONE, &times(&D, &times(&xx, &yy)))
    ));
    assert_eq!(bytes(&BASE_X)[0] & 1, 0);
    assert!(same(&BASE.t, &times(&BASE_X, &BASE_Y)));
    // Twice a point is the point added to itself.
    let (mut twice, mut sum) = (BASE, BASE);
    double(&mut twice);
    add(&mut sum, &BASE);
    assert_eq!(encoded(&twice), encoded(&sum));
    assert!(same(&times(&twice.x, &twice.y), &times(&twice.t, &twice.z)));
    // The group order times the base point is the neutral point; the order is
    // not below itself.
    let mut order_less_one = ORDER;
    order_less_one[0] -= 1;
    let mut all = times_base(&order_less_one);
    add(&mut all, &times_base(&bytes(&ONE)));
    assert_eq!(encoded(&all), bytes(&ONE));
    assert!(below_order(&order_less_one) && !below_order(&ORDER));
    let eighth = point(&hex::decode_array(EIGHTH).unwrap());
    assert!(small_order(&eighth));
    let mut fourth = eighth;
    double(&mut fourth);
    double(&mut fourth);
    assert_ne!(encoded(&fourth), bytes(&ONE));
}

#[test]
fn sha_512_is_the_standards() {
    assert_eq!(
        hex::encode(&sha512(&[b"abc"])),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
         2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
    // Two blocks, in pieces that end inside and at a block's end.
    let long: Vec<u8> = (0..200).collect();
    for cut in [0, 1, 111, 112, 127, 128, 129, 200] {
        let (first, rest) = long.split_at(cut);
        assert_eq!(
            hex::encode(&sha512(&[first, rest])),
            "986058e9895e2c2ab8f9e8cbdf801db12a44842a56a91d5a4e87b1fc98b29372\
             2c4664142e42c3c551ff898646268cd92b84ed230b8c94bed7798d4f27cd7465",
            "{cut}"
        );
    }
}

#[test]
fn a_hash_is_brought_below_the_group_order() {
    assert_eq!(
        hex::encode(&reduced(&[0xff; 64])),
        "000f9c44e31106a447938568a71b0ed065bef517d273ecce3d9a307c1b419903"
    );
    assert_eq!(
        hex::encode(&reduced(&sha512(&[b"paperpaw"]))),
        "b73aba0ef34b905b31ad04a8cf3bdc373f9a51928b92d03e4e3a656795ecbf07"
    );
    let mut order = [0; 64];
    order[..32].copy_from_slice(&ORDER);
    assert_eq!(reduced(&order), [0; 32]);
    order[0] -= 1;
    assert_eq!(reduced(&order)[..], order[..32]);
}

/// RFC 8032, 7.1, test 1 and test 2.
#[test]
fn the_standards_signatures_hold() {
    let key = hex::decode_array("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        .unwrap();
    let signature = hex::decode_array(
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155\
         5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    )
    .unwrap();
    assert!(both(&key, b"", &signature));
    assert!(!both(&key, b"\0", &signature));
    let key = hex::decode_array("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c")
        .unwrap();
    let signature = hex::decode_array(
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da\
         085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    )
    .unwrap();
    assert!(both(&key, &[0x72], &signature));
}

#[test]
fn an_answer_at_or_above_the_group_order_is_refused() {
    let seed = [9; 32];
    let key = public_key(&seed);
    let signature = sign_message(b"list", &seed);
    assert!(both(&key, b"list", &signature));
    // The same answer plus the group order: the same point, not canonical.
    let mut longer = signature;
    let mut over = 0;
    for (byte, order) in longer[32..].iter_mut().zip(ORDER) {
        let sum = u16::from(*byte) + u16::from(order) + over;
        *byte = sum as u8;
        over = sum >> 8;
    }
    assert_eq!(over, 0);
    assert!(!both(&key, b"list", &longer));
    let mut order = signature;
    order[32..].copy_from_slice(&ORDER);
    assert!(!both(&key, b"list", &order));
}

#[test]
fn points_of_a_small_order_and_not_canonical_ones_are_refused() {
    let seed = [9; 32];
    let honest = public_key(&seed);
    let signature = sign_message(b"list", &seed);
    let mut refused = Vec::new();
    for sign in [0, 0x80] {
        // y = 0, 1, -1, the two of order 8, the prime and above (not canonical).
        for y in [
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0100000000000000000000000000000000000000000000000000000000000000",
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
            "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
            "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        ] {
            let mut point: [u8; 32] = hex::decode_array(y).unwrap();
            point[31] |= sign;
            refused.push(point);
        }
    }
    for point in refused {
        assert!(
            !both(&point, b"list", &signature),
            "{}",
            hex::encode(&point)
        );
        let mut with_r = signature;
        with_r[..32].copy_from_slice(&point);
        assert!(!both(&honest, b"list", &with_r), "{}", hex::encode(&point));
        // Whatever the answer: nothing holds under such a key.
        with_r[32..].fill(0);
        assert!(!both(&point, b"list", &with_r), "{}", hex::encode(&point));
    }
    // A y no point of the curve has.
    let mut off = [0; 32];
    off[0] = 2;
    let mut none = NEUTRAL;
    assert!(!negated(&mut none, &off));
    assert!(!both(&off, b"list", &signature));
}

#[test]
fn a_key_or_an_r_with_a_small_part_is_judged_as_the_kernel_does() {
    let eighth = point(&hex::decode_array(EIGHTH).unwrap());
    let mut holds = 0;
    for (n, message) in [&b"one"[..], b"two", b"three", b"four", b"five", b"six"]
        .into_iter()
        .enumerate()
    {
        let seed = [n as u8; 4];
        let (key, signature) = signed_with_parts(&seed, message, &NEUTRAL, &NEUTRAL);
        assert!(both(&key, message, &signature));
        for (key_part, r_part) in [(&eighth, &NEUTRAL), (&NEUTRAL, &eighth), (&eighth, &eighth)] {
            let (mixed, signature) = signed_with_parts(&seed, message, key_part, r_part);
            holds += u32::from(both(&mixed, message, &signature));
            assert!(!both(&mixed, b"another", &signature));
            // Under the plain key it holds where the key had no part added.
            assert_eq!(both(&key, message, &signature), mixed == key);
        }
    }
    // What is left over is of a small order, which four times makes x 0.
    assert_eq!(holds, 18);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn a_signature_holds_and_any_change_breaks_it_as_for_the_kernel(
        seed in any::<[u8; 32]>(),
        message in proptest::collection::vec(any::<u8>(), 0..300),
        at in 0usize..(32 + 64) * 8,
        cut in any::<prop::sample::Index>(),
    ) {
        let key = public_key(&seed);
        let signature = sign_message(&message, &seed);
        prop_assert!(both(&key, &message, &signature));
        let (mut other_key, mut other) = (key, signature);
        match at / 8 {
            byte @ 0..32 => other_key[byte] ^= 1 << (at % 8),
            byte => other[byte - 32] ^= 1 << (at % 8),
        }
        prop_assert!(!both(&other_key, &message, &other));
        if !message.is_empty() {
            let mut changed = message.clone();
            changed[cut.index(message.len())] ^= 1;
            prop_assert!(!both(&key, &changed, &signature));
            prop_assert!(!both(&key, &message[1..], &signature));
        }
    }

    #[test]
    fn whatever_the_bytes_both_say_the_same(
        key in any::<[u8; 32]>(),
        r in any::<[u8; 32]>(),
        s in any::<[u8; 32]>(),
        message in proptest::collection::vec(any::<u8>(), 0..64),
        seed in any::<[u8; 32]>(),
    ) {
        let mut signature = [0; 64];
        signature[..32].copy_from_slice(&r);
        signature[32..].copy_from_slice(&s);
        both(&key, &message, &signature);
        // A real key and a real R with it, so that the product is reached.
        signature[31] &= 0x7f;
        signature[63] &= 0x0f;
        both(&public_key(&seed), &message, &signature);
        signature[..32].copy_from_slice(&public_key(&key));
        both(&public_key(&seed), &message, &signature);
    }
}
