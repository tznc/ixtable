//! Typed grid track sizes and limits (port of `src/design/trackInput.ts`). The track editors
//! apply a number only when it is valid, and show the problem otherwise.

use crate::design::{GridTrack, TrackKind};

/// A track's size from typed text, or the problem with it.
pub fn parse_track_size(kind: &TrackKind, text: &str) -> Result<f64, String> {
    let parsed: f64 = text.trim().parse().unwrap_or(f64::NAN);
    if text.trim().is_empty() || !parsed.is_finite() || parsed <= 0.0 {
        return Err("Size must be a positive number.".into());
    }
    Ok(if *kind == TrackKind::Fixed { parsed.round().max(1.0) } else { parsed })
}

/// Which limit of a track is being typed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Limit {
    Min,
    Max,
}

/// A min or max pixel limit from typed text (blank clears it), or the problem with it.
pub fn parse_track_limit(track: &GridTrack, limit: Limit, text: &str) -> Result<Option<f64>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let parsed: f64 = text.trim().parse().unwrap_or(f64::NAN);
    if !parsed.is_finite() || parsed < 0.0 {
        return Err("Enter a whole number of pixels.".into());
    }
    let value = parsed.round();
    let min = if limit == Limit::Min { Some(value) } else { track.min };
    let max = if limit == Limit::Max { Some(value) } else { track.max };
    if let (Some(min), Some(max)) = (min, max) {
        if min > max {
            return Err("Minimum cannot exceed maximum.".into());
        }
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::{Limit, parse_track_limit, parse_track_size};
    use crate::design::{GridTrack, TrackKind};

    // Port of "parses track sizes and limits" in tests/unit/grid-regions.test.ts.
    #[test]
    fn parses_track_sizes_and_limits() {
        assert!(parse_track_size(&TrackKind::Fr, "").unwrap_err().contains("positive"));
        assert!(parse_track_size(&TrackKind::Fixed, "0").unwrap_err().contains("positive"));
        assert_eq!(parse_track_size(&TrackKind::Fixed, "80.4"), Ok(80.0));
        assert_eq!(parse_track_size(&TrackKind::Fr, "1.5"), Ok(1.5));
        let track = GridTrack { kind: TrackKind::Fr, value: Some(1.0), min: None, max: Some(50.0) };
        assert!(parse_track_limit(&track, Limit::Min, "90").unwrap_err().contains("Minimum cannot exceed"));
        assert_eq!(parse_track_limit(&track, Limit::Min, "40"), Ok(Some(40.0)));
        assert_eq!(parse_track_limit(&track, Limit::Max, ""), Ok(None));
        assert!(parse_track_limit(&track, Limit::Max, "-3").is_err());
    }
}
