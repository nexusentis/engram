use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc, Weekday};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Result of parsing a temporal expression
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub expression: String,
    pub confidence: f32,
}

/// Parser for temporal expressions in text
pub struct TemporalParser {
    // Compiled regex patterns
    yesterday_re: Regex,
    today_re: Regex,
    last_week_re: Regex,
    last_month_re: Regex,
    last_year_re: Regex,
    days_ago_re: Regex,
    weeks_ago_re: Regex,
    months_ago_re: Regex,
    month_year_re: Regex,
    iso_date_re: Regex,
    this_morning_re: Regex,
    last_night_re: Regex,
    last_weekday_re: Regex,
    last_weekend_re: Regex,
    couple_days_re: Regex,
    couple_weeks_re: Regex,
    couple_months_re: Regex,
    few_days_re: Regex,
    few_weeks_re: Regex,
    few_months_re: Regex,
    other_day_re: Regex,
}

impl TemporalParser {
    pub fn new() -> Self {
        Self {
            yesterday_re: Regex::new(r"(?i)\byesterday\b").unwrap(),
            today_re: Regex::new(r"(?i)\btoday\b").unwrap(),
            last_week_re: Regex::new(r"(?i)\blast\s+week\b").unwrap(),
            last_month_re: Regex::new(r"(?i)\blast\s+month\b").unwrap(),
            last_year_re: Regex::new(r"(?i)\blast\s+year\b").unwrap(),
            days_ago_re: Regex::new(r"(?i)(\d+)\s+days?\s+ago").unwrap(),
            weeks_ago_re: Regex::new(r"(?i)(\d+)\s+weeks?\s+ago").unwrap(),
            months_ago_re: Regex::new(r"(?i)(\d+)\s+months?\s+ago").unwrap(),
            month_year_re: Regex::new(r"(?i)\b(january|february|march|april|may|june|july|august|september|october|november|december)\s+(\d{4})\b").unwrap(),
            iso_date_re: Regex::new(r"\b(\d{4})-(\d{2})-(\d{2})\b").unwrap(),
            this_morning_re: Regex::new(r"(?i)\bthis\s+morning\b").unwrap(),
            last_night_re: Regex::new(r"(?i)\blast\s+night\b").unwrap(),
            last_weekday_re: Regex::new(r"(?i)\blast\s+(monday|tuesday|wednesday|thursday|friday|saturday|sunday)\b").unwrap(),
            last_weekend_re: Regex::new(r"(?i)\blast\s+weekend\b").unwrap(),
            couple_days_re: Regex::new(r"(?i)\ba\s+couple\s+(?:of\s+)?days?\s+ago\b").unwrap(),
            couple_weeks_re: Regex::new(r"(?i)\ba\s+couple\s+(?:of\s+)?weeks?\s+ago\b").unwrap(),
            couple_months_re: Regex::new(r"(?i)\ba\s+couple\s+(?:of\s+)?months?\s+ago\b").unwrap(),
            few_days_re: Regex::new(r"(?i)\ba\s+few\s+days?\s+ago\b").unwrap(),
            few_weeks_re: Regex::new(r"(?i)\ba\s+few\s+weeks?\s+ago\b").unwrap(),
            few_months_re: Regex::new(r"(?i)\ba\s+few\s+months?\s+ago\b").unwrap(),
            other_day_re: Regex::new(r"(?i)\bthe\s+other\s+day\b").unwrap(),
        }
    }

    /// Parse text and extract all temporal expressions
    pub fn parse(&self, text: &str, reference_time: DateTime<Utc>) -> Vec<TemporalRange> {
        let mut results = Vec::new();

        // Yesterday
        for mat in self.yesterday_re.find_iter(text) {
            let date = reference_time.date_naive() - Duration::days(1);
            results.push(TemporalRange {
                start: date.and_time(NaiveTime::MIN).and_utc(),
                end: date
                    .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
                    .and_utc(),
                expression: mat.as_str().to_string(),
                confidence: 0.95,
            });
        }

        // Today
        for mat in self.today_re.find_iter(text) {
            let date = reference_time.date_naive();
            results.push(TemporalRange {
                start: date.and_time(NaiveTime::MIN).and_utc(),
                end: date
                    .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
                    .and_utc(),
                expression: mat.as_str().to_string(),
                confidence: 0.95,
            });
        }

        // Last week
        for mat in self.last_week_re.find_iter(text) {
            let start = reference_time - Duration::weeks(1);
            results.push(TemporalRange {
                start,
                end: reference_time,
                expression: mat.as_str().to_string(),
                confidence: 0.9,
            });
        }

        // Last month
        for mat in self.last_month_re.find_iter(text) {
            let start = reference_time - Duration::days(30);
            results.push(TemporalRange {
                start,
                end: reference_time,
                expression: mat.as_str().to_string(),
                confidence: 0.9,
            });
        }

        // Last year
        for mat in self.last_year_re.find_iter(text) {
            let start = reference_time - Duration::days(365);
            results.push(TemporalRange {
                start,
                end: reference_time,
                expression: mat.as_str().to_string(),
                confidence: 0.85,
            });
        }

        // N days ago
        for caps in self.days_ago_re.captures_iter(text) {
            if let Some(days_match) = caps.get(1) {
                if let Ok(days) = days_match.as_str().parse::<i64>() {
                    let date = reference_time - Duration::days(days);
                    results.push(TemporalRange {
                        start: date,
                        end: reference_time,
                        expression: caps
                            .get(0)
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default(),
                        confidence: 0.9,
                    });
                }
            }
        }

        // N weeks ago
        for caps in self.weeks_ago_re.captures_iter(text) {
            if let Some(weeks_match) = caps.get(1) {
                if let Ok(weeks) = weeks_match.as_str().parse::<i64>() {
                    let date = reference_time - Duration::weeks(weeks);
                    results.push(TemporalRange {
                        start: date,
                        end: reference_time,
                        expression: caps
                            .get(0)
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default(),
                        confidence: 0.9,
                    });
                }
            }
        }

        // N months ago
        for caps in self.months_ago_re.captures_iter(text) {
            if let Some(months_match) = caps.get(1) {
                if let Ok(months) = months_match.as_str().parse::<i64>() {
                    let date = reference_time - Duration::days(months * 30);
                    results.push(TemporalRange {
                        start: date,
                        end: reference_time,
                        expression: caps
                            .get(0)
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default(),
                        confidence: 0.85,
                    });
                }
            }
        }

        // This morning
        for mat in self.this_morning_re.find_iter(text) {
            let date = reference_time.date_naive();
            results.push(TemporalRange {
                start: date
                    .and_time(NaiveTime::from_hms_opt(6, 0, 0).unwrap())
                    .and_utc(),
                end: date
                    .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap())
                    .and_utc(),
                expression: mat.as_str().to_string(),
                confidence: 0.9,
            });
        }

        // Last night
        for mat in self.last_night_re.find_iter(text) {
            let date = reference_time.date_naive() - Duration::days(1);
            results.push(TemporalRange {
                start: date
                    .and_time(NaiveTime::from_hms_opt(18, 0, 0).unwrap())
                    .and_utc(),
                end: date
                    .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
                    .and_utc(),
                expression: mat.as_str().to_string(),
                confidence: 0.9,
            });
        }

        // Last [weekday] (e.g., "last Saturday") — resolves to most recent occurrence
        for caps in self.last_weekday_re.captures_iter(text) {
            if let Some(day_match) = caps.get(1) {
                let target_weekday = match day_match.as_str().to_lowercase().as_str() {
                    "monday" => Weekday::Mon,
                    "tuesday" => Weekday::Tue,
                    "wednesday" => Weekday::Wed,
                    "thursday" => Weekday::Thu,
                    "friday" => Weekday::Fri,
                    "saturday" => Weekday::Sat,
                    "sunday" => Weekday::Sun,
                    _ => continue,
                };
                let ref_date = reference_time.date_naive();
                let ref_weekday = ref_date.weekday();
                // Days back: if today is Wed(2) and target is Sat(5), days_back = (2 - 5 + 7) % 7 = 4? No.
                // We want the most recent past occurrence. If same day, go back 7.
                let days_back = {
                    let diff = ref_weekday.num_days_from_monday() as i64
                        - target_weekday.num_days_from_monday() as i64;
                    if diff <= 0 {
                        diff + 7
                    } else {
                        diff
                    }
                };
                let date = ref_date - Duration::days(days_back);
                results.push(TemporalRange {
                    start: date.and_time(NaiveTime::MIN).and_utc(),
                    end: date
                        .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
                        .and_utc(),
                    expression: caps
                        .get(0)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default(),
                    confidence: 0.9,
                });
            }
        }

        // Last weekend (Saturday-Sunday of previous week)
        for mat in self.last_weekend_re.find_iter(text) {
            let ref_date = reference_time.date_naive();
            let ref_weekday = ref_date.weekday();
            // Find last Saturday
            let days_to_last_sat = {
                let diff = ref_weekday.num_days_from_monday() as i64
                    - Weekday::Sat.num_days_from_monday() as i64;
                if diff <= 0 {
                    diff + 7
                } else {
                    diff
                }
            };
            let last_sat = ref_date - Duration::days(days_to_last_sat);
            let last_sun = last_sat + Duration::days(1);
            results.push(TemporalRange {
                start: last_sat.and_time(NaiveTime::MIN).and_utc(),
                end: last_sun
                    .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
                    .and_utc(),
                expression: mat.as_str().to_string(),
                confidence: 0.9,
            });
        }

        // "a couple of days ago" (2 days)
        for mat in self.couple_days_re.find_iter(text) {
            let date = reference_time - Duration::days(2);
            results.push(TemporalRange {
                start: date,
                end: reference_time,
                expression: mat.as_str().to_string(),
                confidence: 0.85,
            });
        }

        // "a couple of weeks ago" (2 weeks)
        for mat in self.couple_weeks_re.find_iter(text) {
            let date = reference_time - Duration::weeks(2);
            results.push(TemporalRange {
                start: date,
                end: reference_time,
                expression: mat.as_str().to_string(),
                confidence: 0.85,
            });
        }

        // "a couple of months ago" (2 months)
        for mat in self.couple_months_re.find_iter(text) {
            let date = reference_time - Duration::days(60);
            results.push(TemporalRange {
                start: date,
                end: reference_time,
                expression: mat.as_str().to_string(),
                confidence: 0.8,
            });
        }

        // "a few days ago" (3 days)
        for mat in self.few_days_re.find_iter(text) {
            let date = reference_time - Duration::days(3);
            results.push(TemporalRange {
                start: date,
                end: reference_time,
                expression: mat.as_str().to_string(),
                confidence: 0.8,
            });
        }

        // "a few weeks ago" (3 weeks)
        for mat in self.few_weeks_re.find_iter(text) {
            let date = reference_time - Duration::weeks(3);
            results.push(TemporalRange {
                start: date,
                end: reference_time,
                expression: mat.as_str().to_string(),
                confidence: 0.8,
            });
        }

        // "a few months ago" (3 months)
        for mat in self.few_months_re.find_iter(text) {
            let date = reference_time - Duration::days(90);
            results.push(TemporalRange {
                start: date,
                end: reference_time,
                expression: mat.as_str().to_string(),
                confidence: 0.75,
            });
        }

        // "the other day" (~2 days ago)
        for mat in self.other_day_re.find_iter(text) {
            let date = reference_time - Duration::days(2);
            results.push(TemporalRange {
                start: date,
                end: reference_time,
                expression: mat.as_str().to_string(),
                confidence: 0.7,
            });
        }

        // Month + year (e.g., "January 2024")
        for caps in self.month_year_re.captures_iter(text) {
            if let (Some(month_match), Some(year_match)) = (caps.get(1), caps.get(2)) {
                let month = match month_match.as_str().to_lowercase().as_str() {
                    "january" => 1,
                    "february" => 2,
                    "march" => 3,
                    "april" => 4,
                    "may" => 5,
                    "june" => 6,
                    "july" => 7,
                    "august" => 8,
                    "september" => 9,
                    "october" => 10,
                    "november" => 11,
                    "december" => 12,
                    _ => continue,
                };

                if let Ok(year) = year_match.as_str().parse::<i32>() {
                    if let Some(start_date) = NaiveDate::from_ymd_opt(year, month, 1) {
                        // Get last day of month
                        let end_date = if month == 12 {
                            NaiveDate::from_ymd_opt(year + 1, 1, 1)
                        } else {
                            NaiveDate::from_ymd_opt(year, month + 1, 1)
                        }
                        .and_then(|d| d.pred_opt())
                        .unwrap_or(start_date);

                        results.push(TemporalRange {
                            start: start_date.and_time(NaiveTime::MIN).and_utc(),
                            end: end_date
                                .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
                                .and_utc(),
                            expression: caps
                                .get(0)
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_default(),
                            confidence: 0.95,
                        });
                    }
                }
            }
        }

        // ISO date (2024-01-15)
        for caps in self.iso_date_re.captures_iter(text) {
            if let (Some(year_match), Some(month_match), Some(day_match)) =
                (caps.get(1), caps.get(2), caps.get(3))
            {
                if let (Ok(year), Ok(month), Ok(day)) = (
                    year_match.as_str().parse::<i32>(),
                    month_match.as_str().parse::<u32>(),
                    day_match.as_str().parse::<u32>(),
                ) {
                    if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                        results.push(TemporalRange {
                            start: date.and_time(NaiveTime::MIN).and_utc(),
                            end: date
                                .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
                                .and_utc(),
                            expression: caps
                                .get(0)
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_default(),
                            confidence: 1.0,
                        });
                    }
                }
            }
        }

        results
    }

    /// Get the most likely t_valid for an extracted fact
    /// Returns the start time of the highest confidence temporal expression
    pub fn resolve_fact_time(
        &self,
        text: &str,
        reference_time: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        let ranges = self.parse(text, reference_time);
        ranges
            .into_iter()
            .max_by(|a, b| {
                a.confidence
                    .partial_cmp(&b.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.start)
    }

    /// Extract temporal markers as strings for storage
    pub fn extract_markers(&self, text: &str) -> Vec<String> {
        let mut markers = Vec::new();

        // Collect all matched expressions
        for mat in self.yesterday_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.today_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.last_week_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.last_month_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.last_year_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for caps in self.days_ago_re.captures_iter(text) {
            if let Some(m) = caps.get(0) {
                markers.push(m.as_str().to_string());
            }
        }
        for caps in self.weeks_ago_re.captures_iter(text) {
            if let Some(m) = caps.get(0) {
                markers.push(m.as_str().to_string());
            }
        }
        for caps in self.months_ago_re.captures_iter(text) {
            if let Some(m) = caps.get(0) {
                markers.push(m.as_str().to_string());
            }
        }
        for mat in self.this_morning_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.last_night_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for caps in self.last_weekday_re.captures_iter(text) {
            if let Some(m) = caps.get(0) {
                markers.push(m.as_str().to_string());
            }
        }
        for mat in self.last_weekend_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.couple_days_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.couple_weeks_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.couple_months_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.few_days_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.few_weeks_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.few_months_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for mat in self.other_day_re.find_iter(text) {
            markers.push(mat.as_str().to_string());
        }
        for caps in self.month_year_re.captures_iter(text) {
            if let Some(m) = caps.get(0) {
                markers.push(m.as_str().to_string());
            }
        }
        for caps in self.iso_date_re.captures_iter(text) {
            if let Some(m) = caps.get(0) {
                markers.push(m.as_str().to_string());
            }
        }
        // Deduplicate
        markers.sort();
        markers.dedup();
        markers
    }
}

impl Default for TemporalParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 6, 15, 12, 0, 0).unwrap()
    }

    #[test]
    fn test_temporal_parser_creation() {
        let parser = TemporalParser::new();
        // Just verify it can be created
        assert!(parser.yesterday_re.is_match("yesterday"));
    }

    #[test]
    fn test_yesterday() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("I met her yesterday", now);

        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].start < now);
        assert_eq!(ranges[0].expression, "yesterday");
        assert_eq!(ranges[0].confidence, 0.95);
    }

    #[test]
    fn test_today() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("I have a meeting today", now);

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start.date_naive(), now.date_naive());
    }

    #[test]
    fn test_last_week() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("I saw this last week", now);

        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].start < now);
        assert_eq!(ranges[0].confidence, 0.9);
    }

    #[test]
    fn test_last_month() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("It happened last month", now);

        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].start < now);
    }

    #[test]
    fn test_days_ago() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("I started 5 days ago", now);

        assert_eq!(ranges.len(), 1);
        let expected_start = now - Duration::days(5);
        assert_eq!(ranges[0].start.date_naive(), expected_start.date_naive());
    }

    #[test]
    fn test_weeks_ago() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("We met 2 weeks ago", now);

        assert_eq!(ranges.len(), 1);
        let expected_start = now - Duration::weeks(2);
        assert_eq!(ranges[0].start.date_naive(), expected_start.date_naive());
    }

    #[test]
    fn test_iso_date() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("The meeting is on 2024-06-15", now);

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].confidence, 1.0);
        assert_eq!(
            ranges[0].start.date_naive(),
            NaiveDate::from_ymd_opt(2024, 6, 15).unwrap()
        );
    }

    #[test]
    fn test_month_year() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("I started in January 2024", now);

        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].expression.contains("January"));
        assert_eq!(
            ranges[0].start.date_naive(),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
        );
        assert_eq!(
            ranges[0].end.date_naive(),
            NaiveDate::from_ymd_opt(2024, 1, 31).unwrap()
        );
    }

    #[test]
    fn test_december_month_year() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("It was December 2023", now);

        assert_eq!(ranges.len(), 1);
        assert_eq!(
            ranges[0].start.date_naive(),
            NaiveDate::from_ymd_opt(2023, 12, 1).unwrap()
        );
        assert_eq!(
            ranges[0].end.date_naive(),
            NaiveDate::from_ymd_opt(2023, 12, 31).unwrap()
        );
    }

    #[test]
    fn test_multiple_expressions() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("I started yesterday but the deadline is 2024-07-01", now);

        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn test_no_temporal_expressions() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("I like coffee and books", now);

        assert!(ranges.is_empty());
    }

    #[test]
    fn test_this_morning() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("I saw her this morning", now);

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].expression, "this morning");
    }

    #[test]
    fn test_last_night() {
        let parser = TemporalParser::new();
        let now = fixed_time();
        let ranges = parser.parse("The party was last night", now);

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].expression, "last night");
    }

    #[test]
    fn test_resolve_fact_time_highest_confidence() {
        let parser = TemporalParser::new();
        let now = fixed_time();

        // ISO date has confidence 1.0, yesterday has 0.95
        let time = parser.resolve_fact_time("I started yesterday on 2024-06-10", now);

        assert!(time.is_some());
        // Should pick the ISO date due to higher confidence
        assert_eq!(
            time.unwrap().date_naive(),
            NaiveDate::from_ymd_opt(2024, 6, 10).unwrap()
        );
    }

    #[test]
    fn test_resolve_fact_time_none() {
        let parser = TemporalParser::new();
        let now = fixed_time();

        let time = parser.resolve_fact_time("I like pizza", now);
        assert!(time.is_none());
    }

    #[test]
    fn test_extract_markers() {
        let parser = TemporalParser::new();
        let markers =
            parser.extract_markers("I met her yesterday and we have a meeting on 2024-07-01");

        assert_eq!(markers.len(), 2);
        assert!(markers.contains(&"yesterday".to_string()));
        assert!(markers.contains(&"2024-07-01".to_string()));
    }

    #[test]
    fn test_case_insensitive() {
        let parser = TemporalParser::new();
        let now = fixed_time();

        // Test various cases
        assert_eq!(parser.parse("YESTERDAY", now).len(), 1);
        assert_eq!(parser.parse("Yesterday", now).len(), 1);
        assert_eq!(parser.parse("LAST WEEK", now).len(), 1);
        assert_eq!(parser.parse("JANUARY 2024", now).len(), 1);
    }

    #[test]
    fn test_singular_plural_days() {
        let parser = TemporalParser::new();
        let now = fixed_time();

        assert_eq!(parser.parse("1 day ago", now).len(), 1);
        assert_eq!(parser.parse("2 days ago", now).len(), 1);
    }

    #[test]
    fn test_singular_plural_weeks() {
        let parser = TemporalParser::new();
        let now = fixed_time();

        assert_eq!(parser.parse("1 week ago", now).len(), 1);
        assert_eq!(parser.parse("3 weeks ago", now).len(), 1);
    }

}
