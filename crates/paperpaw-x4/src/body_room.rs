//! How an answer's body in memory takes its room (`core.net`, `shells/xteink`
//! `http.rs`). A `Vec` that grows by itself doubles: a 16 KiB body that gets
//! one more piece asks for 32 KiB, and the old and the new buffer exist at
//! once. The room a request may take is already reserved from the device's
//! network pool (`max_body`), so the buffer is taken once, up to the most the
//! request may keep, or in exact steps for a limit that is large; never past
//! the limit, never by doubling.

/// The most reserved at once for the first piece; a larger limit is reached in
/// steps of [`STEP`].
pub const FIRST: usize = 32 * 1024;

/// How much a body that outgrew what it has takes more, for a limit past
/// [`FIRST`].
pub const STEP: usize = 16 * 1024;

/// How many bytes of a piece of `piece` bytes fit in a body of `len` bytes
/// that keeps at most `limit`.
#[must_use]
pub const fn fit(len: usize, piece: usize, limit: usize) -> usize {
    let room = limit.saturating_sub(len);
    if piece < room { piece } else { room }
}

/// The capacity to reserve exactly (`Vec::try_reserve_exact` for the
/// difference to `len`) before `piece` more bytes (already cut with [`fit`])
/// are added to a body of `len` bytes that has `capacity`, or `None` where
/// they fit. The result holds `len + piece`, never exceeds `limit` (unless
/// the body already holds more), and is never more than `FIRST` or `STEP`
/// beyond what the body had.
#[must_use]
pub const fn capacity_for(
    capacity: usize,
    len: usize,
    piece: usize,
    limit: usize,
) -> Option<usize> {
    let need = len.saturating_add(piece);
    if need <= capacity {
        return None;
    }
    let step = if capacity == 0 { FIRST } else { STEP };
    let mut target = capacity.saturating_add(step);
    if target < need {
        target = need;
    }
    if target > limit {
        target = limit;
    }
    // `need <= limit` whenever the piece was cut with `fit`.
    if target < need {
        target = need;
    }
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body fed in pieces: the buffer is taken like `try_reserve_exact`
    /// does, and the largest buffer that ever existed at once is counted.
    fn feed(limit: usize, piece: usize, total: usize) -> (usize, usize, usize) {
        let (mut len, mut cap) = (0usize, 0usize);
        let (mut takes, mut peak) = (0usize, 0usize);
        let mut left = total;
        while left > 0 {
            let n = fit(len, piece.min(left), limit);
            left -= piece.min(left);
            if n == 0 {
                continue;
            }
            if let Some(target) = capacity_for(cap, len, n, limit) {
                assert!(target >= len + n);
                assert!(target <= limit.max(len + n));
                takes += 1;
                // Old and new buffer exist together while it is copied.
                peak = peak.max(cap + target);
                cap = target;
            }
            len += n;
        }
        assert!(len <= limit);
        assert!(len <= cap);
        (takes, cap, peak)
    }

    #[test]
    fn a_limit_up_to_first_is_taken_once_and_never_grows() {
        // 16 KiB pieces into a 23 KiB limit: the doubled Vec asked for 32 KiB.
        let (takes, cap, peak) = feed(23 * 1024, 4096, 64 * 1024);
        assert_eq!((takes, cap), (1, 23 * 1024));
        assert_eq!(peak, 23 * 1024);
    }

    #[test]
    fn never_asks_for_more_than_the_limit() {
        for limit in [1, 100, 4096, 16 * 1024, 23 * 1024, 32 * 1024, 100_000] {
            for piece in [1, 7, 1460, 4096, 16 * 1024] {
                let (_, cap, _) = feed(limit, piece, 200_000);
                assert!(cap <= limit, "limit {limit} piece {piece} cap {cap}");
            }
        }
    }

    #[test]
    fn a_large_limit_grows_in_exact_steps_not_by_doubling() {
        let limit = 200_000;
        let (takes, cap, _) = feed(limit, 4096, limit);
        assert_eq!(cap, limit);
        // 32 KiB, then 16 KiB steps (the last cut at the limit).
        assert_eq!(takes, 1 + (limit - FIRST).div_ceil(STEP));
        // No step is more than STEP past what the body had.
        let mut cap = FIRST;
        while cap < limit {
            let next = capacity_for(cap, cap, 1, limit).unwrap();
            assert!(next <= cap + STEP);
            cap = next;
        }
    }

    #[test]
    fn a_piece_that_fits_takes_nothing() {
        assert_eq!(capacity_for(4096, 1000, 3096, 8192), None);
        assert_eq!(capacity_for(0, 0, 0, 8192), None);
    }

    #[test]
    fn a_piece_is_cut_at_the_limit() {
        assert_eq!(fit(10, 100, 50), 40);
        assert_eq!(fit(50, 100, 50), 0);
        assert_eq!(fit(60, 100, 50), 0);
        assert_eq!(fit(0, 5, 50), 5);
    }

    #[test]
    fn a_zero_limit_takes_nothing() {
        assert_eq!(fit(0, 10, 0), 0);
        assert_eq!(capacity_for(0, 0, 0, 0), None);
    }
}
