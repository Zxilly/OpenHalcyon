use super::*;
use cpu_time::ProcessTime;
use serde::{Deserialize, Serialize};
use std::{io, net::SocketAddr, sync::atomic::AtomicU32, time::Duration};

const POLL: Duration = Duration::from_secs(2);
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdmissionPolicy {
    pub tick_p99_ms: Option<u64>,
    /// 100 percent is one logical processor, regardless of host core count.
    pub process_cpu_percent: Option<f64>,
    pub rss_bytes: Option<u64>,
    /// Game-socket payload plus the GS length headers.
    pub egress_bytes_per_second: Option<u64>,
}
impl Default for AdmissionPolicy {
    fn default() -> Self {
        Self {
            tick_p99_ms: Some(40),
            process_cpu_percent: None,
            rss_bytes: None,
            egress_bytes_per_second: None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortRange {
    pub first: u16,
    pub last: u16,
}
pub(crate) struct Ports {
    range: Option<PortRange>,
    next: AtomicU32,
    retry: Mutex<Option<Instant>>,
}
impl Ports {
    pub fn new(range: Option<PortRange>) -> Result<Self, PlatformError> {
        if range.is_some_and(|r| r.first == 0 || r.last < r.first) {
            return Err(PlatformError::Invalid("invalid game port range"));
        }
        Ok(Self {
            range,
            next: AtomicU32::new(0),
            retry: Mutex::default(),
        })
    }
    pub fn slots(&self) -> usize {
        self.range
            .map(|r| usize::from(r.last - r.first) + 1)
            .unwrap_or(usize::MAX)
    }
    pub fn exhausted(&self) -> bool {
        self.retry
            .lock()
            .unwrap()
            .is_some_and(|t| t > Instant::now())
    }
    pub fn bind(&self, ip: IpAddr) -> io::Result<TcpListener> {
        self.bind_with(|port| TcpListener::bind(SocketAddr::new(ip, port)))
    }

    fn bind_with<T>(&self, mut bind: impl FnMut(u16) -> io::Result<T>) -> io::Result<T> {
        let Some(range) = self.range else {
            return bind(0);
        };
        let total = self.slots() as u32;
        let start = self.next.load(Ordering::Relaxed);
        for offset in 0..total {
            let index = (start + offset) % total;
            let port = range.first + index as u16;
            match bind(port) {
                Ok(listener) => {
                    self.next.store((index + 1) % total, Ordering::Relaxed);
                    *self.retry.lock().unwrap() = None;
                    return Ok(listener);
                }
                Err(err) if err.kind() == io::ErrorKind::AddrInUse => continue,
                Err(err) => return Err(err),
            }
        }
        *self.retry.lock().unwrap() = Some(Instant::now() + POLL);
        Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "configured game ports are occupied",
        ))
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceMetrics {
    pub sampled_at: DateTime<Utc>,
    pub process_cpu_percent: Option<f64>,
    pub rss_bytes: Option<u64>,
    pub egress_bytes_per_second: Option<u64>,
    pub tick_p99_ms: Option<f64>,
    pub overloaded: bool,
    pub reason: Option<&'static str>,
}
pub(crate) struct Gate {
    policy: AdmissionPolicy,
    last: Instant,
    cpu: Option<ProcessTime>,
    bytes: u64,
    hot: u8,
    cool: u8,
    metrics: ResourceMetrics,
}
impl Gate {
    pub fn new(policy: AdmissionPolicy) -> Result<Self, PlatformError> {
        if policy.tick_p99_ms == Some(0)
            || policy.rss_bytes == Some(0)
            || policy.egress_bytes_per_second == Some(0)
            || policy
                .process_cpu_percent
                .is_some_and(|p| !p.is_finite() || p <= 0.)
        {
            return Err(PlatformError::Invalid("invalid resource admission budget"));
        }
        let rss = memory_stats::memory_stats().map(|m| m.physical_mem as u64);
        let mut this = Self {
            policy,
            last: Instant::now(),
            cpu: ProcessTime::try_now().ok(),
            bytes: 0,
            hot: 0,
            cool: 0,
            metrics: ResourceMetrics {
                sampled_at: Utc::now(),
                rss_bytes: rss,
                process_cpu_percent: None,
                egress_bytes_per_second: None,
                tick_p99_ms: None,
                overloaded: false,
                reason: None,
            },
        };
        this.evaluate();
        Ok(this)
    }
    fn evaluate(&mut self) {
        let m = &mut self.metrics;
        // The configured RSS cap is a hard boundary. Rate and latency budgets
        // require two consecutive hot samples. Reopen only after three cool ones.
        let hard = self
            .policy
            .rss_bytes
            .is_some_and(|limit| m.rss_bytes.is_none_or(|rss| rss >= limit));
        let tests = [
            (
                "rss",
                m.rss_bytes.map(|v| v as f64),
                self.policy.rss_bytes.map(|v| v as f64),
            ),
            (
                "cpu",
                m.process_cpu_percent,
                self.policy.process_cpu_percent,
            ),
            (
                "egress",
                m.egress_bytes_per_second.map(|v| v as f64),
                self.policy.egress_bytes_per_second.map(|v| v as f64),
            ),
            (
                "tick",
                m.tick_p99_ms,
                self.policy.tick_p99_ms.map(|v| v as f64),
            ),
        ];
        let culprit = tests
            .iter()
            .find(|(_, v, max)| v.zip(*max).is_some_and(|(v, max)| v > max))
            .map(|t| t.0);
        if hard || culprit.is_some() {
            self.hot = self.hot.saturating_add(1);
            self.cool = 0;
            if hard || self.hot >= 2 {
                m.overloaded = true;
                m.reason = if hard { Some("rss") } else { culprit };
            }
        } else {
            self.hot = 0;
            let cool = tests
                .iter()
                .all(|(_, v, max)| v.zip(*max).is_none_or(|(v, max)| v <= max * 0.85));
            if cool {
                self.cool = self.cool.saturating_add(1)
            } else {
                self.cool = 0
            }
            if self.cool >= 3 {
                m.overloaded = false;
                m.reason = None;
            }
        }
    }
    fn observe(&mut self, bytes: u64, p99: Option<u64>) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        if elapsed < 1. {
            return;
        }
        let cpu = ProcessTime::try_now().ok();
        let pct = cpu
            .zip(self.cpu)
            .map(|(n, o)| 100. * n.duration_since(o).as_secs_f64() / elapsed);
        let m = &mut self.metrics;
        m.sampled_at = Utc::now();
        m.process_cpu_percent = pct;
        m.rss_bytes = memory_stats::memory_stats().map(|m| m.physical_mem as u64);
        m.egress_bytes_per_second =
            Some((bytes.saturating_sub(self.bytes) as f64 / elapsed).round() as u64);
        m.tick_p99_ms = p99.map(|us| us as f64 / 1000.);
        self.last = now;
        self.cpu = cpu;
        self.bytes = bytes;
        self.evaluate();
    }
}
impl Rooms {
    pub(crate) fn monitor_resources(&self) {
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.0.monitor_started.swap(true, Ordering::Relaxed) {
            return;
        }
        let weak = Arc::downgrade(&self.0);
        rt.spawn(async move {
            loop {
                tokio::time::sleep(POLL).await;
                let Some(m) = weak.upgrade() else { break };
                let _ = tokio::task::spawn_blocking(move || {
                    let rooms: Vec<_> = m.rooms.lock().unwrap().values().cloned().collect();
                    let peak = rooms
                        .into_iter()
                        .filter_map(|r| {
                            let r = r.lock().unwrap();
                            if r.phase != "running" || r.latency.recent.len() < 20 {
                                return None;
                            }
                            let mut tail: Vec<_> = r.latency.recent.iter().copied().collect();
                            drop(r);
                            let nth = (tail.len() * 99).div_ceil(100) - 1;
                            let (_, p, _) = tail.select_nth_unstable(nth);
                            Some(*p)
                        })
                        .max();
                    m.gate
                        .lock()
                        .unwrap()
                        .observe(m.bytes.load(Ordering::Relaxed), peak);
                })
                .await;
            }
        });
    }
    pub fn resource_metrics(&self) -> ResourceMetrics {
        self.0.gate.lock().unwrap().metrics.clone()
    }
    pub fn available_rooms(&self) -> usize {
        self.free_with_count(self.active_rooms())
    }
    pub(crate) fn free_with_count(&self, occupied: usize) -> usize {
        if self.0.ports.exhausted() || self.0.gate.lock().unwrap().metrics.overloaded {
            return 0;
        }
        self.capacity().saturating_sub(occupied)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overload_and_recovery_require_sustained_samples() {
        let mut g = Gate::new(AdmissionPolicy {
            egress_bytes_per_second: Some(100),
            ..AdmissionPolicy::default()
        })
        .unwrap();
        g.metrics.egress_bytes_per_second = Some(110);
        g.evaluate();
        assert!(!g.metrics.overloaded);
        g.evaluate();
        assert_eq!(g.metrics.reason, Some("egress"));
        g.metrics.egress_bytes_per_second = Some(90);
        for _ in 0..4 {
            g.evaluate()
        }
        assert!(g.metrics.overloaded);
        g.metrics.egress_bytes_per_second = Some(70);
        for _ in 0..3 {
            g.evaluate()
        }
        assert!(!g.metrics.overloaded);
        g.metrics.tick_p99_ms = Some(44.);
        g.evaluate();
        g.evaluate();
        assert_eq!(g.metrics.reason, Some("tick"));
    }
    #[tokio::test]
    async fn pressure_blocks_new_matches_and_preserves_reservations() {
        let rooms = Rooms::new(IpAddr::from([127, 0, 0, 1]), "127.0.0.1".into(), 2, None);
        let req = Reservation {
            match_id: Uuid::new_v4(),
            players: vec![Uuid::new_v4()],
            teams: vec![],
            mode: "practice".into(),
            difficulty: None,
        };
        let first = rooms.prepare(req.clone()).unwrap();
        {
            let mut gate = rooms.0.gate.lock().unwrap();
            gate.metrics.tick_p99_ms = Some(50.);
            gate.evaluate();
            gate.evaluate();
        }
        assert_eq!(rooms.available_rooms(), 0);
        assert_eq!(rooms.prepare(req.clone()).unwrap().address, first.address);
        rooms.activate(req.match_id).unwrap();
        assert!(matches!(
            rooms.reserve(Reservation {
                match_id: Uuid::new_v4(),
                ..req
            }),
            Err(PlatformError::HostUnavailable)
        ));
        assert_eq!(rooms.active_rooms(), 1);
    }
    #[test]
    fn range_scans_occupied_ports_and_wraps() {
        let ports = Ports::new(Some(PortRange {
            first: 54000,
            last: 54001,
        }))
        .unwrap();
        let try_bind = |occupied: &[u16], port| {
            if occupied.contains(&port) {
                Err(io::Error::from(io::ErrorKind::AddrInUse))
            } else {
                Ok(port)
            }
        };
        assert_eq!(
            ports
                .bind_with(|port| try_bind(&[54000, 54001], port))
                .unwrap_err()
                .kind(),
            io::ErrorKind::AddrInUse
        );
        assert!(ports.exhausted());
        assert_eq!(
            ports.bind_with(|port| try_bind(&[54000], port)).unwrap(),
            54001
        );
        assert!(!ports.exhausted());
        assert_eq!(ports.bind_with(Ok).unwrap(), 54000);
        assert_eq!(Ports::new(None).unwrap().bind_with(Ok).unwrap(), 0);
    }
}
