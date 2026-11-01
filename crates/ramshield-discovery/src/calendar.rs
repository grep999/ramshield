//! Calendar learning and workload declarations.
//!
//! This module learns recurring time-of-week and time-of-month traffic shapes.
//! Operator-declared workloads are recorded as expected context, not as a
//! mechanism for suppressing or hiding observations. A declared workload is
//! always visible in reports and retains its provenance.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io, path::{Path, PathBuf}};

const WEEK_SLOTS: usize = 7 * 24;
const MONTH_SLOTS: usize = 31 * 24;
const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Recurrence {
    Daily,
    Weekly { weekday: u8 },
    MonthlyDay { day: u8 },
    MonthEnd,
    /// Explicit operator-declared recurring window, independent of calendar recurrence.
    EveryNDays { interval_days: u16, anchor_day: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkloadDeclaration {
    pub id: String,
    pub description: String,
    pub recurrence: Recurrence,
    pub start_hour: u8,
    pub duration_hours: u8,
    pub expected_services: Vec<String>,
    pub declared_by: String,
    pub declared_at_secs: u64,
    pub notes: Option<String>,
}

impl WorkloadDeclaration {
    pub fn validate(&self) -> io::Result<()> {
        if self.id.is_empty() || self.id.len() > 96 || !self.id.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "workload id must be 1..=96 ASCII alphanumeric/._- characters"));
        }
        if self.description.len() > 512 || self.declared_by.len() > 128 || self.expected_services.len() > 64 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "workload declaration exceeds field bounds"));
        }
        if self.start_hour > 23 || !(1..=24).contains(&self.duration_hours) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "workload start_hour must be 0..=23 and duration_hours 1..=24"));
        }
        match self.recurrence {
            Recurrence::Weekly { weekday } if weekday > 6 =>
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "weekday must be 0..=6 (Monday=0)")),
            Recurrence::MonthlyDay { day } if !(1..=31).contains(&day) =>
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "monthly day must be 1..=31")),
            Recurrence::EveryNDays { interval_days, .. } if interval_days == 0 =>
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "interval_days must be nonzero")),
            _ => {}
        }
        Ok(())
    }

    pub fn active_at(&self, timestamp_secs: u64) -> bool {
        let hour_of_day = ((timestamp_secs / 3600) % 24) as u8;
        let offset = (hour_of_day + 24 - self.start_hour) % 24;
        if offset >= self.duration_hours { return false; }
        let days = timestamp_secs / 86_400;
        match self.recurrence {
            Recurrence::Daily => true,
            Recurrence::Weekly { weekday } => ((days + 3) % 7) as u8 == weekday,
            Recurrence::MonthlyDay { day } => utc_day_of_month(timestamp_secs) == Some(day),
            Recurrence::MonthEnd => is_month_end(timestamp_secs),
            Recurrence::EveryNDays { interval_days, anchor_day } =>
                days >= anchor_day && (days - anchor_day) % u64::from(interval_days) == 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Slot {
    observations: u64,
    packets: u64,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CalendarState {
    schema: u32,
    week: Vec<Slot>,
    month: Vec<Slot>,
    declarations: BTreeMap<String, WorkloadDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSlotSummary {
    pub slot: usize,
    pub observations: u64,
    pub packets: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSnapshot {
    pub week: Vec<CalendarSlotSummary>,
    pub month: Vec<CalendarSlotSummary>,
    pub declarations: Vec<WorkloadDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedWorkload {
    pub id: String,
    pub description: String,
    pub active: bool,
    pub expected_services: Vec<String>,
    pub declared_by: String,
    pub notes: Option<String>,
}

pub struct TrafficCalendar {
    state: CalendarState,
    path: PathBuf,
}

impl TrafficCalendar {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let state = if path.exists() {
            let state: CalendarState = serde_json::from_slice(&fs::read(&path)?)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if state.schema != SCHEMA || state.week.len() != WEEK_SLOTS || state.month.len() != MONTH_SLOTS {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "traffic calendar schema mismatch"));
            }
            state
        } else {
            CalendarState { schema: SCHEMA, week: vec![Slot::default(); WEEK_SLOTS],
                month: vec![Slot::default(); MONTH_SLOTS], declarations: BTreeMap::new() }
        };
        Ok(Self { state, path })
    }

    /// Feed a known service, deployment, batch job or recurring workload before it runs.
    /// Replacing a declaration with the same ID updates its operator-provided context.
    pub fn declare_workload(&mut self, declaration: WorkloadDeclaration) -> io::Result<()> {
        declaration.validate()?;
        self.state.declarations.insert(declaration.id.clone(), declaration);
        Ok(())
    }

    pub fn remove_workload(&mut self, id: &str) -> bool {
        self.state.declarations.remove(id).is_some()
    }

    /// Aggregate an hourly traffic observation into recurring calendar slots.
    /// weekday is UTC Monday=0; timestamps must be Unix UTC seconds.
    pub fn observe_hour(&mut self, timestamp_secs: u64, packets: u64, bytes: u64) {
        let hour = timestamp_secs / 3600;
        let hour_of_day = (hour % 24) as usize;
        let weekday = ((timestamp_secs / 86_400 + 3) % 7) as usize;
        let week_slot = weekday * 24 + hour_of_day;
        add(&mut self.state.week[week_slot], packets, bytes);
        if let Some(day) = utc_day_of_month(timestamp_secs) {
            let month_slot = (usize::from(day) - 1) * 24 + hour_of_day;
            add(&mut self.state.month[month_slot], packets, bytes);
        }
    }

    /// Operator context matching the supplied time. Declarations never erase samples.
    pub fn expected_workloads(&self, timestamp_secs: u64) -> Vec<ExpectedWorkload> {
        self.state.declarations.values().map(|d| ExpectedWorkload {
            id: d.id.clone(), description: d.description.clone(),
            active: d.active_at(timestamp_secs), expected_services: d.expected_services.clone(),
            declared_by: d.declared_by.clone(), notes: d.notes.clone(),
        }).collect()
    }

    pub fn snapshot(&self) -> CalendarSnapshot {
        CalendarSnapshot {
            week: summaries(&self.state.week),
            month: summaries(&self.state.month),
            declarations: self.state.declarations.values().cloned().collect(),
        }
    }

    /// Persist declarations and learned calendar atomically.
    pub fn flush(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() { fs::create_dir_all(parent)?; }
        let bytes = serde_json::to_vec(&self.state)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut tmp = self.path.clone();
        tmp.set_extension("tmp");
        {
            let mut file = fs::OpenOptions::new().create(true).truncate(true).write(true).open(&tmp)?;
            use std::io::Write;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        fs::rename(&tmp, &self.path)?;
        if let Some(parent) = self.path.parent() { fs::File::open(parent)?.sync_all()?; }
        Ok(())
    }
}

fn add(slot: &mut Slot, packets: u64, bytes: u64) {
    slot.observations = slot.observations.saturating_add(1);
    slot.packets = slot.packets.saturating_add(packets);
    slot.bytes = slot.bytes.saturating_add(bytes);
}

fn summaries(slots: &[Slot]) -> Vec<CalendarSlotSummary> {
    slots.iter().enumerate().map(|(slot, value)| CalendarSlotSummary {
        slot, observations: value.observations, packets: value.packets, bytes: value.bytes,
    }).collect()
}

fn utc_day_of_month(timestamp_secs: u64) -> Option<u8> {
    let days = timestamp_secs / 86_400;
    let z = i64::try_from(days).ok()?.checked_add(719_468)?;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 { year += 1; }
    let _ = year;
    u8::try_from(day).ok()
}

fn is_month_end(timestamp_secs: u64) -> bool {
    let day = timestamp_secs / 86_400;
    let next = (day + 1).saturating_mul(86_400);
    match (utc_day_of_month(timestamp_secs), utc_day_of_month(next)) {
        (Some(current), Some(following)) => following < current,
        (Some(current), None) => current >= 28,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    fn path() -> PathBuf {
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("rs-calendar-{n}.json"))
    }
    #[test]
    fn weekly_and_month_end_workloads_are_declared_context() {
        let p = path();
        let mut c = TrafficCalendar::open(&p).unwrap();
        c.declare_workload(WorkloadDeclaration {
            id: "month-end-billing".into(), description: "Monthly billing run".into(),
            recurrence: Recurrence::MonthEnd, start_hour: 22, duration_hours: 3,
            expected_services: vec!["billing-worker".into()], declared_by: "operator".into(),
            declared_at_secs: 1_700_000_000, notes: None,
        }).unwrap();
        let feb_28_2024_23 = 1_709_164_800;
        assert!(c.expected_workloads(feb_28_2024_23).iter().any(|w| w.id == "month-end-billing" && w.active));
        c.observe_hour(feb_28_2024_23, 100, 10_000);
        assert!(c.snapshot().month.iter().any(|s| s.observations > 0));
        c.flush().unwrap();
        let reopened = TrafficCalendar::open(&p).unwrap();
        assert_eq!(reopened.snapshot().declarations.len(), 1);
        let _ = fs::remove_file(p);
    }
    #[test]
    fn weekly_schedule_uses_monday_zero() {
        let declaration = WorkloadDeclaration {
            id: "weekly-report".into(), description: "Weekly report".into(),
            recurrence: Recurrence::Weekly { weekday: 0 }, start_hour: 1, duration_hours: 2,
            expected_services: vec![], declared_by: "ops".into(), declared_at_secs: 0, notes: None,
        };
        assert!(declaration.active_at(4 * 86_400 + 3600));
        assert!(!declaration.active_at(5 * 86_400 + 3600));
    }
}
