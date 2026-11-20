use crate::{AppState, Result, abort, parse_id};
use uuid::Uuid;

impl AppState {
    /// Release local matches whose in-memory host disappeared. Cross-instance
    /// assignments are reconciled separately against committed pool state.
    pub fn recover_local_rooms(&self, is_running: impl Fn(Uuid) -> bool) -> Result<usize> {
        self.transaction(|tx| {
            let mut query = tx.prepare("SELECT id FROM matches WHERE phase='playing'")?;
            let ids = query
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut stopped = 0;
            for id in ids {
                if !is_running(parse_id(&id)?) {
                    abort(tx, &id, crate::AbortReason::HostRestarted)?;
                    stopped += 1;
                }
            }
            Ok(stopped)
        })
    }
}
