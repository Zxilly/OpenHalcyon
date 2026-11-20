"""Verify a supplement against original TCP/VGR files using the Rust codecs.

Build capture_inventory and replay_inventory first. The three positional match
directories must be the three ARAL streams summarized in the supplied report.
Only aggregate fields and an opcode are printed. No network access is used.
"""
import argparse
import json
import subprocess
from collections import Counter, defaultdict
from pathlib import Path


def read_json(path):
    with path.open(encoding="utf-8") as source:
        return json.load(source)


def invocation(binary, *args):
    result = subprocess.run([binary, *args], text=True, capture_output=True)
    require(result.returncode == 0, f"{binary.name} rejected its input (exit {result.returncode})")
    return result.stdout.splitlines()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify(args):
    sizes = defaultdict(Counter)
    directions = defaultdict(set)
    totals = defaultdict(Counter)
    typed = Counter()
    opaque = Counter()
    short = Counter()
    registry = set()
    readers = {}
    entity_checks = None
    for match in args.matches:
        lines = invocation(args.bin / "capture_inventory", match, "--sizes")
        require("hello_port_check\tok" in lines, "missing archive hello port verification")
        for line in lines:
            row = line.split("\t")
            if row[0] in ("registry", "size", "handshake"):
                if row[0] == "registry":
                    require(row[2] == "true", f"unregistered opcode {row[1]}")
                    registry.add(int(row[1]))
                    continue
                kind, direction, op, size, count = row
                op, size, count = map(int, (op, size, count))
                sizes[op][size] += count
                directions[op].add({"C2S": "C->S", "S2C": "S->C"}[direction])
                totals[op]["network"] += count
                if kind == "handshake":
                    totals[op]["handshake"] += count
            elif row[0] in ("C2S", "S2C"):
                op = int(row[1], 16)
                typed[op] += int(row[3])
                opaque[op] += int(row[4])
                require(int(row[5]) == 0, f"field decode failed at {op:#06x}")
                short[op] += int(row[6])

    for line in invocation(args.bin / "replay_inventory", args.vgr, "--empirical",
                           args.empirical / "replay_entities_5v5.json"):
        row = line.split("\t")
        if row[0] == "entity_checks":
            entity_checks = int(row[1])
        elif row[0] == "registry":
            require(row[2] == "true", f"unregistered replay opcode {row[1]}")
            registry.add(int(row[1]))
        elif row[0] == "size":
            _, _, op, size, count = row
            sizes[int(op)][int(size)] += int(count)
            totals[int(op)]["vgr"] += int(count)
        elif row[0].startswith("0x"):
            op = int(row[0], 16)
            readers[op] = row[1]
            typed[op] += int(row[2])
            opaque[op] += int(row[3])

    rows = read_json(args.empirical / "opcodes_observed.json")
    ids = {r["opcode_dec"] for r in rows}
    require(len(ids) == len(rows) and ids == set(sizes), "opcode sets disagree")
    print("opcode\tTCP\tVGR\tdirections\treader\ttyped\topaque\tshort_live_reader\tsize:count")
    for row in sorted(rows, key=lambda r: r["opcode_dec"]):
        op = row["opcode_dec"]
        label = f"{op:#06x}"
        require(int(row["opcode_hex"], 16) == op, f"{label} numeric IDs disagree")
        require(op in registry or totals[op]["handshake"], f"{label} unknown opcode")
        for src, key in (("network", "count_aral_3matches"), ("vgr", "count_vgr_5v5")):
            require(totals[op][src] == row[key], f"{label} {src} count mismatch")
        require(directions[op] == set(row["directions"]), f"{label} direction mismatch")
        observed = {int(k): v for k, v in row["observed_sizes"].items()}
        # The extractor exports at most the eight most frequent sizes; ties at
        # eighth place depend on record order.
        measured = sizes[op]
        require(len(observed) == min(8, len(measured)) and
                all(measured[k] == v for k, v in observed.items()) and
                sorted(observed.values(), reverse=True) ==
                sorted(measured.values(), reverse=True)[:8], f"{label} size histogram mismatch")
        tag = row["read_side_tag"]
        reader = readers.get(op, "unlisted")
        if tag and tag["size"] is not None:
            require(reader == str(tag["size"]), f"{label} fixed reader mismatch ({reader})")
        elif tag:
            require(reader in ("ignored", "clock"), f"{label} reader classification mismatch ({reader})")
        else:
            require(reader == "unlisted", f"{label} unexpected Android reader {reader}")
        require(typed[op] + opaque[op] + totals[op]["handshake"] ==
                totals[op]["network"] + totals[op]["vgr"], f"{label} missing decode results")
        print(label, totals[op]["network"], totals[op]["vgr"], ",".join(sorted(directions[op])) or "-",
              reader, typed[op], opaque[op], short[op],
              ",".join(f"{k}:{n}" for k,n in sorted(measured.items())), sep="\t")
    require(entity_checks is not None, "missing replay entity verification")
    print(f"verified_opcodes={len(rows)} verified_replay_actors={entity_checks}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ("bin", "vgr", "empirical"):
        parser.add_argument("--" + flag, type=Path, required=True)
    parser.add_argument("matches", type=Path, nargs=3)
    verify(parser.parse_args())
