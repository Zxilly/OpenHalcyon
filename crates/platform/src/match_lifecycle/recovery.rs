use diesel::prelude::*;
use uuid::Uuid;

use crate::{
    AbortReason, Result,
    events::orm_schema::matches,
    service::{AppState, abort, parse_id},
};

impl AppState {
    /// Release local matches whose in-memory host disappeared. Cross-instance
    /// assignments are reconciled separately against committed pool state.
    pub fn recover_local_rooms(&self, is_running: impl Fn(Uuid) -> bool) -> Result<usize> {
        self.transaction(|db| {
            let ids = matches::table
                .filter(matches::phase.eq("playing"))
                .select(matches::id)
                .load::<String>(db)?;
            let mut stopped = 0;
            for id in ids {
                if !is_running(parse_id(&id)?) {
                    abort(db, &id, AbortReason::HostRestarted)?;
                    stopped += 1;
                }
            }
            Ok(stopped)
        })
    }
}
