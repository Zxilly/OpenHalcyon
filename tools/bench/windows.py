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
import urllib.request

import psutil
from winjob import Job

ROOT = Path(__file__).resolve().parents[2]
PORT_LOCK = threading.Lock()

def local_ports():
    sockets = [socket.socket() for _ in range(3)]
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
            bootstrap, https, notify = local_ports()
            origin = f"https://127.0.0.1:{https}"
            text = cfg.read_text(encoding="utf-8")
            for name, port in (("bootstrap_listen", bootstrap), ("platform_listen", https), ("notify_listen", notify)):
                text = re.sub(rf'(?m)^{name} = .*$', f'{name} = "127.0.0.1:{port}"', text)
            text = re.sub(r'(?m)^game_bind = .*$', 'game_bind = "127.0.0.1"', text)
            text = re.sub(r'(?m)^seats_per_match = .*$', f'seats_per_match = {humans}', text)
            text = re.sub(r'(?m)^max_rooms = .*$', f'max_rooms = {args.rooms}', text)
            text = re.sub(r'(?m)^origin = .*$', f'origin = "{origin}"', text)
            if args.data_dir:
                text = "data_dir = " + json.dumps(str(args.data_dir.resolve())) + "\n" + text
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
        client = subprocess.Popen([load_bin,"--platform",origin,"--certificate",instance/"tls.pem","--rooms",str(args.rooms),"--humans",str(humans),"--seconds",str(args.seconds),"--slow-reader-ms",str(args.slow_reader_ms)]+(["--allow-admin-rotation"] if args.rotate_seconds else []),
            env=env, stdout=client_log,stderr=client_err,creationflags=subprocess.CREATE_NO_WINDOW)
        teardown.callback(client.wait)
        teardown.callback(client.kill)
        psutil.Process(client.pid).cpu_affinity(args.client_cpus)
        samples = []
        rotated = []
        t0 = time.monotonic()
        with (run/"samples.jsonl").open("w", encoding="utf-8", buffering=1) as log:
            while True:
                if server.poll() is not None:
                    raise RuntimeError("measured server exited")
                rooms = admin("matches?limit=100")["items"]
                cpu_times = proc.cpu_times()
                row = {"wall_s": time.monotonic()-t0, "cpu_s": cpu_times.user+cpu_times.system, "rss":proc.memory_info().rss,
                    "rooms":[{k:r.get(k) for k in ("id","phase","tickLatency")} for r in rooms],"client":latest_client(client_path)}
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
                        raise RuntimeError(f"load client failed: {(run/'client.err').read_text()}")
                    break
                if row["wall_s"] > args.seconds + 120:
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
        byte_delta=end["client"]["gs_bytes"]-start["client"]["gs_bytes"]
        report={"humans_per_room":humans,"bots_per_room":10-humans,"rooms":args.rooms,"duration_s":elapsed,
            "logical_cpu":cpu,"commit_limit_bytes":1024**3,"server_cpu_percent":100*(end["cpu_s"]-start["cpu_s"])/elapsed,
            "peak_sampled_rss_bytes":max(r["rss"] for r in active),"ticks":ticks,"over40ms":over,
            "max_sampled_room_window_p99_us":max(v["p99Us"] or 0 for r in active for x in r["rooms"] if (v:=x["tickLatency"])),
            "max_tick_us":max(v["maxUs"] for v in last.values()),"p99_within_40ms":ticks>0 and over/ticks<=0.01,
            "gs_rx_bytes_per_sec":byte_delta/elapsed,"gs_rx_bytes_per_sec_per_human":byte_delta/elapsed/(humans*args.rooms),
            "client":end["client"],"steady_samples":len(active),"intentional_rotations":len(rotated)}
        write_json(run/"result.json",report)
        return report

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--seconds",type=int,default=1800)
    parser.add_argument("--rooms",type=int,default=4)
    parser.add_argument("--humans",type=int,nargs="+",default=[1,5,10])
    parser.add_argument("--server-cpus",type=int,nargs="+",required=True)
    parser.add_argument("--client-cpus",type=int,nargs="+",required=True)
    parser.add_argument("--data-dir",type=Path)
    parser.add_argument("--slow-reader-ms",type=int,default=0)
    parser.add_argument("--rotate-seconds",type=int,default=0)
    parser.add_argument("--output",type=Path,required=True)
    args=parser.parse_args()
    if os.name!="nt" or len(args.server_cpus)!=len(args.humans) or len(set(args.server_cpus))!=len(args.server_cpus) or set(args.server_cpus)&set(args.client_cpus):
        parser.error("provide one distinct server CPU per scenario and separate client CPUs on Windows")
    root=args.output.resolve()
    root.mkdir(parents=True,exist_ok=False)
    binaries=root/"bin";binaries.mkdir()
    sha={}
    for name in ("openhalcyon.exe","halcyon-loadtest.exe"):
        dst=binaries/name;shutil.copy2(ROOT/"target"/"release"/name,dst)
        sha[name]=hashlib.sha256(dst.read_bytes()).hexdigest()
    write_json(root/"environment.json",{"platform":platform.platform(),"cpu":platform.processor(),"logical_cpus":psutil.cpu_count(),"logical_to_physical_count":psutil.cpu_count(logical=False),"timestamp":time.time(),"binaries_sha256":sha,"git_head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip(),"dirty":bool(subprocess.check_output(["git","status","--porcelain"],cwd=ROOT)),"args":vars(args)|{"output":str(root),"data_dir":str(args.data_dir)}})
    with concurrent.futures.ThreadPoolExecutor() as executor:
        tasks=[executor.submit(measure,args,root,h,c) for h,c in zip(args.humans,args.server_cpus)]
        results=[t.result() for t in tasks]
    write_json(root/"results.json",results)
    print(json.dumps(results,indent=2))
if __name__=="__main__":
    main()
