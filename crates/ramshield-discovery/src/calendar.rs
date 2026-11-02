//! Calendar learning and workload declarations.
//!
//! This module learns recurring time-of-week and time-of-month traffic shapes.
//! Operator-declared workloads are recorded as expected context, not as a
//! mechanism for suppressing or hiding observations. A declared workload is
//! always visible in reports and retains its provenance.

use crate::{CountModelFit, CountMoments, fit_count_model};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

const WEEK_SLOTS: usize = 7 * 24;
const MONTH_SLOTS: usize = 31 * 24;
const SCHEMA: u32 = 3;
const MAX_DECLARATIONS: usize = 512;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Recurrence {
    Daily,
    Weekly {
        weekday: u8,
    },
    MonthlyDay {
        day: u8,
    },
    MonthEnd,
    /// Explicit operator-declared recurring window, independent of calendar recurrence.
    EveryNDays {
        interval_days: u16,
        anchor_day: u64,
    },
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
        if self.id.is_empty()
            || self.id.len() > 96
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workload id must be 1..=96 ASCII alphanumeric/._- characters",
            ));
        }
        if self.description.len() > 512
            || self.declared_by.len() > 128
            || self.expected_services.len() > 64
            || self
                .expected_services
                .iter()
                .any(|service| service.is_empty() || service.len() > 128)
            || self.notes.as_ref().is_some_and(|notes| notes.len() > 1_024)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workload declaration exceeds field bounds",
            ));
        }
        if self.start_hour > 23 || !(1..=24).contains(&self.duration_hours) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workload start_hour must be 0..=23 and duration_hours 1..=24",
            ));
        }
        match self.recurrence {
            Recurrence::Weekly { weekday } if weekday > 6 => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "weekday must be 0..=6 (Monday=0)",
                ));
            }
            Recurrence::MonthlyDay { day } if !(1..=31).contains(&day) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "monthly day must be 1..=31",
                ));
            }
            Recurrence::EveryNDays {
                interval_days: 0, ..
            } => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "interval_days must be nonzero",
                ));
            }
            _ => {}
        }
        Ok(())
    }

    pub fn active_at(&self, timestamp_secs: u64) -> bool {
        let hour_of_day = ((timestamp_secs / 3600) % 24) as u8;
        let offset = (hour_of_day + 24 - self.start_hour) % 24;
        if offset >= self.duration_hours {
            return false;
        }
        // Evaluate recurrence against the scheduled start day, not the
        // current day, so windows crossing midnight remain active.
        let start_secs = timestamp_secs.saturating_sub(u64::from(offset) * 3600);
        let days = start_secs / 86_400;
        match self.recurrence {
            Recurrence::Daily => true,
            Recurrence::Weekly { weekday } => ((days + 3) % 7) as u8 == weekday,
            Recurrence::MonthlyDay { day } => utc_day_of_month(start_secs) == Some(day),
            Recurrence::MonthEnd => is_month_end(start_secs),
            Recurrence::EveryNDays {
                interval_days,
                anchor_day,
            } => days >= anchor_day && (days - anchor_day).is_multiple_of(u64::from(interval_days)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Slot {
    observations: u64,
    packets: u64,
    moments: CountMoments,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CalendarState {
    schema: u32,
    week: Vec<Slot>,
    month: Vec<Slot>,
    month_end: Vec<Slot>,
    declarations: BTreeMap<String, WorkloadDeclaration>,
    executions: BTreeMap<String, WorkloadExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSlotSummary {
    pub slot: usize,
    pub observations: u64,
    pub packets: u64,
    pub mean_packets: Option<f64>,
    pub sample_variance_packets: Option<f64>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSnapshot {
    pub week: Vec<CalendarSlotSummary>,
    pub month: Vec<CalendarSlotSummary>,
    pub month_end: Vec<CalendarSlotSummary>,
    pub declarations: Vec<WorkloadDeclaration>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CalendarAssessmentKind {
    InsufficientData,
    WithinDescriptiveBand,
    AboveDescriptiveBand,
    BelowDescriptiveBand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarAssessment {
    pub kind: CalendarAssessmentKind,
    pub baseline_source: Option<String>,
    pub observations: u64,
    pub mean_packets: Option<f64>,
    pub standard_deviation_packets: Option<f64>,
    /// Candidate count family only; not a calibrated tail probability.
    pub count_model: Option<CountModelFit>,
    pub observed_packets: u64,
    pub lower_descriptive_packets: Option<f64>,
    pub upper_descriptive_packets: Option<f64>,
    pub active_workloads: Vec<ExpectedWorkload>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadExecution {
    pub id: String,
    pub description: String,
    pub state: ExecutionState,
    pub start_time_secs: u64,
    pub expected_duration_hours: u64,
    /// Whole elapsed hours from the actual start, rounded down.
    pub actual_duration_hours: Option<u64>,
    pub declared_by: String,
    pub notes: Option<String>,
}

impl WorkloadExecution {
    fn expected_end_secs(&self) -> Option<u64> {
        self.expected_duration_hours
            .checked_mul(3600)?
            .checked_add(self.start_time_secs)
    }

    fn validate(&self) -> io::Result<()> {
        if self.id.is_empty()
            || self.id.len() > 96
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "execution id must be 1..=96 ASCII alphanumeric/._- characters",
            ));
        }
        if self.description.len() > 512
            || self.declared_by.len() > 128
            || self.notes.as_ref().is_some_and(|notes| notes.len() > 1_024)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "execution metadata exceeds field bounds",
            ));
        }
        if self.state == ExecutionState::Declared
            || self.start_time_secs == 0
            || self.expected_duration_hours == 0
            || self.expected_end_secs().is_none()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "execution state, start time, or expected end time is invalid",
            ));
        }
        if self.actual_duration_hours.is_some()
            && !matches!(
                self.state,
                ExecutionState::Overrun | ExecutionState::Completed
            )
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "actual duration is only valid for overrun or completed executions",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ExecutionState {
    #[default]
    Declared,
    Scheduled,
    Active,
    Overrun,
    Completed,
    Cancelled,
    Missed,
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
            if state.schema != SCHEMA
                || state.week.len() != WEEK_SLOTS
                || state.month.len() != MONTH_SLOTS
                || state.month_end.len() != 24
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "traffic calendar schema mismatch",
                ));
            }
            let slots_valid = state
                .week
                .iter()
                .chain(&state.month)
                .chain(&state.month_end)
                .all(|slot| {
                    slot.observations == slot.moments.observations() && slot.moments.is_valid()
                });
            let declarations_valid = state
                .declarations
                .iter()
                .all(|(id, declaration)| id == &declaration.id && declaration.validate().is_ok());
            let executions_valid = state.executions.len() <= MAX_DECLARATIONS
                && state
                    .executions
                    .iter()
                    .all(|(id, execution)| id == &execution.id && execution.validate().is_ok());
            if !slots_valid
                || !declarations_valid
                || !executions_valid
                || state.declarations.len() > MAX_DECLARATIONS
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "traffic calendar contains invalid persisted state",
                ));
            }
            state
        } else {
            CalendarState {
                schema: SCHEMA,
                week: vec![Slot::default(); WEEK_SLOTS],
                month: vec![Slot::default(); MONTH_SLOTS],
                month_end: vec![Slot::default(); 24],
                declarations: BTreeMap::new(),
                executions: BTreeMap::new(),
            }
        };
        Ok(Self { state, path })
    }

    /// Feed a known service, deployment, batch job or recurring workload before it runs.
    /// Replacing a declaration with the same ID updates its operator-provided context.
    pub fn declare_workload(&mut self, declaration: WorkloadDeclaration) -> io::Result<()> {
        declaration.validate()?;
        if !self.state.declarations.contains_key(&declaration.id)
            && self.state.declarations.len() >= MAX_DECLARATIONS
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workload declaration limit reached",
            ));
        }
        self.state
            .declarations
            .insert(declaration.id.clone(), declaration);
        Ok(())
    }

    pub fn remove_workload(&mut self, id: &str) -> bool {
        self.state.declarations.remove(id).is_some()
    }

    /// Register a scheduled workload execution, replacing the same ID if present.
    pub fn declare_execution(&mut self, execution: WorkloadExecution) -> io::Result<()> {
        if execution.state != ExecutionState::Scheduled {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "execution must start in Scheduled state",
            ));
        }
        execution.validate()?;
        if !self.state.executions.contains_key(&execution.id)
            && self.state.executions.len() >= MAX_DECLARATIONS
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "execution limit reached",
            ));
        }
        self.state
            .executions
            .insert(execution.id.clone(), execution);
        Ok(())
    }

    /// Transition an execution to a new state using the current UTC clock.
    pub fn transition_execution(
        &mut self,
        id: &str,
        new_state: ExecutionState,
    ) -> Result<(), io::Error> {
        let now_secs = match new_state {
            ExecutionState::Active | ExecutionState::Overrun | ExecutionState::Completed => {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(io::Error::other)?
                    .as_secs()
            }
            _ => 0,
        };
        self.transition_execution_at(id, new_state, now_secs)
    }

    fn transition_execution_at(
        &mut self,
        id: &str,
        new_state: ExecutionState,
        now_secs: u64,
    ) -> Result<(), io::Error> {
        match new_state {
            ExecutionState::Active => {
                let Some(mut execution) = self.state.executions.get(id).cloned() else {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "execution not found",
                    ));
                };
                if execution.state != ExecutionState::Scheduled || now_secs == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid transition or execution start time",
                    ));
                }
                execution.state = ExecutionState::Active;
                execution.start_time_secs = now_secs;
                execution.actual_duration_hours = None;
                execution.validate()?;
                self.state.executions.insert(id.to_string(), execution);
                Ok(())
            }
            ExecutionState::Overrun => {
                let Some(mut execution) = self.state.executions.get(id).cloned() else {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "execution not found",
                    ));
                };
                if execution.state != ExecutionState::Active {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid transition",
                    ));
                }
                let expected_end = execution.expected_end_secs().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "execution expected end time overflows",
                    )
                })?;
                if now_secs <= expected_end {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "execution has not exceeded its expected duration",
                    ));
                }
                let elapsed = now_secs
                    .checked_sub(execution.start_time_secs)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "clock precedes execution start",
                        )
                    })?;
                execution.state = ExecutionState::Overrun;
                execution.actual_duration_hours = Some(elapsed / 3600);
                self.state.executions.insert(id.to_string(), execution);
                Ok(())
            }
            ExecutionState::Completed => {
                let Some(mut execution) = self.state.executions.get(id).cloned() else {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "execution not found",
                    ));
                };
                if !matches!(
                    execution.state,
                    ExecutionState::Active | ExecutionState::Overrun
                ) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid transition",
                    ));
                }
                let elapsed = now_secs
                    .checked_sub(execution.start_time_secs)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "clock precedes execution start",
                        )
                    })?;
                execution.state = ExecutionState::Completed;
                execution.actual_duration_hours = Some(elapsed / 3600);
                self.state.executions.insert(id.to_string(), execution);
                Ok(())
            }
            ExecutionState::Cancelled => {
                let Some(mut execution) = self.state.executions.get(id).cloned() else {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "execution not found",
                    ));
                };
                if execution.state == ExecutionState::Completed {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid transition",
                    ));
                }
                execution.state = ExecutionState::Cancelled;
                execution.actual_duration_hours = None;
                self.state.executions.insert(id.to_string(), execution);
                Ok(())
            }
            ExecutionState::Missed => {
                let Some(mut execution) = self.state.executions.get(id).cloned() else {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "execution not found",
                    ));
                };
                if execution.state != ExecutionState::Scheduled {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid transition",
                    ));
                }
                execution.state = ExecutionState::Missed;
                self.state.executions.insert(id.to_string(), execution);
                Ok(())
            }
            ExecutionState::Scheduled | ExecutionState::Declared => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid transition",
            )),
        }
    }

    /// Get the current execution state for a given execution ID.
    pub fn execution_state(&self, id: &str) -> Option<ExecutionState> {
        self.state.executions.get(id).map(|e| e.state)
    }

    /// Get the full execution record for a given ID.
    pub fn execution(&self, id: &str) -> Option<&WorkloadExecution> {
        self.state.executions.get(id)
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
        if is_month_end(timestamp_secs) {
            add(&mut self.state.month_end[hour_of_day], packets, bytes);
        }
    }

    /// Operator context matching the supplied time. Declarations never erase samples.
    pub fn expected_workloads(&self, timestamp_secs: u64) -> Vec<ExpectedWorkload> {
        self.state
            .declarations
            .values()
            .map(|d| ExpectedWorkload {
                id: d.id.clone(),
                description: d.description.clone(),
                active: d.active_at(timestamp_secs),
                expected_services: d.expected_services.clone(),
                declared_by: d.declared_by.clone(),
                notes: d.notes.clone(),
            })
            .collect()
    }

    /// Compare a new hourly count with recurring history. Monthly day-of-month
    /// history takes precedence when it has enough samples (useful for month-end
    /// jobs); otherwise the hour-of-week profile is used. The current mean +/-
    /// sigma band is descriptive only, not a calibrated prediction interval.
    /// The count-family fit is diagnostic and does not establish goodness-of-fit.
    pub fn assess(
        &self,
        timestamp_secs: u64,
        observed_packets: u64,
        min_observations: u64,
        sigma: f64,
    ) -> CalendarAssessment {
        let min_observations = min_observations.max(2);
        let hour = (timestamp_secs / 3600 % 24) as usize;
        let weekday = ((timestamp_secs / 86_400 + 3) % 7) as usize;
        let weekly = &self.state.week[weekday * 24 + hour];
        let monthly = utc_day_of_month(timestamp_secs)
            .map(|day| &self.state.month[(usize::from(day) - 1) * 24 + hour]);
        let month_end = &self.state.month_end[hour];
        let selected = (is_month_end(timestamp_secs) && month_end.observations >= min_observations)
            .then_some(("month-end", month_end))
            .or_else(|| {
                monthly
                    .filter(|slot| slot.observations >= min_observations)
                    .map(|slot| ("hour-of-month", slot))
            })
            .or_else(|| {
                (weekly.observations >= min_observations).then_some(("hour-of-week", weekly))
            });
        let active_workloads = self.expected_workloads(timestamp_secs);
        let Some((source, slot)) = selected else {
            return CalendarAssessment {
                kind: CalendarAssessmentKind::InsufficientData,
                baseline_source: None,
                observations: monthly.map_or(weekly.observations, |m| m.observations),
                mean_packets: None,
                standard_deviation_packets: None,
                count_model: None,
                observed_packets,
                lower_descriptive_packets: None,
                upper_descriptive_packets: None,
                active_workloads,
            };
        };
        let mean = slot.moments.mean();
        let variance = slot.moments.sample_variance().unwrap_or(0.0);
        let stddev = variance.sqrt();
        let count_model = fit_count_model(slot.observations, mean, variance);
        let spread = if sigma.is_finite() {
            sigma.max(0.0) * stddev
        } else {
            0.0
        };
        let lower = (mean - spread).max(0.0);
        let upper = mean + spread;
        let observed = observed_packets as f64;
        let kind = if observed > upper {
            CalendarAssessmentKind::AboveDescriptiveBand
        } else if observed < lower {
            CalendarAssessmentKind::BelowDescriptiveBand
        } else {
            CalendarAssessmentKind::WithinDescriptiveBand
        };
        CalendarAssessment {
            kind,
            baseline_source: Some(source.to_string()),
            observations: slot.observations,
            mean_packets: Some(mean),
            standard_deviation_packets: Some(stddev),
            count_model: Some(count_model),
            observed_packets,
            lower_descriptive_packets: Some(lower),
            upper_descriptive_packets: Some(upper),
            active_workloads,
        }
    }

    pub fn snapshot(&self) -> CalendarSnapshot {
        CalendarSnapshot {
            week: summaries(&self.state.week),
            month: summaries(&self.state.month),
            month_end: summaries(&self.state.month_end),
            declarations: self.state.declarations.values().cloned().collect(),
        }
    }

    /// Persist declarations and learned calendar atomically.
    pub fn flush(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec(&self.state)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut tmp = self.path.clone();
        tmp.set_extension("tmp");
        {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&tmp)?;
            use std::io::Write;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        fs::rename(&tmp, &self.path)?;
        if let Some(parent) = self.path.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}

fn add(slot: &mut Slot, packets: u64, bytes: u64) {
    if slot.observations == u64::MAX {
        return;
    }
    slot.observations = slot.observations.saturating_add(1);
    slot.packets = slot.packets.saturating_add(packets);
    slot.bytes = slot.bytes.saturating_add(bytes);
    slot.moments.push(packets);
}

fn summaries(slots: &[Slot]) -> Vec<CalendarSlotSummary> {
    slots
        .iter()
        .enumerate()
        .map(|(slot, value)| CalendarSlotSummary {
            slot,
            observations: value.observations,
            packets: value.packets,
            mean_packets: (value.observations > 0).then(|| value.moments.mean()),
            sample_variance_packets: value.moments.sample_variance(),
            bytes: value.bytes,
        })
        .collect()
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
    if month <= 2 {
        year += 1;
    }
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
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rs-calendar-{n}.json"))
    }

    fn execution(
        id: &str,
        start_time_secs: u64,
        expected_duration_hours: u64,
    ) -> WorkloadExecution {
        WorkloadExecution {
            id: id.into(),
            description: "Scheduled workload".into(),
            state: ExecutionState::Scheduled,
            start_time_secs,
            expected_duration_hours,
            actual_duration_hours: None,
            declared_by: "operator".into(),
            notes: None,
        }
    }
    #[test]
    fn weekly_and_month_end_workloads_are_declared_context() {
        let p = path();
        let mut c = TrafficCalendar::open(&p).unwrap();
        c.declare_workload(WorkloadDeclaration {
            id: "month-end-billing".into(),
            description: "Monthly billing run".into(),
            recurrence: Recurrence::MonthEnd,
            start_hour: 22,
            duration_hours: 3,
            expected_services: vec!["billing-worker".into()],
            declared_by: "operator".into(),
            declared_at_secs: 1_700_000_000,
            notes: None,
        })
        .unwrap();
        let feb_29_2024_23 = 1_709_247_600;
        assert!(
            c.expected_workloads(feb_29_2024_23)
                .iter()
                .any(|w| w.id == "month-end-billing" && w.active)
        );
        c.observe_hour(feb_29_2024_23, 100, 10_000);
        assert!(c.snapshot().month.iter().any(|s| s.observations > 0));
        c.flush().unwrap();
        let reopened = TrafficCalendar::open(&p).unwrap();
        assert_eq!(reopened.snapshot().declarations.len(), 1);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn weekly_schedule_uses_monday_zero() {
        let declaration = WorkloadDeclaration {
            id: "weekly-report".into(),
            description: "Weekly report".into(),
            recurrence: Recurrence::Weekly { weekday: 0 },
            start_hour: 1,
            duration_hours: 2,
            expected_services: vec![],
            declared_by: "ops".into(),
            declared_at_secs: 0,
            notes: None,
        };
        assert!(declaration.active_at(4 * 86_400 + 3600));
        assert!(!declaration.active_at(5 * 86_400 + 3600));
    }
    #[test]
    fn repeated_weekly_pattern_flags_outlier_but_reports_workload_context() {
        let p = path();
        let mut c = TrafficCalendar::open(&p).unwrap();
        let monday_midnight = 1_704_067_200u64;
        for week in 0..4 {
            c.observe_hour(monday_midnight + week * 7 * 86_400, 100, 10_000);
        }
        c.declare_workload(WorkloadDeclaration {
            id: "monthly-billing".into(),
            description: "Billing export".into(),
            recurrence: Recurrence::MonthEnd,
            start_hour: 0,
            duration_hours: 2,
            expected_services: vec!["billing".into()],
            declared_by: "ops".into(),
            declared_at_secs: 0,
            notes: Some("Expected monthly batch".into()),
        })
        .unwrap();
        let assessment = c.assess(monday_midnight + 4 * 7 * 86_400, 1_000, 4, 3.0);
        assert_eq!(
            assessment.kind,
            CalendarAssessmentKind::AboveDescriptiveBand
        );
        assert_eq!(assessment.baseline_source.as_deref(), Some("hour-of-week"));
        assert_eq!(assessment.observations, 4);
        assert!(assessment.upper_descriptive_packets.unwrap() < 1_000.0);
        assert!(assessment.count_model.is_some());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn calendar_assessment_refuses_to_score_without_history() {
        let p = path();
        let c = TrafficCalendar::open(&p).unwrap();
        let assessment = c.assess(1_700_000_000, 50_000, 4, 3.0);
        assert_eq!(assessment.kind, CalendarAssessmentKind::InsufficientData);
        assert!(assessment.mean_packets.is_none());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn one_observation_is_never_treated_as_a_baseline() {
        let p = path();
        let mut c = TrafficCalendar::open(&p).unwrap();
        let t = 1_704_067_200u64;
        c.observe_hour(t, 100, 10_000);
        let assessment = c.assess(t + 4 * 7 * 86_400, 1_000, 1, 3.0);
        assert_eq!(assessment.kind, CalendarAssessmentKind::InsufficientData);
        let _ = fs::remove_file(p);
    }
    #[test]
    fn execution_validation_rejects_overflowing_expected_end() {
        let p = path();
        let mut calendar = TrafficCalendar::open(&p).unwrap();
        assert!(
            calendar
                .declare_execution(execution("overflow", u64::MAX - 100, 1))
                .is_err()
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn activation_rejects_expected_end_overflow_after_actual_start() {
        let p = path();
        let mut calendar = TrafficCalendar::open(&p).unwrap();
        calendar
            .declare_execution(execution("long-job", 1, u64::MAX / 3600))
            .unwrap();
        assert!(
            calendar
                .transition_execution_at("long-job", ExecutionState::Active, 1_000)
                .is_err()
        );
        assert_eq!(
            calendar.execution_state("long-job"),
            Some(ExecutionState::Scheduled)
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn execution_transitions_are_checked_and_record_total_elapsed_hours() {
        let p = path();
        let mut calendar = TrafficCalendar::open(&p).unwrap();
        calendar
            .declare_execution(execution("billing", 1_000, 1))
            .unwrap();
        calendar
            .transition_execution_at("billing", ExecutionState::Active, 2_000)
            .unwrap();

        assert!(
            calendar
                .transition_execution_at("billing", ExecutionState::Overrun, 5_599)
                .is_err()
        );
        assert_eq!(
            calendar.execution_state("billing"),
            Some(ExecutionState::Active)
        );

        calendar
            .transition_execution_at("billing", ExecutionState::Overrun, 9_200)
            .unwrap();
        assert_eq!(
            calendar.execution("billing").unwrap().actual_duration_hours,
            Some(2)
        );
        calendar
            .transition_execution_at("billing", ExecutionState::Completed, 11_000)
            .unwrap();
        assert_eq!(
            calendar.execution_state("billing"),
            Some(ExecutionState::Completed)
        );
        assert_eq!(
            calendar.execution("billing").unwrap().actual_duration_hours,
            Some(2)
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn completed_execution_records_elapsed_time_before_expected_end() {
        let p = path();
        let mut calendar = TrafficCalendar::open(&p).unwrap();
        calendar
            .declare_execution(execution("batch", 1_000, 3))
            .unwrap();
        calendar
            .transition_execution_at("batch", ExecutionState::Active, 2_000)
            .unwrap();
        calendar
            .transition_execution_at("batch", ExecutionState::Completed, 9_200)
            .unwrap();
        assert_eq!(
            calendar.execution("batch").unwrap().actual_duration_hours,
            Some(2)
        );
        let _ = fs::remove_file(p);
    }

    #[test]
    fn persisted_execution_validation_rejects_overflowing_schedule() {
        let p = path();
        let mut calendar = TrafficCalendar::open(&p).unwrap();
        calendar
            .state
            .executions
            .insert("overflow".into(), execution("overflow", u64::MAX - 100, 1));
        calendar.flush().unwrap();
        assert!(TrafficCalendar::open(&p).is_err());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn persisted_execution_validation_enforces_capacity() {
        let p = path();
        let mut calendar = TrafficCalendar::open(&p).unwrap();
        for index in 0..=MAX_DECLARATIONS {
            let id = format!("exec-{index}");
            calendar
                .state
                .executions
                .insert(id.clone(), execution(&id, 1, 1));
        }
        calendar.flush().unwrap();
        assert!(TrafficCalendar::open(&p).is_err());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn execution_at_capacity_can_replace_existing_id() {
        let p = path();
        let mut calendar = TrafficCalendar::open(&p).unwrap();
        for index in 0..MAX_DECLARATIONS {
            let id = format!("exec-{index}");
            calendar.declare_execution(execution(&id, 1, 1)).unwrap();
        }
        let mut replacement = execution("exec-0", 2, 1);
        replacement.description = "Updated schedule".into();
        calendar.declare_execution(replacement).unwrap();
        assert_eq!(calendar.state.executions.len(), MAX_DECLARATIONS);
        assert_eq!(
            calendar.execution("exec-0").unwrap().description,
            "Updated schedule"
        );
        let _ = fs::remove_file(p);
    }
}
