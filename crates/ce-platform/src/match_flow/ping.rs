use crate::{Error, Platform};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

const MAX_SITES: usize = 32;
// A silent local TCP peer produced 2000+ ms samples in Android 147219.
const NATIVE_TIMEOUT_MS: f64 = 2_000.;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PingSite {
    pub site: String,
    pub host: String,
    pub port: u16,
    pub node_id: Uuid,
}
#[derive(Default)]
pub(crate) struct Sites(Vec<PingSite>);
impl Sites {
    fn new(entries: Vec<PingSite>) -> Result<Self, Error> {
        let mut labels = HashSet::new();
        let mut nodes = HashSet::new();
        if entries.len() > MAX_SITES
            || entries.iter().any(|s| {
                let host = &s.host;
                host.is_empty()
                    || host.len() > 253
                    || !(host.parse::<std::net::IpAddr>().is_ok()
                        || host.split('.').all(|part| {
                            !part.is_empty()
                                && part.len() <= 63
                                && !part.starts_with('-')
                                && !part.ends_with('-')
                                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        }))
                    || s.site.is_empty()
                    || s.site.len() > 64
                    || !s
                        .site
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
                    || s.port == 0
                    || s.node_id.is_nil()
                    || !labels.insert(s.site.as_str())
                    || !nodes.insert(s.node_id)
            })
        {
            return Err(Error::Config("invalid or duplicate ping site"));
        }
        Ok(Self(entries))
    }
    pub fn announcement(&self) -> Vec<SiteAnnouncement<'_>> {
        self.0
            .iter()
            .map(|s| SiteAnnouncement {
                site: &s.site,
                host: &s.host,
                port: s.port,
            })
            .collect()
    }
    pub fn report(&self, sites: &Value) -> BTreeMap<Uuid, u32> {
        let mut values = BTreeMap::new();
        let Some(rows) = sites.as_array().filter(|rows| rows.len() <= MAX_SITES) else {
            return values;
        };
        for row in rows {
            let Ok(row) = ReportedSite::deserialize(row) else {
                continue;
            };
            let Some(site) = self
                .0
                .iter()
                .find(|s| row.site == s.site && row.host == s.host)
            else {
                continue;
            };
            if [row.avg, row.current, row.max, row.std_dev]
                .into_iter()
                .all(|v| v.is_finite() && (0.0..NATIVE_TIMEOUT_MS).contains(&v))
            {
                let ms = row.avg.max(row.current).max(row.max).ceil().max(1.) as u32;
                values
                    .entry(site.node_id)
                    .and_modify(|v: &mut u32| *v = (*v).max(ms))
                    .or_insert(ms);
            }
        }
        values
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SiteAnnouncement<'a> {
    site: &'a str,
    host: &'a str,
    port: u16,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportedSite<'a> {
    site: &'a str,
    host: &'a str,
    avg: f64,
    current: f64,
    std_dev: f64,
    max: f64,
}

impl Platform {
    pub fn configure_ping_sites(
        platform: &mut Arc<Self>,
        entries: Vec<PingSite>,
    ) -> Result<(), Error> {
        Arc::get_mut(platform)
            .ok_or(Error::Config("configure sites before sharing the platform"))?
            .sites = Sites::new(entries)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binds_reports_to_advertised_hosts_and_caps_work() {
        let id = Uuid::new_v4();
        let s = PingSite {
            site: "lab".into(),
            host: "127.0.0.1".into(),
            port: 1119,
            node_id: id,
        };
        assert!(Sites::new(vec![s.clone(), s.clone()]).is_err());
        let sites = Sites::new(vec![s]).unwrap();
        assert_eq!(
            sites.announcement(),
            [SiteAnnouncement {
                site: "lab",
                host: "127.0.0.1",
                port: 1119
            }]
        );
        let valid = serde_json::to_value(ReportedSite {
            site: "lab",
            host: "127.0.0.1",
            avg: 6.4,
            current: 5.,
            std_dev: 1.,
            max: 11.2,
        })
        .unwrap();
        assert_eq!(
            sites.report(&Value::Array(vec![valid.clone()])),
            BTreeMap::from([(id, 12)])
        );
        for (k, v) in [
            ("site", Value::from("absent")),
            ("host", Value::from("other")),
            ("stdDev", Value::from(-1)),
            ("max", Value::from(2000.032)),
            ("avg", Value::from("NaN")),
        ] {
            let mut row = valid.clone();
            row[k] = v;
            assert!(sites.report(&Value::Array(vec![row])).is_empty());
        }
        assert!(
            sites
                .report(&Value::Array(vec![valid; MAX_SITES + 1]))
                .is_empty()
        );
    }
}
