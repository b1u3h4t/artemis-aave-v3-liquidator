// Cargo.toml 需添加依赖:

use ethers::types::U256;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref WAD: U256 = U256::from(1_000_000_000_000_000_000u128); // 1e18
    pub static ref HALF_WAD: U256 = U256::from(500_000_000_000_000_000u128); // 0.5e18
    pub static ref RAY: U256 = U256::from(1_000_000_000_000_000_000_000_000_000u128); // 1e27
    pub static ref HALF_RAY: U256 = U256::from(500_000_000_000_000_000_000_000_000u128); // 0.5e27
    pub static ref WAD_RAY_RATIO: U256 = U256::from(1_000_000_000u128); // 1e9
}

/// 两个 wad 相乘，四舍五入到最近的 wad
pub fn wad_mul(a: U256, b: U256) -> U256 {
    if b.is_zero() {
        return U256::zero();
    }

    let max_a = (U256::max_value() - *HALF_WAD) / b;
    if a > max_a {
        panic!("wadMul: multiplication overflow");
    }

    (a * b + *HALF_WAD) / *WAD
}

/// 两个 wad 相除，四舍五入到最近的 wad
pub fn wad_div(a: U256, b: U256) -> U256 {
    if b.is_zero() {
        panic!("wadDiv: division by zero");
    }

    let half_b = b / U256::from(2);
    let max_a = (U256::max_value() - half_b) / *WAD;
    if a > max_a {
        panic!("wadDiv: multiplication overflow");
    }

    (a * *WAD + half_b) / b
}

/// 两个 ray 相乘，四舍五入到最近的 ray
pub fn ray_mul(a: U256, b: U256) -> U256 {
    if b.is_zero() {
        return U256::zero();
    }

    let max_a = (U256::max_value() - *HALF_RAY) / b;
    if a > max_a {
        panic!("rayMul: multiplication overflow");
    }

    (a * b + *HALF_RAY) / *RAY
}

/// 两个 ray 相除，四舍五入到最近的 ray
pub fn ray_div(a: U256, b: U256) -> U256 {
    if b.is_zero() {
        panic!("rayDiv: division by zero");
    }

    let half_b = b / U256::from(2);
    let max_a = (U256::max_value() - half_b) / *RAY;
    if a > max_a {
        panic!("rayDiv: multiplication overflow");
    }

    (a * *RAY + half_b) / b
}

/// 将 ray 转换为 wad（四舍五入）
pub fn ray_to_wad(a: U256) -> U256 {
    let ratio = *WAD_RAY_RATIO;
    let result = a / ratio;
    let remainder = a % ratio;

    if remainder >= ratio / U256::from(2) {
        result + U256::one()
    } else {
        result
    }
}

/// 将 wad 转换为 ray
pub fn wad_to_ray(a: U256) -> U256 {
    let ratio = *WAD_RAY_RATIO;
    let result = a * ratio;

    if result / ratio != a {
        panic!("wadToRay: overflow");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::U256;

    // 测试 wadMul 函数
    #[test]
    fn test_wad_mul() {
        let a = U256::from_dec_str("134534543232342353231234").unwrap();
        let b = U256::from_dec_str("13265462389132757665657").unwrap();

        // 正常乘法
        assert_eq!(
            wad_mul(a, b),
            a * b / *WAD + (a * b % *WAD + *HALF_WAD) / *WAD
        );

        // 乘以零
        assert_eq!(wad_mul(U256::zero(), b), U256::zero());
        assert_eq!(wad_mul(a, U256::zero()), U256::zero());

        // 溢出测试
        let too_large_a = (U256::max_value() - *HALF_WAD) / b + U256::one();
        let result = std::panic::catch_unwind(|| wad_mul(too_large_a, b));
        assert!(result.is_err());
    }

    // 测试 wadDiv 函数
    #[test]
    fn test_wad_div() {
        let a = U256::from_dec_str("134534543232342353231234").unwrap();
        let b = U256::from_dec_str("13265462389132757665657").unwrap();

        // 正常除法
        assert_eq!(wad_div(a, b), (a * *WAD + b / U256::from(2)) / b);

        // 除数为零
        let result = std::panic::catch_unwind(|| wad_div(a, U256::zero()));
        assert!(result.is_err());

        // 溢出测试
        let half_b = b / U256::from(2);
        let too_large_a = (U256::max_value() - half_b) / *WAD + U256::one();
        let result = std::panic::catch_unwind(|| wad_div(too_large_a, b));
        assert!(result.is_err());
    }

    // 测试 rayMul 函数
    #[test]
    fn test_ray_mul() {
        let a = U256::from_dec_str("134534543232342353231234").unwrap();
        let b = U256::from_dec_str("13265462389132757665657").unwrap();

        // 正常乘法
        assert_eq!(
            ray_mul(a, b),
            a * b / *RAY + (a * b % *RAY + *HALF_RAY) / *RAY
        );

        // 乘以零
        assert_eq!(ray_mul(U256::zero(), b), U256::zero());
        assert_eq!(ray_mul(a, U256::zero()), U256::zero());

        // 溢出测试
        let too_large_a = (U256::max_value() - *HALF_RAY) / b + U256::one();
        let result = std::panic::catch_unwind(|| ray_mul(too_large_a, b));
        assert!(result.is_err());
    }

    // 测试 rayDiv 函数
    #[test]
    fn test_ray_div() {
        let a = U256::from_dec_str("134534543232342353231234").unwrap();
        let b = U256::from_dec_str("13265462389132757665657").unwrap();

        // 正常除法
        assert_eq!(ray_div(a, b), (a * *RAY + b / U256::from(2)) / b);

        // 除数为零
        let result = std::panic::catch_unwind(|| ray_div(a, U256::zero()));
        assert!(result.is_err());

        // 溢出测试
        let half_b = b / U256::from(2);
        let too_large_a = (U256::max_value() - half_b) / *RAY + U256::one();
        let result = std::panic::catch_unwind(|| ray_div(too_large_a, b));
        assert!(result.is_err());
    }

    // 测试 rayToWad 函数
    #[test]
    fn test_ray_to_wad() {
        let half = *WAD_RAY_RATIO / U256::from(2);

        // 精确转换
        let a = U256::exp10(27);
        assert_eq!(ray_to_wad(a), U256::exp10(18));

        // 向下舍入
        let round_down = U256::exp10(27) + half - U256::one();
        assert_eq!(ray_to_wad(round_down), U256::exp10(18));

        // 向上舍入
        let round_up = U256::exp10(27) + half;
        assert_eq!(ray_to_wad(round_up), U256::exp10(18) + U256::one());

        // 大数测试
        let too_large = U256::max_value() - half + U256::one();
        assert_eq!(
            ray_to_wad(too_large),
            too_large / *WAD_RAY_RATIO + U256::one()
        );
    }

    // 测试 wadToRay 函数
    #[test]
    fn test_wad_to_ray() {
        // 正常转换
        let a = U256::exp10(18);
        assert_eq!(wad_to_ray(a), U256::exp10(27));

        // 溢出测试
        let too_large = U256::max_value() / *WAD_RAY_RATIO + U256::one();
        let result = std::panic::catch_unwind(|| wad_to_ray(too_large));
        assert!(result.is_err());
    }
}
