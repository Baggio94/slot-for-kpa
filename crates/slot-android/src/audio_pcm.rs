//! F9: allocation-free transfer from the bounded native PCM FIFO.
//!
//! Host-tested on Linux and used by the Android JNI consumer. The FIFO keeps
//! the real-core PCM order; this helper never changes samples or cadence.
use std::collections::VecDeque;

/// The Android audio worker uses a reused 4096-short Java array. Do not let
/// a future caller silently request larger chunks than the established JNI
/// reader; preserve its batching and wake-up behavior.
pub(crate) const PCM_READ_MAX_SHORTS: usize = 4096;

/// The exact production FIFO enqueue policy, with an observable count of
/// discarded *old* PCM shorts. Preserve beta8's queue limit, sample ordering
/// and behavior; the result is diagnostics-only and must never alter pacing.
pub(crate) fn enqueue_pcm_bounded(
    audio: &mut VecDeque<i16>, samples: &[i16], limit: usize,
) -> usize {
    let overflow = audio.len().saturating_add(samples.len()).saturating_sub(limit);
    let dropped = overflow.min(audio.len());
    for _ in 0..dropped {
        audio.pop_front();
    }
    audio.extend(samples.iter().copied());
    dropped
}

pub(crate) fn drain_pcm_into(audio: &mut VecDeque<i16>, destination: &mut [i16]) -> usize {
    let count = audio.len().min(destination.len()).min(PCM_READ_MAX_SHORTS);
    for (position, sample) in audio.drain(..count).enumerate() {
        destination[position] = sample;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_fifo_does_not_write_stale_data() {
        let mut q = VecDeque::new();
        let mut reused = [0x1234i16; 8];
        assert_eq!(drain_pcm_into(&mut q, &mut reused), 0);
        assert_eq!(reused, [0x1234; 8]);
    }

    #[test]
    fn chunked_read_preserves_pcm_order_and_exact_tail() {
        let mut q: VecDeque<i16> = (-200..5000).map(|n| n as i16).collect();
        let expected: Vec<i16> = q.iter().copied().collect();
        let mut reused = [i16::MIN; PCM_READ_MAX_SHORTS];
        let first = drain_pcm_into(&mut q, &mut reused);
        assert_eq!(first, PCM_READ_MAX_SHORTS);
        assert_eq!(&reused[..first], &expected[..first]);
        let second = drain_pcm_into(&mut q, &mut reused);
        assert_eq!(second, expected.len() - first);
        assert_eq!(&reused[..second], &expected[first..]);
        assert!(q.is_empty());
    }

    #[test]
    fn wrapped_fifo_and_small_destination_preserve_order() {
        let mut q = VecDeque::with_capacity(8);
        q.extend([1, 2, 3, 4, 5, 6]);
        for _ in 0..4 { q.pop_front(); }
        q.extend([7, 8, 9, 10, 11, 12]);
        let mut reused = [0i16; 3];
        let mut result = Vec::new();
        while !q.is_empty() {
            let count = drain_pcm_into(&mut q, &mut reused);
            result.extend_from_slice(&reused[..count]);
        }
        assert_eq!(result, [5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn short_final_chunk_does_not_clear_or_replay_old_samples() {
        let mut q = VecDeque::from(vec![1i16, -1, 2, -2, 3, -3]);
        let mut reused = [99i16; 8];
        assert_eq!(drain_pcm_into(&mut q, &mut reused), 6);
        assert_eq!(&reused[..6], &[1, -1, 2, -2, 3, -3]);
        assert_eq!(&reused[6..], &[99, 99]);
        assert_eq!(drain_pcm_into(&mut q, &mut reused), 0);
        assert!(q.is_empty());
    }

    #[test]
    fn enqueue_without_overflow_preserves_unchanged_pcm_and_order() {
        let mut q = VecDeque::from(vec![1i16, -2, 3, -4]);
        assert_eq!(enqueue_pcm_bounded(&mut q, &[5, -6], 12), 0);
        assert_eq!(q.into_iter().collect::<Vec<_>>(), vec![1, -2, 3, -4, 5, -6]);
    }

    #[test]
    fn enqueue_on_wrapped_fifo_counts_only_evicted_old_pcm() {
        let mut q = VecDeque::with_capacity(8);
        q.extend([1i16, 2, 3, 4, 5, 6]);
        q.pop_front();
        q.pop_front();
        q.extend([7, 8]);
        assert_eq!(enqueue_pcm_bounded(&mut q, &[9, 10, 11, 12], 8), 2);
        assert_eq!(q.into_iter().collect::<Vec<_>>(), vec![5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn enqueue_huge_pcm_burst_keeps_original_beta8_semantics() {
        // Guard against inadvertently truncating source samples and changing
        // the audio waveform if a future core returns a large native chunk.
        let mut q = VecDeque::from(vec![1i16, 2]);
        assert_eq!(enqueue_pcm_bounded(&mut q, &[3, 4, 5, 6, 7], 3), 2);
        assert_eq!(q.into_iter().collect::<Vec<_>>(), vec![3, 4, 5, 6, 7]);
    }

    #[test]
    fn read_max_remains_4096_even_with_larger_destination() {
        let mut q = VecDeque::from(vec![10i16; 5000]);
        let mut destination = [0i16; 5000];
        assert_eq!(drain_pcm_into(&mut q, &mut destination), 4096);
        assert_eq!(q.len(), 904);
    }
}
