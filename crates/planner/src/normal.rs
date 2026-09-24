//! Quantiles of the standard normal distribution, for chance constraints.

/// Φ⁻¹(p): the z with P(Z ≤ z) = p for a standard normal Z.
///
/// Acklam's rational approximation (relative error below 1.2e-9), which is
/// far more precise than any forecast error model it is used with.
pub fn inverse_cdf(p: f64) -> f64 {
    assert!(p > 0.0 && p < 1.0, "probability must be in (0, 1), got {p}");
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383577518672690e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [7.784695709041462e-03, 3.224671290700398e-01, 2.445134137142996e+00, 3.754408661907416e+00];
    const P_LOW: f64 = 0.02425;
    if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - P_LOW {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        -inverse_cdf(1.0 - p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_quantiles() {
        assert!(inverse_cdf(0.5).abs() < 1e-9);
        assert!((inverse_cdf(0.95) - 1.644_853_627).abs() < 1e-8);
        assert!((inverse_cdf(0.99) - 2.326_347_874).abs() < 1e-8);
        assert!((inverse_cdf(0.001) + 3.090_232_306).abs() < 1e-8);
    }
}
