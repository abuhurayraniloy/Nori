//! Command: nori speed

use crate::service::SystemLoad;
use nori_core::vectors::Vectors;
use std::time::Instant;

pub struct SpeedArgs {
    pub tries: usize,
    pub json: bool,
}

pub fn run_speed(args: SpeedArgs) -> Result<(), String> {
    let tries = if args.tries == 0 { 5 } else { args.tries };

    let load_before = SystemLoad::sample();

    // Benchmark SIMD dot product of 768-dim vectors
    let rows = vec![0.035_f32; 768];
    let query = vec![0.042_f32; 768];
    let mut scores = vec![0.0f32; 1];

    let start = Instant::now();
    let iterations = 100_000 * tries;

    for _ in 0..iterations {
        Vectors::scores(&rows, &query, &mut scores);
    }

    let elapsed = start.elapsed();
    let seconds = elapsed.as_secs_f64();
    let ops_per_sec = if seconds > 0.0 {
        iterations as f64 / seconds
    } else {
        0.0
    };

    let load_after = SystemLoad::sample();
    let sys_load = match (&load_before, &load_after) {
        (Some(b), Some(a)) => a.load_between(b),
        _ => 0.0,
    };

    if args.json {
        let json_val = serde_json::json!({
            "event": "speed",
            "iterations": iterations,
            "elapsed_seconds": seconds,
            "simd_dot_products_per_sec": ops_per_sec,
            "system_cpu_load": sys_load,
            "score_check": scores[0],
        });
        println!("{}", serde_json::to_string(&json_val).unwrap());
    } else {
        println!("\n⚡ Nori SIMD Performance Benchmark:");
        println!("  Iterations:          {}", iterations);
        println!("  Time elapsed:        {:.3}s", seconds);
        println!("  SIMD 768-dim dot/s:  {:.2} million/sec", ops_per_sec / 1e6);
        println!("  System CPU load:     {:.1}%\n", sys_load * 100.0);
    }

    Ok(())
}
