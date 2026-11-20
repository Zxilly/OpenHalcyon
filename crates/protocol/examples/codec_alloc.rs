#[path = "codec_bench/harness.rs"]
mod harness;

use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn main() {
    println!("case,operation,frames,allocations,reallocations,bytes_allocated,bytes_reallocated");
    harness::run(
        harness::iterations(),
        &mut |case, operation, frames, run| {
            run(frames / 4);
            let region = Region::new(GLOBAL);
            run(frames);
            let stats = region.change();
            println!(
                "{case},{operation},{frames},{},{},{},{}",
                stats.allocations,
                stats.reallocations,
                stats.bytes_allocated,
                stats.bytes_reallocated
            );
        },
    );
}
