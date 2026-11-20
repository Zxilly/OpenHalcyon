"""Compare distinct release builds using deterministic room or spell workloads."""

import argparse
import csv
import hashlib
import os
import re
import statistics
import subprocess
import sys


def run_room(binary, args):
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


def run_phase(binary, args):
    env = dict(os.environ, HALCYON_DELIVERY=args.delivery, HALCYON_DATA_EXTRA=args.extra)
    command = [binary, "phase_cost::effect_phase_cost", "--ignored", "--exact", "--nocapture"]
    output = subprocess.run(command, capture_output=True, text=True, check=True, env=env).stdout
    result = re.search(
        r"^phase median_block_ns (\d+) min \d+ max \d+ events (\d+) final ([0-9a-f]{64})$",
        output, re.MULTILINE,
    )
    if result is None:
        raise ValueError(f"Missing spell phase measurement: {output}")
    ns, events, state = result.groups()
    return int(ns), (events, state)


def executable_hash(path):
    digest = hashlib.sha256()
    with open(path, "rb") as binary:
        for block in iter(lambda: binary.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("baseline", "candidate", "delivery", "extra"):
        parser.add_argument(name)
    parser.add_argument("--scenario", choices=("room", "room-effects", "phase"), default="room-effects")
    parser.add_argument("--pairs", type=int, default=7)
    args = parser.parse_args()
    if args.pairs < 1:
        parser.error("--pairs must be positive")
    builds = {name: executable_hash(getattr(args, name)) for name in ("baseline", "candidate")}
    if builds["baseline"] == builds["candidate"]:
        parser.error(f"both executables have the same SHA-256: {builds['baseline']}")
    print(f"Executable SHA-256: {builds}", file=sys.stderr)
    if args.scenario == "phase":
        run = run_phase
        fields = ("phase_ns", "events", "state")
    else:
        run = run_room
        fields = ("seconds", "tick", "winner", "trace", "state", "gifts", "bulwarks")
    _, expected = run(args.baseline, args)
    labels = {"baseline": args.baseline, "candidate": args.candidate}
    if run(args.candidate, args)[1] != expected:
        raise ValueError("Candidate warm-up differs from the baseline")
    writer = csv.writer(sys.stdout, lineterminator="\n")
    writer.writerow(("pair", "build", *fields))
    elapsed = {label: [] for label in labels}
    for pair in range(args.pairs):
        order = labels if pair % 2 == 0 else reversed(labels)
        for label in order:
            duration, result = run(labels[label], args)
            if result != expected:
                raise ValueError(f"{label} diverged during pair {pair + 1}: {result}")
            elapsed[label].append(duration)
            writer.writerow((pair + 1, label, duration, *result))
    medians = {label: statistics.median(times) for label, times in elapsed.items()}
    print(f"Median {fields[0]}: {medians}", file=sys.stderr)
    ratios = [b / a for a, b in zip(elapsed["baseline"], elapsed["candidate"], strict=True)]
    print(f"Median paired change: {(statistics.median(ratios) - 1) * 100:+.2f}%", file=sys.stderr)


if __name__ == "__main__":
    main()
