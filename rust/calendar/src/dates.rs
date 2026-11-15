//! As contas de datas do calendário, sem GTK e sem dependências: a grade de 6 semanas começando no
//! domingo, como o `calendar.Calendar(firstweekday=6)` do Python com as semanas completadas.

/// Uma data do calendário gregoriano.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    pub fn new(year: i32, month: u32, day: u32) -> Date {
        Date { year, month, day }
    }

    /// Dias desde 1970-01-01 (o algoritmo `days_from_civil`, de Howard Hinnant).
    pub fn days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(days: i64) -> Date {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Date { year, month, day }
    }

    /// O dia da semana com o domingo em 0, a coluna da grade.
    pub fn weekday_from_sunday(self) -> usize {
        (self.days() + 4).rem_euclid(7) as usize // 1970-01-01 foi uma quinta
    }

    /// O dia da semana com a segunda em 0, como o `date.weekday()` do Python.
    pub fn weekday_from_monday(self) -> usize {
        (self.weekday_from_sunday() + 6) % 7
    }

    /// No formato ISO, como o `date.isoformat()` do Python.
    pub fn parse_iso(text: &str) -> Option<Date> {
        let mut parts = text.splitn(3, '-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        (1..=12).contains(&month).then(|| Date::new(year, month, day))
    }
}

/// O mês `n` meses depois (ou antes) de (year, month).
pub fn shift_month(year: i32, month: u32, n: i32) -> (i32, u32) {
    let index = year * 12 + month as i32 - 1 + n;
    (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
}

/// As 6 semanas da grade: começa no domingo que vem antes (ou no próprio) do dia 1.
pub fn month_grid(year: i32, month: u32) -> Vec<[Date; 7]> {
    let first = Date::new(year, month, 1);
    let start = first.days() - first.weekday_from_sunday() as i64;
    (0..6)
        .map(|week| {
            let mut days = [first; 7];
            for (column, day) in days.iter_mut().enumerate() {
                *day = Date::from_days(start + week * 7 + column as i64);
            }
            days
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_round_trip_and_weekdays() {
        for (date, days) in [(Date::new(1970, 1, 1), 0), (Date::new(2000, 2, 29), 11_016), (Date::new(2026, 11, 12), 20_769)] {
            assert_eq!(date.days(), days);
            assert_eq!(Date::from_days(days), date);
        }
        assert_eq!(Date::new(2026, 11, 12).weekday_from_sunday(), 4); // uma quinta
        assert_eq!(Date::new(2026, 11, 12).weekday_from_monday(), 3);
        assert_eq!(Date::new(2026, 11, 15).weekday_from_sunday(), 0);
    }

    #[test]
    fn the_grid_starts_on_sunday_and_has_six_weeks() {
        // Fevereiro de 2026 começa num domingo: a primeira semana é a do dia 1
        let grid = month_grid(2026, 2);
        assert_eq!(grid.len(), 6);
        assert_eq!(grid[0][0], Date::new(2026, 2, 1));
        assert_eq!(grid[5][6], Date::new(2026, 3, 14));
        // Novembro de 2026 começa num domingo também; outubro, numa quinta
        assert_eq!(month_grid(2026, 10)[0][0], Date::new(2026, 9, 27));
    }

    #[test]
    fn shifting_months_crosses_years() {
        assert_eq!(shift_month(2026, 1, -1), (2025, 12));
        assert_eq!(shift_month(2026, 12, 1), (2027, 1));
        assert_eq!(shift_month(2026, 11, 0), (2026, 11));
    }

    #[test]
    fn parse_iso_dates() {
        assert_eq!(Date::parse_iso("2026-12-25"), Some(Date::new(2026, 12, 25)));
        assert_eq!(Date::parse_iso("2026-13-01"), None);
        assert_eq!(Date::parse_iso("x"), None);
    }
}
