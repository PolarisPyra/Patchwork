//! The `reply_costs` table: the costs of replies kept after they were deleted.

// Rust guideline compliant 2026-10-09

use rusqlite::params;

use super::store::Store;
use super::{dec, enc, err};
use crate::Error;
use crate::costs::ReplyCost;
use crate::threads::TokenUsage;

impl Store {
    /// Saves replies' costs, replacing any kept before under the same message.
    ///
    /// # Errors
    ///
    /// Returns an error when the database write fails.
    pub(crate) fn save_reply_costs(&self, rows: &[ReplyCost]) -> Result<(), Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        for r in rows {
            tx.execute(
                "INSERT OR REPLACE INTO reply_costs (message_id, thread_id, title, project, agent,
                 model, at, input, cache_read, cache_write, output, cost_micros)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    r.message_id,
                    r.thread_id,
                    r.title,
                    r.project,
                    enc(&r.agent),
                    r.model,
                    r.at,
                    r.usage.input,
                    r.usage.cache_read,
                    r.usage.cache_write,
                    r.usage.output,
                    r.usage.cost_micros,
                ],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(())
    }

    /// Every reply cost kept.
    pub(crate) fn reply_costs(&self) -> Vec<ReplyCost> {
        let inner = self.lock();
        let rows = inner
            .conn
            .prepare(
                "SELECT message_id, thread_id, title, project, agent, model, at, input,
                 cache_read, cache_write, output, cost_micros FROM reply_costs",
            )
            .and_then(|mut stmt| {
                stmt.query_map([], |r| {
                    Ok(ReplyCost {
                        message_id: r.get(0)?,
                        thread_id: r.get(1)?,
                        title: r.get(2)?,
                        project: r.get(3)?,
                        agent: dec(r.get(4)?)?,
                        model: r.get(5)?,
                        at: r.get(6)?,
                        usage: TokenUsage {
                            input: r.get(7)?,
                            cache_read: r.get(8)?,
                            cache_write: r.get(9)?,
                            output: r.get(10)?,
                            cost_micros: r.get(11)?,
                            ..TokenUsage::default()
                        },
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
            });
        rows.unwrap_or_else(|e| {
            log::warn!(error:% = e; "could not read the kept reply costs: {{error}}");
            Vec::new()
        })
    }
}
