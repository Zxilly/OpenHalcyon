# 对局服容量测试

`halcyon-loadtest` 是独立的 CE 协议客户端。每个真人席位都创建游客、加入队列、接受对局，随后建立选人和游戏两条 TCP 连接。客户端使用平台返回的 matchId 和地址，锁定英雄，解密下行帧，持续发送时钟和移动。每个房间的队列名独立，房间结束后同一批账号重新入队。运行前用 release 构建生成所需的两个程序。

```powershell
cargo build --release -p openhalcyon -p halcyon-loadtest
python -m pip install psutil
python -B tools/bench/windows.py --rooms 4 --seconds 1810 --server-cpus 4 8 12 --client-cpus 20 22 24 --data-dir ../vg/_ana/delivery --output runtime/perf-run
```

上例要求机器至少提供列出的逻辑 CPU。根据本机 CPU 拓扑选择互不重叠的服务端和客户端核，使用新的输出目录。省略 `--data-dir` 时使用内置验证地图和 Ringo 数据。每个场景会生成一套新的本地证书、账号库和随机回环端口。脚本创建 Windows Job Object，将每个服务端限制到一颗指定逻辑 CPU，进程提交内存上限 1 GiB，并设置单个 Tokio 工作线程。CPU、RSS 仅采样服务端 PID。负载客户端与服务端运行在不同进程；同机回环测试仍共享宿主机和缓存。

默认并行运行每局 1、5、10 个真人席位三个场景，每局总席位均为 10。用 `--humans 5 --server-cpus 4` 可以单独跑一档。按房间数逐档测试时，为每次运行指定独立目录。输出中记录构建二进制 SHA-256、工作区提交、CPU 掩码、系统版本及场景参数。

- `samples.jsonl` 每五秒记录 RSS、累积 CPU 时间、房间状态、服务端 tick 计数以及客户端累计读写数。短运行使用更密的采样。延迟从该 tick 的计划时刻计到模拟及广播入队结束。
- `client.jsonl` 记录完整解密的 GS 帧和字节、位置帧、命令、已开和已结束的房间数。字节统计含 GS 长度头，不计 TLS 平台请求、TCP/IP 头和重传。
- `result.json` 使用所有初始房间均已连接后的采样区间。通过累积 `over40ms` 与 tick 数精确判断该区间内是否至少 99% 的 tick 在 40 ms 内完成。`max_sampled_room_window_p99_us` 是采样时各房间最近最多 1024 步的 p99 中的最大值。RSS 是采样峰值。
- `--ramp-seconds 90` 将各组初次登录均匀错开 90 秒。全部不同房间都完成两条游戏连接后开始计时。默认各组同时启动。汇总中的 `max_60s_server_cpu_percent` 是至少一分钟滚动窗口内的最高服务端 CPU 使用率。
- `--slow-reader-ms 250` 在每个房间的最后一个真人游戏连接上设置 4 KiB 接收缓冲区，每次收到一帧后暂停 250 ms。`--rotate-seconds 30` 周期性通过鉴权管理端终止一个房间，使客户端重新排队，覆盖连续开局。仅使用本次脚本新建的实例运行。
- 1 GiB 的 Windows 提交限制与 Linux cgroup 的内存计量不同。正式设定准入上限还需要运营者实际硬件及带宽预算，并对临界档进行至少两小时测试。远程负载机可以独立运行 `halcyon-loadtest --platform https://HOST:PORT --certificate tls.pem --rooms N --humans H --seconds S`；被测服需配置相应的真人席位数、房间容量和可达地址。

临时实例含管理员初始凭据、证书私钥和游客数据库。发布结果只需脱敏的环境、采样与汇总文件。
