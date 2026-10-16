//! Coordinates for the original three Slot core-selector hint faces.
//! Kept separate from OpenGL so the non-overlap guarantee is host-testable.

pub fn positions(widths: [u32; 3]) -> [(f32, f32); 3] {
    // Slot's core board occupies X=174..546 in its native 720x480 canvas.
    const LEFT: f32 = 174.0;
    const RIGHT: f32 = 546.0;
    const GAP: f32 = 12.0;
    let original = widths.map(|w| w as f32);
    let sum: f32 = original.iter().sum();
    let scale = if sum > 0.0 {
        ((RIGHT - LEFT - 2.0 * GAP) / sum).min(1.0)
    } else {
        1.0
    };
    let [a, b, c] = original.map(|w| w * scale);
    let first = LEFT;
    let last = RIGHT - c;
    // Keep the original middle hint centred when space permits; otherwise
    // use the nearest non-overlapping position.
    let centre = (720.0 - b) / 2.0;
    let middle = centre.max(first + a + GAP).min(last - b - GAP);
    [(first, scale), (middle, scale), (last, scale)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_original_hints_are_spaced_within_board() {
        for widths in [[88, 112, 96], [130, 170, 150], [200, 300, 210], [1, 1, 1]] {
            let p = positions(widths);
            let a_end = p[0].0 + widths[0] as f32 * p[0].1;
            let b_end = p[1].0 + widths[1] as f32 * p[1].1;
            let c_end = p[2].0 + widths[2] as f32 * p[2].1;
            assert!(a_end + 11.9 <= p[1].0, "{widths:?} p={p:?}");
            assert!(b_end + 11.9 <= p[2].0, "{widths:?} p={p:?}");
            assert!(p[0].0 >= 174.0);
            assert!(c_end <= 546.001);
        }
    }

    #[test]
    fn centre_label_remains_centred_when_space_allows() {
        let p = positions([80, 90, 80]);
        let middle_centre = p[1].0 + 90.0 * p[1].1 / 2.0;
        assert!((middle_centre - 360.0).abs() < 0.01);
        assert_eq!(p[0].1, 1.0);
    }
}
