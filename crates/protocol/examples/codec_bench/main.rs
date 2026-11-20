mod harness;

use std::time::Instant;

fn main() {
    println!("case,operation,frames,elapsed_ns,frames_per_sec");
    harness::run(
        harness::iterations(),
        &mut |case, operation, frames, run| {
            run(frames / 4);
            let start = Instant::now();
            run(frames);
            let elapsed = start.elapsed();
            let frames_per_sec = frames as f64 / elapsed.as_secs_f64();
            println!(
                "{case},{operation},{frames},{},{frames_per_sec:.0}",
                elapsed.as_nanos()
            );
        },
    );
}
