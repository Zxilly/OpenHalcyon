"""Aggregate the paired report emitted by the release example."""
import re
import statistics
import sys
from pathlib import Path

names = ["World::visible", "packed", "hecs rebuild", "team vectors",
         "hecs archetypes", "brush cache", "persistent hecs"]
path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("paired-results.txt")
lines = path.read_text(encoding="utf-8-sig").splitlines()
if not re.fullmatch(r"sizes \(\d+, \d+, \d+\)", lines[0]):
    raise ValueError("missing size header")
rows = {}
for line in lines[1:]:
    tag, arena, seed, mode, ticks, step, total, p99, retained, calls, digest = line.split()
    key = arena, int(seed), int(mode)
    if tag != "pair" or key in rows or not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise ValueError("invalid paired record")
    rows[key] = int(ticks), int(step), int(total), int(p99), int(retained), int(calls), digest
expected = {(arena, seed, mode) for arena in ("validation", "solo")
            for seed in (1, 2) for mode in range(len(names))}
if set(rows) != expected:
    raise ValueError("incomplete paired matrix")
for arena in ("validation", "solo"):
    for seed in (1, 2):
        states = {(r[0], r[1], r[6]) for k, r in rows.items() if k[:2] == (arena, seed)}
        if len(states) != 1 or next(iter(states))[0] < 1:
            raise ValueError("inconsistent paired world")
    print(arena)
    print("mode | mean us | mean run p99 us | mean retained B")
    for mode, name in enumerate(names):
        runs = [rows[arena, seed, mode] for seed in (1, 2)]
        mean = statistics.mean
        print(f"{name} | {mean(r[2]/r[0]/1000 for r in runs):.2f} | "
              f"{mean(r[3]/1000 for r in runs):.2f} | {mean(r[4] for r in runs):.0f}")
