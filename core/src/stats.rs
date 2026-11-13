//! Weekly counts of words checked and fixes applied. Local only, like insights; no text is kept.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Week {
    pub words: u64,
    pub fixes: u64,
}

/// This week and the one before, for the menu-bar panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WeekSummary {
    pub this: Week,
    pub last: Week,
}

const KEEP_WEEKS: usize = 8;

/// Monday-based week number; day 0 (1970-01-01) was a Thursday.
fn week_of(day: u64) -> u64 {
    (day + 3) / 7
}

pub struct StatsStore {
    dir: PathBuf,
    weeks: BTreeMap<u64, Week>,
}

impl StatsStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let weeks = fs::read(dir.join("stats.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { dir, weeks }
    }

    /// `day` is days since the Unix epoch.
    pub fn record(&mut self, day: u64, words: u64, fixes: u64) {
        let w = self.weeks.entry(week_of(day)).or_default();
        w.words += words;
        w.fixes += fixes;
        while self.weeks.len() > KEEP_WEEKS {
            self.weeks.pop_first();
        }
        let _ = self.save();
    }

    pub fn summary(&self, day: u64) -> WeekSummary {
        let wk = week_of(day);
        let get = |w: u64| self.weeks.get(&w).copied().unwrap_or_default();
        WeekSummary { this: get(wk), last: wk.checked_sub(1).map(get).unwrap_or_default() }
    }

    fn save(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        fs::write(self.dir.join("stats.json"), serde_json::to_string(&self.weeks)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    // 2026-11-09 is a Monday.
    const MON: u64 = 20766;

    #[test]
    fn weeks_start_on_monday() {
        assert_eq!(week_of(MON - 1) + 1, week_of(MON));
        assert_eq!(week_of(MON), week_of(MON + 6));
    }

    #[test]
    fn sums_this_week_and_last_and_persists() {
        let dir = TempDir::new();
        let mut s = StatsStore::new(dir.path());
        s.record(MON - 3, 10, 1);
        s.record(MON, 20, 2);
        s.record(MON + 6, 5, 0);
        let want = WeekSummary { this: Week { words: 25, fixes: 2 }, last: Week { words: 10, fixes: 1 } };
        assert_eq!(s.summary(MON + 2), want);
        assert_eq!(StatsStore::new(dir.path()).summary(MON + 2), want);
        assert_eq!(s.summary(MON + 14), WeekSummary::default());
    }

    #[test]
    fn keeps_only_the_last_eight_weeks() {
        let dir = TempDir::new();
        let mut s = StatsStore::new(dir.path());
        for i in 0..10 {
            s.record(MON + i * 7, 1, 0);
        }
        assert_eq!(s.weeks.len(), KEEP_WEEKS);
        assert_eq!(s.summary(MON + 9 * 7).last, Week { words: 1, fixes: 0 });
    }
}
