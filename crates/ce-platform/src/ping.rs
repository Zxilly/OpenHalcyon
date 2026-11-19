use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

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
pub(super) struct Sites(Vec<PingSite>);
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
    pub fn announcement(&self) -> Value {
        Value::Array(
            self.0
                .iter()
                .map(|s| json!({"site":s.site,"host":s.host,"port":s.port}))
                .collect(),
        )
    }
    pub fn report(&self, sites: &Value) -> BTreeMap<Uuid, u32> {
        let mut values = BTreeMap::new();
        let Some(rows) = sites.as_array().filter(|rows| rows.len() <= MAX_SITES) else {
            return values;
        };
        for row in rows {
            let Some(site) = self.0.iter().find(|s| {
                row["site"].as_str() == Some(&s.site) && row["host"].as_str() == Some(&s.host)
            }) else {
                continue;
            };
            let number = |name: &str| {
                row[name]
                    .as_f64()
                    .filter(|v| v.is_finite() && (0.0..NATIVE_TIMEOUT_MS).contains(v))
            };
            if let (Some(avg), Some(current), Some(max), Some(_)) = (
                number("avg"),
                number("current"),
                number("max"),
                number("stdDev"),
            ) {
                let ms = avg.max(current).max(max).ceil().max(1.) as u32;
                values
                    .entry(site.node_id)
                    .and_modify(|v: &mut u32| *v = (*v).max(ms))
                    .or_insert(ms);
            }
        }
        values
    }
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
            json!([{"site":"lab","host":"127.0.0.1","port":1119}])
        );
        let valid =
            json!({"site":"lab","host":"127.0.0.1","avg":6.4,"current":5,"stdDev":1,"max":11.2});
        assert_eq!(sites.report(&json!([valid])), BTreeMap::from([(id, 12)]));
        for (k, v) in [
            ("site", json!("absent")),
            ("host", json!("other")),
            ("stdDev", json!(-1)),
            ("max", json!(2000.032)),
            ("avg", json!("NaN")),
        ] {
            let mut row = valid.clone();
            row[k] = v;
            assert!(sites.report(&json!([row])).is_empty());
        }
        assert!(
            sites
                .report(&Value::Array(vec![valid; MAX_SITES + 1]))
                .is_empty()
        );
    }
}
