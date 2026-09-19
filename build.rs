//! build.rs - 编译期注入构建时间戳
//!
//! 每次编译时把 `SUBHUTI_BUILD_TIME` 环境变量固化进二进制，
//! 运行时（如 MCP `serverInfo`）即可用 `env!("SUBHUTI_BUILD_TIME")` 读到，
//! 让客户端/ AI 能识别当前二进制是什么时候编译的。
//! 不声明 `rerun-if-changed`，让每次构建都刷新时间戳。

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时钟早于 1970")
        .as_secs() as i64;

    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hh, mm, ss) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    let (yyyy, mo, dd) = civil_from_days(days);

    let stamp = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        yyyy, mo, dd, hh, mm, ss
    );
    println!("cargo:rustc-env=SUBHUTI_BUILD_TIME={}", stamp);
}

/// 自 1970-01-01 起的天数 → (年, 月, 日)，参照 Howard Hinnant 的 civil_from_days。
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
