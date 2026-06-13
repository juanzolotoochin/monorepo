//! Pure data layer for the energy visualizer: CSV parsing, model, aggregation.
//! No gpui dependencies so it can be unit-tested in isolation.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Kwh,
    Cost,
}

impl Metric {
    pub fn label(self) -> &'static str {
        match self {
            Metric::Kwh => "kWh",
            Metric::Cost => "$",
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct HourSample {
    pub kwh: f64,
    pub cost: f64,
}

impl HourSample {
    pub fn value(&self, metric: Metric) -> f64 {
        match metric {
            Metric::Kwh => self.kwh,
            Metric::Cost => self.cost,
        }
    }
}

#[derive(Clone)]
pub struct DayData {
    pub date: String,
    pub hours: [HourSample; 24],
}

impl DayData {
    pub fn total(&self, metric: Metric) -> f64 {
        self.hours.iter().map(|h| h.value(metric)).sum()
    }
}

/// One hour's tooltip text: period header, consumption, price, and unit rate.
/// Rate shows "— / kWh" when `kwh` is 0 (rate undefined).
pub fn hour_tooltip(hour: usize, kwh: f64, cost: f64) -> String {
    let rate = if kwh > 0.0 {
        format!("${:.2} / kWh", cost / kwh)
    } else {
        "\u{2014} / kWh".to_string()
    };
    format!("{hour:02}:00\u{2013}{hour:02}:59\n{kwh:.2} kWh\n${cost:.2}\n{rate}")
}

pub struct Dataset {
    pub days: Vec<DayData>,
    pub max_hourly_kwh: f64,
    pub max_hourly_cost: f64,
}

impl Dataset {
    pub fn max_hourly(&self, metric: Metric) -> f64 {
        match metric {
            Metric::Kwh => self.max_hourly_kwh,
            Metric::Cost => self.max_hourly_cost,
        }
    }

    pub fn from_csv_str(s: &str) -> Result<Dataset, String> {
        let mut lines = s.lines();

        // Skip the metadata preamble up to and including the column header row.
        let found_header = lines.by_ref().any(|l| l.starts_with("TYPE,"));
        if !found_header {
            return Err("CSV header row (TYPE,DATE,...) not found".to_string());
        }

        // Group rows by date, preserving first-seen order, then sort.
        let mut order: Vec<String> = Vec::new();
        let mut by_date: std::collections::HashMap<String, DayData> = std::collections::HashMap::new();

        for line in lines {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 6 || f[0] != "Electric usage" {
                continue;
            }
            let date = f[1].to_string();
            let hour: usize = match f[2].get(0..2).and_then(|h| h.parse().ok()) {
                Some(h) if h < 24 => h,
                _ => continue,
            };
            let kwh: f64 = match f[4].parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let cost: f64 = f[5].trim_start_matches('$').parse().unwrap_or(0.0);

            let day = by_date.entry(date.clone()).or_insert_with(|| {
                order.push(date.clone());
                DayData { date: date.clone(), hours: [HourSample::default(); 24] }
            });
            day.hours[hour] = HourSample { kwh, cost };
        }

        if order.is_empty() {
            return Err("no electricity usage rows found".to_string());
        }

        let mut days: Vec<DayData> = order.into_iter().map(|d| by_date.remove(&d).unwrap()).collect();
        days.sort_by(|a, b| a.date.cmp(&b.date));

        let max_hourly_kwh = days
            .iter()
            .flat_map(|d| d.hours.iter())
            .map(|h| h.kwh)
            .fold(0.0_f64, f64::max);
        let max_hourly_cost = days
            .iter()
            .flat_map(|d| d.hours.iter())
            .map(|h| h.cost)
            .fold(0.0_f64, f64::max);

        Ok(Dataset { days, max_hourly_kwh, max_hourly_cost })
    }

    pub fn from_path(path: &str) -> Result<Dataset, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {path}: {e}"))?;
        Self::from_csv_str(&contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
Name,JANE DOE
Address,\"123 EXAMPLE ST, ANYTOWN CA 90000\"
Account Number,1234567890
Service,Service 1

TYPE,DATE,START TIME,END TIME,USAGE (kWh),COST,NOTES
Electric usage,2026-01-02,00:00,00:59,0.50,$0.20
Electric usage,2026-01-02,23:00,23:59,2.00,$0.80
Electric usage,2026-01-01,00:00,00:59,1.00,$0.40
Electric usage,2026-01-01,14:00,14:59,3.00,$1.20
";

    #[test]
    fn parses_and_sorts_days_chronologically() {
        let ds = Dataset::from_csv_str(SAMPLE).unwrap();
        assert_eq!(ds.days.len(), 2);
        assert_eq!(ds.days[0].date, "2026-01-01");
        assert_eq!(ds.days[1].date, "2026-01-02");
    }

    #[test]
    fn places_samples_in_correct_hour_and_strips_dollar() {
        let ds = Dataset::from_csv_str(SAMPLE).unwrap();
        let day0 = &ds.days[0];
        assert!((day0.hours[0].kwh - 1.00).abs() < 1e-9);
        assert!((day0.hours[0].cost - 0.40).abs() < 1e-9);
        assert!((day0.hours[14].kwh - 3.00).abs() < 1e-9);
        assert!((day0.hours[14].cost - 1.20).abs() < 1e-9);
    }

    #[test]
    fn missing_hours_default_to_zero() {
        let ds = Dataset::from_csv_str(SAMPLE).unwrap();
        let day0 = &ds.days[0];
        assert_eq!(day0.hours[1].kwh, 0.0);
        assert_eq!(day0.hours[1].cost, 0.0);
    }

    #[test]
    fn computes_fixed_maxima_across_all_samples() {
        let ds = Dataset::from_csv_str(SAMPLE).unwrap();
        assert!((ds.max_hourly_kwh - 3.00).abs() < 1e-9);
        assert!((ds.max_hourly_cost - 1.20).abs() < 1e-9);
    }

    #[test]
    fn day_total_sums_hours_for_metric() {
        let ds = Dataset::from_csv_str(SAMPLE).unwrap();
        assert!((ds.days[1].total(Metric::Kwh) - 2.50).abs() < 1e-9);
        assert!((ds.days[1].total(Metric::Cost) - 1.00).abs() < 1e-9);
    }

    #[test]
    fn errors_when_no_data_rows() {
        let err = Dataset::from_csv_str("Name,Nobody\nTYPE,DATE,START TIME,END TIME,USAGE (kWh),COST,NOTES\n");
        assert!(err.is_err());
    }

    #[test]
    fn from_path_missing_file_errors() {
        let err = Dataset::from_path("/no/such/energy_file_zzz.csv");
        assert!(err.is_err());
    }

    #[test]
    fn hour_tooltip_shows_metrics_and_rate() {
        let t = hour_tooltip(14, 3.0, 1.20);
        assert!(t.contains("14:00"));
        assert!(t.contains("3.00 kWh"));
        assert!(t.contains("$1.20"));
        assert!(t.contains("$0.40 / kWh"));
    }

    #[test]
    fn hour_tooltip_zero_kwh_uses_dash_rate() {
        let t = hour_tooltip(2, 0.0, 0.0);
        assert!(t.contains("0.00 kWh"));
        assert!(t.contains("\u{2014} / kWh")); // "— / kWh"
        assert!(!t.contains("inf") && !t.contains("NaN"));
    }
}
