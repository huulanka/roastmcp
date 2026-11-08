//! Small numeric helpers shared by the analysis modules.

/// Median of the values; sorts the slice in place. `None` for an empty slice.
pub(crate) fn median(values: &mut [f64]) -> Option<f64> {
    let n = values.len();
    if n == 0 {
        return None;
    }
    values.sort_unstable_by(f64::total_cmp);
    let mid = n / 2;
    if n % 2 == 1 {
        Some(values[mid])
    } else {
        Some((values[mid - 1] + values[mid]) / 2.0)
    }
}

/// Least-squares slope of `y` over `x`, or `None` with fewer than
/// `min_points` points or without spread in `x`.
pub(crate) fn ls_slope<I>(points: I, min_points: usize) -> Option<f64>
where
    I: Iterator<Item = (f64, f64)> + Clone,
{
    let (n, sx, sy) = points
        .clone()
        .fold((0usize, 0.0, 0.0), |(n, sx, sy), (x, y)| {
            (n + 1, sx + x, sy + y)
        });
    if n < min_points.max(2) {
        return None;
    }
    let nf = n as f64;
    let (mx, my) = (sx / nf, sy / nf);
    let (sxx, sxy) = points.fold((0.0, 0.0), |(sxx, sxy), (x, y)| {
        let dx = x - mx;
        (sxx + dx * dx, sxy + dx * (y - my))
    });
    if !sxx.is_finite() || sxx <= 1e-12 {
        return None;
    }
    let slope = sxy / sxx;
    slope.is_finite().then_some(slope)
}

/// Linear interpolation in points sorted by `x`. `None` outside their range.
#[allow(dead_code)] // used by the roast metrics (next milestone)
pub(crate) fn interpolate(points: &[(f64, f64)], x: f64) -> Option<f64> {
    if !x.is_finite() {
        return None;
    }
    let i = points.partition_point(|p| p.0 < x);
    let right = points.get(i)?;
    if right.0 == x {
        return Some(right.1);
    }
    let left = points.get(i.checked_sub(1)?)?;
    let span = right.0 - left.0;
    if span <= 0.0 {
        return Some(right.1);
    }
    Some(left.1 + (right.1 - left.1) * (x - left.0) / span)
}

/// Area between the polyline through `points` (sorted by `x`) and the line
/// `y = base`, counting only the parts above `base`. Units: y·x.
#[allow(dead_code)] // used by the roast metrics (next milestone)
pub(crate) fn area_above(points: &[(f64, f64)], base: f64) -> f64 {
    points
        .windows(2)
        .map(|w| {
            let (x0, y0) = (w[0].0, w[0].1 - base);
            let (x1, y1) = (w[1].0, w[1].1 - base);
            let dx = x1 - x0;
            if dx <= 0.0 {
                0.0
            } else if y0 >= 0.0 && y1 >= 0.0 {
                (y0 + y1) / 2.0 * dx
            } else if y0 <= 0.0 && y1 <= 0.0 {
                0.0
            } else {
                // One end above, one below: only the triangle above counts.
                let above = y0.max(y1);
                let frac = above / (y0 - y1).abs();
                above * frac * dx / 2.0
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_odd_and_even() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(&mut []), None);
    }

    #[test]
    fn slope_of_a_line() {
        let pts = [(0.0, 1.0), (1.0, 3.0), (3.0, 7.0)];
        let s = ls_slope(pts.iter().copied(), 3).unwrap();
        assert!((s - 2.0).abs() < 1e-12);
        assert_eq!(ls_slope(pts[..2].iter().copied(), 3), None);
        assert_eq!(ls_slope([(1.0, 1.0), (1.0, 2.0)].iter().copied(), 2), None);
    }

    #[test]
    fn interpolation() {
        let pts = [(0.0, 0.0), (2.0, 4.0), (3.0, 4.0)];
        assert_eq!(interpolate(&pts, 1.0), Some(2.0));
        assert_eq!(interpolate(&pts, 3.0), Some(4.0));
        assert_eq!(interpolate(&pts, 0.0), Some(0.0));
        assert_eq!(interpolate(&pts, -0.1), None);
        assert_eq!(interpolate(&pts, 3.1), None);
    }

    #[test]
    fn area_counts_only_the_part_above_base() {
        // Triangle from (0, 0) up to (2, 2), base 0: area 2.
        assert!((area_above(&[(0.0, 0.0), (2.0, 2.0)], 0.0) - 2.0).abs() < 1e-12);
        // Crossing from -1 to +1 over 2: triangle above of 0.5.
        assert!((area_above(&[(0.0, -1.0), (2.0, 1.0)], 0.0) - 0.5).abs() < 1e-12);
        assert_eq!(area_above(&[(0.0, -1.0), (2.0, -3.0)], 0.0), 0.0);
    }
}
