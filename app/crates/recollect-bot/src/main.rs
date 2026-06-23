//! Headless balance sim: `cargo run -p recollect-bot --release -- 10000`
//! Prints win/draw rates and P1-vs-P2 skew — instrumenting the design target
//! (P1/P2 inside 48–52%) before a single pixel is rendered.

#![forbid(unsafe_code)]
use recollect_bot::selfplay;
use recollect_core::state::MatchResult;
use recollect_core::types::Seat;

fn main() {
    let n: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let (mut a, mut b, mut d) = (0u64, 0u64, 0u64);
    for i in 0..n {
        match selfplay(i, i.wrapping_mul(0x9E37_79B9)).0 {
            MatchResult::Win(Seat::A) => a += 1,
            MatchResult::Win(Seat::B) => b += 1,
            MatchResult::Draw => d += 1,
        }
    }
    let pct = |x: u64| 100.0 * x as f64 / n as f64;
    println!(
        "matches={n} A={a} ({:.1}%) B={b} ({:.1}%) draws={d} ({:.1}%)",
        pct(a),
        pct(b),
        pct(d)
    );
}
