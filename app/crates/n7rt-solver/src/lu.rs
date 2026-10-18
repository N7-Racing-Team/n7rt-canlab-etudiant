//! Dense LU factorization with partial pivoting (contract §5.3), written here so that every operation and its
//! order are fixed: no BLAS, no SIMD dispatch, no fused multiply-add.

use crate::{ILL_CONDITIONED_RATIO, PIVOT_TOL};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LuFail {
    /// |u_kk| ≤ PIVOT_TOL · max_j |a_kj| of the original row.
    Singular,
    /// A non-finite entry in the matrix or in the factors.
    NonFinite,
}

/// Factors of P·A = L·U (unit diagonal of L implied). The off-diagonal factors are kept row by row as their
/// non-zero entries in ascending column order: the triangular solves skip exact zeros, which changes no value and
/// makes a sub-step of a sparse circuit matrix much cheaper than n².
#[derive(Clone, Debug)]
pub(crate) struct Lu {
    n: usize,
    /// perm[k] = original row now at position k.
    perm: Vec<usize>,
    /// Entries (column, value) of L strictly below the diagonal; row k ends at lower_end[k].
    lower: Vec<(usize, f64)>,
    lower_end: Vec<usize>,
    /// Entries of U strictly above the diagonal; row k ends at upper_end[k].
    upper: Vec<(usize, f64)>,
    upper_end: Vec<usize>,
    diag: Vec<f64>,
    pub(crate) ill_conditioned: bool,
}

impl Lu {
    /// Factors the n×n row-major matrix `a`. Pivot: first row of strictly maximal |a_ik| in ascending order.
    pub(crate) fn factor(mut a: Vec<f64>, n: usize) -> Result<Lu, LuFail> {
        if a.len() != n * n || a.iter().any(|v| !v.is_finite()) {
            return Err(LuFail::NonFinite);
        }
        let mut row_max: Vec<f64> = (0..n)
            .map(|r| a[r * n..(r + 1) * n].iter().fold(0.0f64, |m, v| if v.abs() > m { v.abs() } else { m }))
            .collect();
        let mut perm: Vec<usize> = (0..n).collect();
        let mut u_max = 0.0f64;
        let mut u_min = f64::INFINITY;
        for k in 0..n {
            let mut p = k;
            let mut best = a[k * n + k].abs();
            for i in (k + 1)..n {
                let v = a[i * n + k].abs();
                if v > best {
                    best = v;
                    p = i;
                }
            }
            if p != k {
                for j in 0..n {
                    a.swap(k * n + j, p * n + j);
                }
                perm.swap(k, p);
                row_max.swap(k, p);
            }
            let pivot = a[k * n + k];
            if !pivot.is_finite() {
                return Err(LuFail::NonFinite);
            }
            if pivot.abs() <= PIVOT_TOL * row_max[k] {
                return Err(LuFail::Singular);
            }
            if pivot.abs() > u_max {
                u_max = pivot.abs();
            }
            if pivot.abs() < u_min {
                u_min = pivot.abs();
            }
            for i in (k + 1)..n {
                let l = a[i * n + k] / pivot;
                a[i * n + k] = l;
                if l != 0.0 {
                    for j in (k + 1)..n {
                        a[i * n + j] -= l * a[k * n + j];
                    }
                }
            }
        }
        if a.iter().any(|v| !v.is_finite()) {
            return Err(LuFail::NonFinite);
        }
        let ill_conditioned = n > 0 && u_max / u_min > ILL_CONDITIONED_RATIO;
        let (mut lower, mut upper) = (Vec::new(), Vec::new());
        let (mut lower_end, mut upper_end) = (Vec::with_capacity(n), Vec::with_capacity(n));
        let mut diag = Vec::with_capacity(n);
        for k in 0..n {
            for (j, &v) in a[k * n..(k + 1) * n].iter().enumerate() {
                if v != 0.0 && j != k {
                    if j < k {
                        lower.push((j, v));
                    } else {
                        upper.push((j, v));
                    }
                }
            }
            lower_end.push(lower.len());
            upper_end.push(upper.len());
            diag.push(a[k * n + k]);
        }
        Ok(Lu { n, perm, lower, lower_end, upper, upper_end, diag, ill_conditioned })
    }

    /// Solves A·x = b. `x` has length n; non-finite results are left to the caller to detect. Same operations in the
    /// same order as the dense substitutions, minus the products by an exact zero.
    pub(crate) fn solve(&self, b: &[f64], x: &mut [f64]) {
        let mut begin = 0;
        for k in 0..self.n {
            let mut s = b[self.perm[k]];
            for &(j, l) in &self.lower[begin..self.lower_end[k]] {
                s -= l * x[j];
            }
            begin = self.lower_end[k];
            x[k] = s;
        }
        for k in (0..self.n).rev() {
            let begin = if k == 0 { 0 } else { self.upper_end[k - 1] };
            let mut s = x[k];
            for &(j, u) in &self.upper[begin..self.upper_end[k]] {
                s -= u * x[j];
            }
            x[k] = s / self.diag[k];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Lu, LuFail};

    fn solve(a: &[f64], n: usize, b: &[f64]) -> Result<Vec<f64>, LuFail> {
        let lu = Lu::factor(a.to_vec(), n)?;
        let mut x = vec![0.0; n];
        lu.solve(b, &mut x);
        Ok(x)
    }

    #[test]
    fn solves_with_row_exchange() {
        // Zero leading pivot: needs the exchange. Exact solution (1, 2, 3).
        let a = [0.0, 2.0, 1.0, 1.0, 1.0, 1.0, 2.0, 1.0, 3.0];
        let b = [7.0, 6.0, 13.0];
        let x = solve(&a, 3, &b).unwrap();
        for (xi, e) in x.iter().zip([1.0, 2.0, 3.0]) {
            assert!((xi - e).abs() < 1e-14, "{x:?}");
        }
    }

    #[test]
    fn pivot_is_first_strict_maximum() {
        // Rows 1 and 2 tie on |a_i0| = 2: the first one (row 1) is chosen; the second column then keeps its order
        // (|3.5| > |2|), so the permutation is (1, 0, 2). Choosing row 2 would have given (2, ...).
        let lu = Lu::factor(vec![1.0, 4.0, 0.0, 2.0, 1.0, 0.0, -2.0, 1.0, 1.0], 3).unwrap();
        assert_eq!(lu.perm, vec![1, 0, 2]);
    }

    #[test]
    fn singular_threshold_is_relative_to_the_original_row() {
        // Second row is the first scaled by 1 + 1e-15: u_11 ≈ 2e-15 ≤ 1e-13 · 2 → singular.
        let a = [1.0, 1.0, 1.0 + 1e-15, 1.0 + 1e-15];
        assert_eq!(solve(&a, 2, &[0.0, 0.0]).err(), Some(LuFail::Singular));
        // Same structure, u_11 = 1e-6 ≫ 1e-13: regular.
        let a = [1.0, 1.0, 1.0, 1.0 + 1e-6];
        let x = solve(&a, 2, &[2.0, 2.0 + 1e-6]).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-9 && (x[1] - 1.0).abs() < 1e-9, "{x:?}");
        assert_eq!(solve(&[0.0], 1, &[1.0]).err(), Some(LuFail::Singular));
        assert_eq!(solve(&[f64::NAN], 1, &[1.0]).err(), Some(LuFail::NonFinite));
    }

    #[test]
    fn sparse_substitutions_solve_nonsymmetric_systems() {
        // Ladder-like nonsymmetric matrices with a few far entries (fill-in and row exchanges): residual of A·x = b.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut unit = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for n in [1, 2, 5, 17, 40] {
            let mut a = vec![0.0; n * n];
            for i in 0..n {
                a[i * n + i] = 0.5 + unit();
                if i + 1 < n {
                    a[i * n + i + 1] = unit() - 0.5;
                    a[(i + 1) * n + i] = 2.0 * unit() - 1.0;
                }
                let far = (i * 7 + 3) % n;
                a[i * n + far] += 3.0 * unit() - 1.5;
            }
            let b: Vec<f64> = (0..n).map(|_| unit() - 0.5).collect();
            let x = solve(&a, n, &b).unwrap();
            for i in 0..n {
                let ax: f64 = (0..n).map(|j| a[i * n + j] * x[j]).sum();
                assert!((ax - b[i]).abs() <= 1e-12, "n = {n}, row {i}: {ax} vs {}", b[i]);
            }
        }
    }

    #[test]
    fn ill_conditioned_flag() {
        let lu = Lu::factor(vec![1e8, 0.0, 0.0, 1e-7], 2).unwrap();
        assert!(lu.ill_conditioned);
        let lu = Lu::factor(vec![1e6, 0.0, 0.0, 1e-7], 2).unwrap();
        assert!(!lu.ill_conditioned);
    }
}
