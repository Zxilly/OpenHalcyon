"""Measure independent Windows servers and CE load clients. Requires Python 3.10+ and psutil."""
import argparse
import concurrent.futures
import contextlib
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import platform
import re
import shutil
import socket
import ssl
import subprocess
import threading
import time
import urllib.parse
import urllib.request

import psutil
from winjob import Job

ROOT = Path(__file__).resolve().parents[2]
PORT_LOCK = threading.Lock()

def local_ports():
    sockets = [socket.socket() for _ in range(4)]
    try:
        for sock in sockets:
            sock.bind(("127.0.0.1", 0))
        return [sock.getsockname()[1] for sock in sockets]
    finally:
        for sock in sockets:
            sock.close()

def write_json(path, data):
    path.write_text(json.dumps(data, indent=2), encoding="utf-8")

def latest_client(path):
    if not path.exists():
        return {}
    for line in reversed(path.read_text(encoding="utf-8").splitlines()):
        try:
            return json.loads(line)
        except json.JSONDecodeError:
            continue
    return {}

def measure(args, root, humans, cpu):
    run = root / f"humans-{humans}"
    run.mkdir()
    server_bin = root / "bin" / "openhalcyon.exe"
    load_bin = root / "bin" / "halcyon-loadtest.exe"
    instance = run / "instance"
    subprocess.run([server_bin, "init", "--dir", instance, "--host", "127.0.0.1"], check=True, capture_output=True)
    cfg = instance / "server.toml"
    env = os.environ.copy()
    env.update({"TOKIO_WORKER_THREADS": "1", "RUST_LOG": "warn"})
    with contextlib.ExitStack() as teardown:
        with PORT_LOCK:
            bootstrap, https, notify, xmpp = local_ports()
            origin = f"https://127.0.0.1:{https}"
            text = cfg.read_text(encoding="utf-8")
            for name, port in (("bootstrap_listen", bootstrap), ("platform_listen", https), ("notify_listen", notify), ("xmpp_listen", xmpp)):
                text = re.sub(rf'(?m)^{name} = .*$', f'{name} = "127.0.0.1:{port}"', text)
            text = re.sub(r'(?m)^game_bind = .*$', 'game_bind = "127.0.0.1"', text)
            text = re.sub(r'(?m)^seats_per_match = .*$', f'seats_per_match = {humans}', text)
            text = re.sub(r'(?m)^max_rooms = .*$', f'max_rooms = {args.room_capacity or args.rooms}', text)
            text = re.sub(r'(?m)^origin = .*$', f'origin = "{origin}"', text)
            if args.data_dir:
                text = "data_dir = " + json.dumps(str(args.data_dir.resolve())) + "\n" + text
            if args.data_extra_dir:
                text = "data_extra_dir = " + json.dumps(str(args.data_extra_dir.resolve())) + "\n" + text
            cfg.write_text(text, encoding="utf-8")
            server_log = teardown.enter_context((run / "server.log").open("wb"))
            server = subprocess.Popen([server_bin, "serve", "--config", cfg], env=env, stdout=server_log, stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW)
            teardown.callback(server.wait)
            teardown.callback(server.kill)
            job = Job(server, cpu)
            teardown.callback(job.close)
            proc = psutil.Process(server.pid)
            if proc.cpu_affinity() != [cpu]:
                raise RuntimeError("server affinity was not applied")
            # Verify the port is listening before starting another scenario.
            for _ in range(250):
                if server.poll() is not None:
                    raise RuntimeError(f"server exited; inspect {run / 'server.log'}")
                try:
                    with socket.create_connection(("127.0.0.1", https), timeout=.2):
                        break
                except OSError:
                    time.sleep(.04)
            else:
                raise RuntimeError("server startup timed out")
        trust = ssl.create_default_context(cafile=instance / "tls.pem")
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=trust), urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        def admin(path, payload=None, method=None):
            req = urllib.request.Request(origin + "/api/admin/v1/" + path, headers={"Origin": origin, "Content-Type": "application/json"},
                data=None if payload is None else json.dumps(payload).encode(),method=method)
            with opener.open(req, timeout=5) as res:
                return json.load(res)
        admin("session", {"username":"admin","password":(instance/"initial-admin-password").read_text()})
        client_path = run / "client.jsonl"
        client_log = teardown.enter_context(client_path.open("wb"))
        client_err = teardown.enter_context((run / "client.err").open("wb"))
        load_env = env | {"TOKIO_WORKER_THREADS": str(args.client_workers or len(args.client_cpus))}
        client = subprocess.Popen([load_bin,"--platform",origin,"--certificate",instance/"tls.pem","--rooms",str(args.rooms),"--humans",str(humans),"--seconds",str(args.seconds),"--slow-reader-ms",str(args.slow_reader_ms)]+(["--ramp-seconds",str(args.ramp_seconds)] if args.ramp_seconds else [])+(["--allow-admin-rotation"] if args.rotate_seconds else [])+(["--native-solo"] if args.native_solo else [])+(["--native-aral"] if args.native_aral else []),
            env=load_env, stdout=client_log,stderr=client_err,creationflags=subprocess.CREATE_NO_WINDOW)
        teardown.callback(client.wait)
        teardown.callback(client.kill)
        load_proc=psutil.Process(client.pid)
        load_proc.cpu_affinity(args.client_cpus)
        if sorted(load_proc.cpu_affinity()) != sorted(set(args.client_cpus)):
            raise RuntimeError('load-process affinity was not applied')
        samples = []
        rotated = []
        t0 = time.monotonic()
        with (run/"samples.jsonl").open("w", encoding="utf-8", buffering=1) as log:
            while True:
                if server.poll() is not None:
                    raise RuntimeError("measured server exited")
                page = admin("matches?limit=100")
                rooms = page["items"]
                while cursor := page.get("nextCursor"):
                    page = admin("matches?" + urllib.parse.urlencode({"limit": 100, "cursor": cursor}))
                    rooms.extend(page["items"])
                host={}
                if args.host_telemetry:
                    overview=admin("overview")
                    hosting=overview["hosting"]
                    host={k:hosting.get(k) for k in ("admission","humanSeats","botSeats","reservedSeats","tickP99Ms","cpuPercent","rssBytes","outboundBytesPerSecond")}
                    host["queuedPlayers"]=overview["queuedPlayers"]
                cpu_times = proc.cpu_times()
                try:
                    load_times=load_proc.cpu_times()
                    load_cpu=load_times.user+load_times.system
                except psutil.NoSuchProcess:
                    load_cpu=None
                measured_at = time.monotonic()
                row = {"wall_s": measured_at-t0, "perf_counter_s": time.perf_counter(), "cpu_s": cpu_times.user+cpu_times.system, "load_cpu_s":load_cpu, "hosting":host, "rss":proc.memory_info().rss,
                    "rooms":[{k:r.get(k) for k in ("id","phase","tickLatency","fanoutQueueFull","simTick","admitted")} for r in rooms],"client":latest_client(client_path)}
                log.write(json.dumps(row,separators=(",",":"))+"\n")
                samples.append(row)
                if args.rotate_seconds and row["client"].get("steady_s", 0) is not None:
                    due=(len(rotated)+1)*args.rotate_seconds
                    if row["client"].get("steady_s", 0)>=due:
                        eligible=next((r for r in rooms if r["phase"]=="running" and r["id"] not in rotated),None)
                        if eligible:
                            admin(f"matches/{eligible['id']}/termination",{"reason":"benchmark room rotation"},method="PUT")
                            rotated.append(eligible["id"])
                if client.poll() is not None:
                    if client.returncode != 0:
                        write_json(run/"failure.json", {
                            "client_exit":client.returncode, "wall_s":row["wall_s"],
                            "last_client":row["client"], "server_rss_bytes":row["rss"],
                            "running_rooms":sum(room["phase"]=="running" for room in rooms),
                            "max_observed_tick_us":max((room["tickLatency"]["maxUs"] for room in rooms if room.get("tickLatency")),default=None),
                            "client_cpus":args.client_cpus, "server_cpu":cpu,
                        })
                        raise RuntimeError(f"load client failed: {(run/'client.err').read_text()}")
                    break
                if row["wall_s"] > args.seconds + args.ramp_seconds + 120:
                    raise RuntimeError("load client failed to complete")
                time.sleep(min(5, max(1,args.seconds//10)))
        active = [r for r in samples if r["client"].get("steady_s") is not None]
        if len(active) < 2:
            raise RuntimeError("too few steady-state samples")
        start, end = active[0], active[-1]
        elapsed = end["wall_s"]-start["wall_s"]
        last = {}
        for r in active:
            for room in r["rooms"]:
                if room["tickLatency"]:
                    last[room["id"]] = room["tickLatency"]
        if not last or not all("over40ms" in v for v in last.values()):
            raise RuntimeError("server did not supply full-window tick telemetry")
        begin = {r["id"]:r["tickLatency"] for r in start["rooms"] if r["tickLatency"]}
        ticks = sum(v["count"]-begin.get(k,{}).get("count",0) for k,v in last.items())
        over = sum(v["over40ms"]-begin.get(k,{}).get("over40ms",0) for k,v in last.items())
        tick_stages = {}
        if all("stages" in v for v in last.values()):
            for stage in ("wakeDelay", "lockWait", "roomWork"):
                totals = {
                    field: sum(v["stages"][stage][field]
                               - begin.get(k, {}).get("stages", {}).get(stage, {}).get(field, 0)
                               for k, v in last.items())
                    for field in ("totalUs", "over40ms", "dominantOver40ms")
                }
                totals["meanUs"] = totals["totalUs"] / ticks if ticks else None
                # The maxima cover each room's lifetime up to the last sample.
                totals["lifetimeMaxUs"] = max(v["stages"][stage]["maxUs"] for v in last.values())
                tick_stages[stage] = totals
            if sum(stage["dominantOver40ms"] for stage in tick_stages.values()) != over:
                raise RuntimeError("tick tail stage counts do not add up to the aggregate")
        # All intervals include wall-clock scheduling time. Keep the oldest point
        # spanning at least a minute for each rolling process-CPU observation.
        left = 0
        minute_cpu = []
        for right in active:
            while left + 1 < len(active) and right["wall_s"] - active[left + 1]["wall_s"] >= 60:
                left += 1
            interval = right["wall_s"] - active[left]["wall_s"]
            if interval >= 60:
                minute_cpu.append(100 * (right["cpu_s"] - active[left]["cpu_s"]) / interval)
        room_seconds = sum(
            sum(x["phase"] == "running" for x in prev["rooms"]) * (nxt["wall_s"] - prev["wall_s"])
            for prev, nxt in zip(active, active[1:])
        )
        byte_delta=end["client"]["gs_bytes"]-start["client"]["gs_bytes"]
        frame_delta=end["client"]["frames"]-start["client"]["frames"]
        cpu_delta=end["cpu_s"]-start["cpu_s"]
        report={"humans_per_room":humans,"bots_per_room":0 if args.native_aral else 10-humans,"rooms":args.rooms,"duration_s":elapsed,
            "logical_cpu":cpu,"commit_limit_bytes":1024**3,"server_cpu_percent":100*cpu_delta/elapsed,
            "server_cpu_seconds":cpu_delta,"decoded_gs_frames":frame_delta,"received_gs_bytes":byte_delta,
            "server_cpu_us_per_decoded_frame":cpu_delta*1e6/frame_delta if frame_delta else None,
            "max_60s_server_cpu_percent":max(minute_cpu, default=None),
            "sampled_active_room_fraction":room_seconds/(args.rooms*elapsed),
            "load_tokio_workers":args.client_workers or len(args.client_cpus),
            "peak_sampled_rss_bytes":max(r["rss"] for r in active),"ticks":ticks,"over40ms":over,
            "max_sampled_room_window_p99_us":max(v["p99Us"] or 0 for r in active for x in r["rooms"] if (v:=x["tickLatency"])),
            "max_tick_us":max(v["maxUs"] for v in last.values()),"p99_within_40ms":ticks>0 and over/ticks<=0.01,
            "tick_stages":tick_stages,
            "gs_rx_bytes_per_sec":byte_delta/elapsed,"gs_rx_bytes_per_sec_per_human":byte_delta/elapsed/(humans*args.rooms),
            "client":end["client"],"steady_samples":len(active),"intentional_rotations":len(rotated)}
        write_json(run/"result.json",report)
        return report

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--seconds",type=int,default=1800)
    parser.add_argument("--rooms",type=int,default=4)
    parser.add_argument("--room-capacity",type=int,help="provider room limit, defaults to requested rooms")
    parser.add_argument("--host-telemetry",action="store_true",help="sample admission and tick gauges via the admin overview")
    parser.add_argument("--humans",type=int,nargs="+",default=[1,5,10])
    parser.add_argument("--server-cpus",type=int,nargs="+",required=True)
    parser.add_argument("--client-workers",type=int,help="load-process Tokio workers (default: one per specified client CPU)")
    parser.add_argument("--client-cpus",type=int,nargs="+",required=True)
    parser.add_argument("--data-dir",type=Path)
    parser.add_argument("--data-extra-dir",type=Path,help="load supplemental CE rules alongside --data-dir")
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--native-aral",action="store_true",help="six native ARAL seats per room; use --humans 6")
    modes.add_argument("--native-solo",action="store_true",help="one stock-protocol seat and nine bots per room; use --humans 1")
    parser.add_argument("--slow-reader-ms",type=int,default=0)
    parser.add_argument("--ramp-seconds",type=int,default=0)
    parser.add_argument("--rotate-seconds",type=int,default=0)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--binary-dir",type=Path,default=ROOT/"target"/"release")
    args=parser.parse_args()
    if args.data_extra_dir and not args.data_dir:
        parser.error("--data-extra-dir requires --data-dir")
    if args.native_aral and args.humans != [6]:
        parser.error("--native-aral requires --humans 6")
    if args.native_solo and args.humans != [1]:
        parser.error("--native-solo requires --humans 1")
    if os.name!="nt" or (args.room_capacity is not None and args.room_capacity<args.rooms) or (args.client_workers is not None and args.client_workers<1) or len(args.server_cpus)!=len(args.humans) or len(set(args.server_cpus))!=len(args.server_cpus) or set(args.server_cpus)&set(args.client_cpus):
        parser.error("provide one distinct server CPU per scenario and separate client CPUs on Windows")
    root=args.output.resolve()
    root.mkdir(parents=True,exist_ok=False)
    binaries=root/"bin";binaries.mkdir()
    sha={}
    for name in ("openhalcyon.exe","halcyon-loadtest.exe"):
        dst=binaries/name;shutil.copy2(args.binary_dir/name,dst)
        sha[name]=hashlib.sha256(dst.read_bytes()).hexdigest()
    paths = {key: str(value) if value is not None else None for key, value in vars(args).items()
             if isinstance(value, Path) or key in ("data_dir", "data_extra_dir")}
    write_json(root/"environment.json",{"platform":platform.platform(),"cpu":platform.processor(),"logical_cpus":psutil.cpu_count(),"logical_to_physical_count":psutil.cpu_count(logical=False),"timestamp":time.time(),"binaries_sha256":sha,"git_head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip(),"dirty":bool(subprocess.check_output(["git","status","--porcelain"],cwd=ROOT)),"args":vars(args)|paths|{"output":str(root)}})
    with concurrent.futures.ThreadPoolExecutor() as executor:
        tasks=[executor.submit(measure,args,root,h,c) for h,c in zip(args.humans,args.server_cpus)]
        results=[t.result() for t in tasks]
    write_json(root/"results.json",results)
    print(json.dumps(results,indent=2))
if __name__=="__main__":
    main()
