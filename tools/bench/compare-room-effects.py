"""Compare release builds on a deterministic native Solo Bot match."""

import argparse
import csv
import statistics
import subprocess
import sys


def run(binary, args):
    command = [binary, args.delivery, args.extra, args.scenario]
    output = subprocess.run(command, capture_output=True, text=True, check=True).stdout
    room, *extra = csv.reader(output.splitlines())
    label, tick, winner, seconds, trace, state = room
    if label != "room":
        raise ValueError(f"Unexpected match output: {output}")
    gifts = bulwarks = ""
    if args.scenario == "room-effects":
        [effects] = extra
        cast_label, gift_name, gifts, bulwark_name, bulwarks = effects
        if (cast_label, gift_name, bulwark_name) != (
            "effect_casts", "gift_of_fire", "bright_bulwark"
        ) or min(int(gifts), int(bulwarks)) <= 0:
            raise ValueError(f"Expected both benchmark spells: {effects}")
    elif extra:
        raise ValueError(f"Unexpected match output: {output}")
    return float(seconds), (tick, winner, trace, state, gifts, bulwarks)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("baseline", "candidate", "delivery", "extra"):
        parser.add_argument(name)
    parser.add_argument("--scenario", choices=("room", "room-effects"), default="room-effects")
    parser.add_argument("--pairs", type=int, default=7)
    args = parser.parse_args()
    if args.pairs < 1:
        parser.error("--pairs must be positive")
    _, expected = run(args.baseline, args)
    labels = {"baseline": args.baseline, "candidate": args.candidate}
    if run(args.candidate, args)[1] != expected:
        raise ValueError("Candidate warm-up differs from the baseline")
    writer = csv.writer(sys.stdout, lineterminator="\n")
    writer.writerow(("pair", "build", "seconds", "tick", "winner", "trace", "state", "gifts", "bulwarks"))
    elapsed = {label: [] for label in labels}
    for pair in range(args.pairs):
        order = labels if pair % 2 == 0 else reversed(labels)
        for label in order:
            seconds, result = run(labels[label], args)
            if result != expected:
                raise ValueError(f"{label} diverged during pair {pair + 1}: {result}")
            elapsed[label].append(seconds)
            writer.writerow((pair + 1, label, seconds, *result))
    medians = {label: statistics.median(times) for label, times in elapsed.items()}
    print(f"Median seconds: {medians}", file=sys.stderr)
    ratios = [b / a for a, b in zip(elapsed["baseline"], elapsed["candidate"], strict=True)]
    print(f"Median paired change: {(statistics.median(ratios) - 1) * 100:+.2f}%", file=sys.stderr)


if __name__ == "__main__":
    main()
