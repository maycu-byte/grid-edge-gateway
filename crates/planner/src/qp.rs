//! A small builder for convex quadratic programs, solved with Clarabel
//! (a pure-Rust interior-point solver, so the same code runs natively and
//! in WebAssembly):
//!
//! ```text
//! minimise   ½ xᵀ P x + qᵀ x
//! subject to A_eq x = b_eq,   A_le x ≤ b_le
//! ```

use clarabel::algebra::CscMatrix;
use clarabel::solver::{
    DefaultSettingsBuilder, DefaultSolver, IPSolver, NonnegativeConeT, SolverStatus, SupportedConeT, ZeroConeT,
};

pub(crate) type Terms = Vec<(usize, f64)>;

#[derive(Default)]
pub(crate) struct Qp {
    n: usize,
    q: Vec<f64>,
    p: (Vec<usize>, Vec<usize>, Vec<f64>),
    eq: Vec<(Terms, f64)>,
    le: Vec<(Terms, f64)>,
}

pub(crate) struct Solution {
    pub x: Vec<f64>,
    pub objective: f64,
    pub iterations: u32,
    pub solve_ms: f64,
}

impl Qp {
    /// A new variable with bounds (infinite bounds add no row).
    pub fn var(&mut self, lo: f64, hi: f64) -> usize {
        let i = self.n;
        self.n += 1;
        self.q.push(0.0);
        if lo.is_finite() {
            self.le.push((vec![(i, -1.0)], -lo));
        }
        if hi.is_finite() {
            self.le.push((vec![(i, 1.0)], hi));
        }
        i
    }

    pub fn cost(&mut self, i: usize, c: f64) {
        self.q[i] += c;
    }

    pub fn eq(&mut self, terms: Terms, rhs: f64) {
        self.eq.push((terms, rhs));
    }

    pub fn le(&mut self, terms: Terms, rhs: f64) {
        self.le.push((terms, rhs));
    }

    pub fn ge(&mut self, terms: Terms, rhs: f64) {
        self.le.push((terms.into_iter().map(|(i, a)| (i, -a)).collect(), -rhs));
    }

    /// Adds `w · (aᵀx)²` to the objective.
    pub fn square(&mut self, terms: &[(usize, f64)], w: f64) {
        // Merge repeated variables first: w(ax + bx)² = w(a + b)²x².
        let mut a: Vec<(usize, f64)> = Vec::with_capacity(terms.len());
        for &(i, v) in terms {
            match a.iter_mut().find(|t| t.0 == i) {
                Some(t) => t.1 += v,
                None => a.push((i, v)),
            }
        }
        // ½ xᵀPx = w (aᵀx)² ⇒ P = 2w aaᵀ. Clarabel takes the upper triangle,
        // so each unordered pair is stored once.
        for (k, &(i, ai)) in a.iter().enumerate() {
            for &(j, aj) in &a[k..] {
                self.p.0.push(i.min(j));
                self.p.1.push(i.max(j));
                self.p.2.push(2.0 * w * ai * aj);
            }
        }
    }

    pub fn solve(&self) -> Result<Solution, String> {
        let n = self.n;
        let m_eq = self.eq.len();
        let m = m_eq + self.le.len();
        let (mut ri, mut ci, mut vi) = (Vec::new(), Vec::new(), Vec::new());
        let mut b = Vec::with_capacity(m);
        for (row, (terms, rhs)) in self.eq.iter().chain(self.le.iter()).enumerate() {
            for &(j, a) in terms {
                if a != 0.0 {
                    ri.push(row);
                    ci.push(j);
                    vi.push(a);
                }
            }
            b.push(*rhs);
        }
        let a = CscMatrix::new_from_triplets(m, n, ri, ci, vi);
        let p = CscMatrix::new_from_triplets(n, n, self.p.0.clone(), self.p.1.clone(), self.p.2.clone());
        let cones: Vec<SupportedConeT<f64>> = vec![ZeroConeT(m_eq), NonnegativeConeT(m - m_eq)];
        let settings = DefaultSettingsBuilder::default()
            .verbose(false)
            .max_iter(200)
            .time_limit(10.0)
            .build()
            .map_err(|e| format!("solver settings: {e:?}"))?;
        let mut solver =
            DefaultSolver::new(&p, &self.q, &a, &b, &cones, settings).map_err(|e| format!("solver setup: {e:?}"))?;
        solver.solve();
        match solver.solution.status {
            SolverStatus::Solved | SolverStatus::AlmostSolved => Ok(Solution {
                x: solver.solution.x.clone(),
                objective: solver.solution.obj_val,
                iterations: solver.solution.iterations,
                solve_ms: solver.solution.solve_time * 1000.0,
            }),
            s => Err(format!("{s:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_a_small_lp() {
        // max x + y  s.t. x + 2y ≤ 4, 3x + y ≤ 6, x, y ≥ 0  →  x = 1.6, y = 1.2
        let mut qp = Qp::default();
        let x = qp.var(0.0, f64::INFINITY);
        let y = qp.var(0.0, f64::INFINITY);
        qp.cost(x, -1.0);
        qp.cost(y, -1.0);
        qp.le(vec![(x, 1.0), (y, 2.0)], 4.0);
        qp.le(vec![(x, 3.0), (y, 1.0)], 6.0);
        let s = qp.solve().unwrap();
        assert!((s.x[x] - 1.6).abs() < 1e-6 && (s.x[y] - 1.2).abs() < 1e-6, "{:?}", s.x);
    }

    #[test]
    fn square_term_matches_its_definition() {
        // min (x − y − 1)² with x, y ∈ [0, 5] and y = 2  →  x = 3
        let mut qp = Qp::default();
        let x = qp.var(0.0, 5.0);
        let y = qp.var(0.0, 5.0);
        let one = qp.var(1.0, 1.0);
        qp.eq(vec![(y, 1.0)], 2.0);
        qp.square(&[(x, 1.0), (y, -1.0), (one, -1.0)], 1.0);
        let s = qp.solve().unwrap();
        assert!((s.x[x] - 3.0).abs() < 1e-5, "{:?}", s.x);
        assert!(s.objective.abs() < 1e-6, "(3 − 2 − 1)² = 0, got {}", s.objective);
    }
}
