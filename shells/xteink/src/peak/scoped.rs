//! Allocation-free counters for one explicitly bounded measurement.

/// One scope's heap measurements. Heap-used values include allocator overhead;
/// request/freed byte counters use the caller's allocation layout sizes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScopeSample {
    pub baseline_used: usize,
    pub end_used: usize,
    pub peak_used: usize,
    pub allocations: usize,
    pub requested_bytes: usize,
    pub failed_allocations: usize,
    pub deallocations: usize,
    pub freed_requested_bytes: usize,
    /// A saturated counter makes the scope unsuitable for exact comparison.
    pub overflowed: bool,
}

/// Updated only while the caller holds its instrumentation lock.
#[derive(Clone, Copy)]
pub(super) struct Recorder {
    sample: ScopeSample,
}

impl Recorder {
    pub(super) const fn new(baseline_used: usize) -> Self {
        Self {
            sample: ScopeSample {
                baseline_used,
                end_used: baseline_used,
                peak_used: baseline_used,
                allocations: 0,
                requested_bytes: 0,
                failed_allocations: 0,
                deallocations: 0,
                freed_requested_bytes: 0,
                overflowed: false,
            },
        }
    }

    pub(super) fn allocation(&mut self, used: usize, ptr: usize, size: usize) {
        self.sample.peak_used = self.sample.peak_used.max(used);
        if ptr == 0 {
            add(
                &mut self.sample.failed_allocations,
                1,
                &mut self.sample.overflowed,
            );
        } else {
            add(&mut self.sample.allocations, 1, &mut self.sample.overflowed);
            add(
                &mut self.sample.requested_bytes,
                size,
                &mut self.sample.overflowed,
            );
        }
    }

    pub(super) fn deallocation(&mut self, ptr: usize, size: usize) {
        if ptr != 0 {
            add(
                &mut self.sample.deallocations,
                1,
                &mut self.sample.overflowed,
            );
            add(
                &mut self.sample.freed_requested_bytes,
                size,
                &mut self.sample.overflowed,
            );
        }
    }

    pub(super) fn finish(mut self, end_used: usize) -> ScopeSample {
        self.sample.end_used = end_used;
        self.sample.peak_used = self.sample.peak_used.max(end_used);
        self.sample
    }
}

fn add(counter: &mut usize, amount: usize, overflowed: &mut bool) {
    match counter.checked_add(amount) {
        Some(value) => *counter = value,
        None => {
            *counter = usize::MAX;
            *overflowed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_used_and_requested_traffic_are_distinct() {
        let mut scope = Recorder::new(100);
        scope.allocation(132, 1, 24);
        scope.allocation(164, 2, 24);
        scope.deallocation(1, 24);
        let sample = scope.finish(132);
        assert_eq!(
            (sample.baseline_used, sample.end_used, sample.peak_used),
            (100, 132, 164)
        );
        assert_eq!((sample.allocations, sample.requested_bytes), (2, 48));
        assert_eq!(
            (sample.deallocations, sample.freed_requested_bytes),
            (1, 24)
        );
        assert_eq!(sample.failed_allocations, 0);
        assert!(!sample.overflowed);
    }

    #[test]
    fn failure_is_counted_without_successful_request_bytes() {
        let mut scope = Recorder::new(200);
        scope.allocation(200, 0, 8192);
        scope.deallocation(0, 8192);
        let sample = scope.finish(200);
        assert_eq!(sample.failed_allocations, 1);
        assert_eq!((sample.allocations, sample.requested_bytes), (0, 0));
        assert_eq!((sample.deallocations, sample.freed_requested_bytes), (0, 0));
        assert_eq!(sample.peak_used, 200);
    }

    #[test]
    fn dropping_preexisting_allocations_can_end_below_baseline() {
        let mut scope = Recorder::new(200);
        scope.deallocation(1, 160);
        scope.allocation(80, 2, 32);
        let sample = scope.finish(80);
        assert_eq!(sample.peak_used, 200);
        assert_eq!(sample.end_used, 80);
        assert_eq!(sample.freed_requested_bytes, 160);
    }

    #[test]
    fn saturation_is_explicit() {
        let mut scope = Recorder::new(0);
        scope.allocation(1, 1, usize::MAX);
        scope.allocation(2, 2, 1);
        let sample = scope.finish(2);
        assert_eq!(sample.requested_bytes, usize::MAX);
        assert!(sample.overflowed);
    }
}
