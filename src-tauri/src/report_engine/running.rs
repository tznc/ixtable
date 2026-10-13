//! Running sums of one printed report: totals so far by component id. A "group" sum
//! restarts at each instance of the enclosing group.

use super::component::{Band, Comp, Group};
use super::render::ScopeData;
use super::values::{accumulate, RunningValue};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub struct RunningSums {
    sums: HashMap<String, RunningValue>,
    now: Option<DateTime<Utc>>,
}

fn running_of(band: &Band) -> impl Iterator<Item = &Comp> {
    band.components.iter().filter(|c| c.is_running())
}

impl RunningSums {
    pub fn new(now: Option<DateTime<Utc>>) -> Self {
        Self {
            sums: HashMap::new(),
            now,
        }
    }

    fn restart(&mut self, band: &Band) {
        for c in running_of(band) {
            if c.running_sum.as_deref() == Some("group") {
                self.sums.remove(&c.id);
            }
        }
    }

    /// Adds the instance to the sums of its band's running components and returns the
    /// values to print (`None` when the band has no running component).
    pub fn counted(&mut self, band: &Band, scope: &ScopeData) -> Option<HashMap<String, RunningValue>> {
        let comps: Vec<&Comp> = running_of(band).collect();
        if comps.is_empty() {
            return None;
        }
        let mut running = HashMap::new();
        for c in comps {
            let next = accumulate(self.sums.get(&c.id), c, scope, self.now);
            self.sums.insert(c.id.clone(), next.clone());
            running.insert(c.id.clone(), next);
        }
        Some(running)
    }

    /// A new instance of group `level` starts: bands one level down restart "group" sums.
    pub fn restart_below(&mut self, groups: &[Group], level: usize, detail: &Band) {
        match groups.get(level + 1) {
            Some(inner) => {
                self.restart(&inner.header);
                self.restart(&inner.footer);
            }
            None => self.restart(detail),
        }
    }
}
