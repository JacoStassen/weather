//! High/low tide detection from a sampled sea-level series.

use chrono::{Duration, DurationRound};

use crate::types::{TideEvent, TideKind, TideSample};

/// Height difference below which a high/low pair is treated as noise
/// (surge wiggles) rather than a real tide turn.
const MIN_RANGE_M: f64 = 0.02;

/// Finds high and low tides in a time-ordered, evenly sampled series.
///
/// Each turning point is refined with a parabola through it and its two
/// neighbours, so hourly samples still give times to within a few minutes.
/// The result alternates high/low.
pub fn find_extrema(series: &[TideSample]) -> Vec<TideEvent> {
    // Noise filtering works on raw sample heights: the parabola refinement can
    // exaggerate a tiny bump into a convincing-looking turn.
    let mut events: Vec<Candidate> = Vec::new();

    for w in series.windows(3) {
        let (a, b, c) = (w[0].height_m, w[1].height_m, w[2].height_m);
        let kind = if b > a && b >= c {
            TideKind::High
        } else if b < a && b <= c {
            TideKind::Low
        } else {
            continue;
        };

        // Vertex of the parabola through (-1, a), (0, b), (1, c).
        let denom = a - 2.0 * b + c;
        let (offset, height) = if denom.abs() > f64::EPSILON {
            let off = (0.5 * (a - c) / denom).clamp(-0.5, 0.5);
            (off, b - 0.25 * (a - c) * off)
        } else {
            (0.0, b)
        };
        let step = (w[2].time - w[0].time) / 2;
        let time = w[1].time + Duration::milliseconds((step.num_milliseconds() as f64 * offset) as i64);
        let time = time.duration_round(Duration::minutes(1)).unwrap_or(time);

        push_alternating(
            &mut events,
            Candidate {
                event: TideEvent { time, kind, height_m: round_cm(height) },
                raw: b,
            },
        );
    }

    // Drop high/low pairs too small to be real tides, then re-merge.
    let mut i = 1;
    while i < events.len() {
        if (events[i].raw - events[i - 1].raw).abs() < MIN_RANGE_M {
            events.drain(i - 1..=i);
            let rest = events.split_off(i.saturating_sub(1));
            for e in rest {
                push_alternating(&mut events, e);
            }
            i = 1;
        } else {
            i += 1;
        }
    }
    events.into_iter().map(|c| c.event).collect()
}

struct Candidate {
    event: TideEvent,
    /// Height of the sample at the turning point, before refinement.
    raw: f64,
}

/// Appends `e`, or if the previous event has the same kind keeps whichever is
/// more extreme.
fn push_alternating(events: &mut Vec<Candidate>, e: Candidate) {
    match events.last_mut() {
        Some(last) if last.event.kind == e.event.kind => {
            let more_extreme = match e.event.kind {
                TideKind::High => e.raw > last.raw,
                TideKind::Low => e.raw < last.raw,
            };
            if more_extreme {
                *last = e;
            }
        }
        _ => events.push(e),
    }
}

fn round_cm(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, FixedOffset, TimeZone};
    use std::f64::consts::PI;

    const M2_PERIOD_H: f64 = 12.42; // principal lunar semidiurnal tide

    fn start() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
    }

    fn sine_series(hours: usize, amp: f64, phase_h: f64) -> Vec<TideSample> {
        (0..hours)
            .map(|h| TideSample {
                time: start() + Duration::hours(h as i64),
                height_m: amp * (2.0 * PI * (h as f64 - phase_h) / M2_PERIOD_H).cos(),
            })
            .collect()
    }

    #[test]
    fn finds_semidiurnal_highs_and_lows_from_hourly_samples() {
        let phase = 1.7; // first high at 01:42
        let events = find_extrema(&sine_series(48, 1.2, phase));

        // 48 h of samples, edges excluded: highs at 1.7 + 12.42k, lows half a period later.
        assert!(events.len() >= 7, "got {}", events.len());
        for (i, e) in events.iter().enumerate() {
            let k = i as f64 / 2.0;
            let expected_h = phase + k * M2_PERIOD_H;
            let expected = start() + Duration::seconds((expected_h * 3600.0) as i64);
            let err_min = (e.time - expected).num_minutes().abs();
            assert!(err_min <= 10, "event {i} off by {err_min} min");
            let expected_kind = if i % 2 == 0 { TideKind::High } else { TideKind::Low };
            assert_eq!(e.kind, expected_kind);
            assert!((e.height_m.abs() - 1.2).abs() < 0.03, "height {}", e.height_m);
        }
    }

    #[test]
    fn ignores_small_wiggles() {
        let mut s = sine_series(30, 1.0, 0.0);
        // A 5 mm bump on the falling limb shouldn't create an extra high/low.
        s[4].height_m = s[3].height_m + 0.005;
        let events = find_extrema(&s);
        for pair in events.windows(2) {
            assert_ne!(pair[0].kind, pair[1].kind);
            assert!((pair[0].height_m - pair[1].height_m).abs() > 1.0);
        }
    }

    #[test]
    fn too_short_series_has_no_events() {
        assert!(find_extrema(&sine_series(2, 1.0, 0.0)).is_empty());
    }
}
