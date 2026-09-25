//! Pure search and timeline navigation, independent of the Windows runtime.
pub fn normalized(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            '\u{e1}' | '\u{e0}' | '\u{e4}' => 'a',
            '\u{e9}' | '\u{e8}' | '\u{eb}' => 'e',
            '\u{ed}' | '\u{ec}' | '\u{ef}' => 'i',
            '\u{f3}' | '\u{f2}' | '\u{f6}' => 'o',
            '\u{fa}' | '\u{f9}' | '\u{fc}' => 'u',
            '\u{f1}' => 'n',
            _ => c,
        })
        .collect()
}

pub fn search_score(query: &str, title: &str, detail: &str) -> Option<usize> {
    let query = normalized(query);
    let title = normalized(title);
    let detail = normalized(detail);
    let tokens: Vec<_> = query.split_whitespace().collect();
    if !tokens
        .iter()
        .all(|token| title.contains(token) || detail.contains(token))
    {
        return None;
    }
    Some(if title == query.trim() {
        0
    } else if title.starts_with(query.trim()) {
        1
    } else {
        2 + tokens
            .iter()
            .filter(|token| !title.contains(**token))
            .count()
    })
}

pub fn span(intervals: impl Iterator<Item = (f64, f64)>) -> Option<(f64, f64)> {
    intervals
        .filter(|(a, b)| a.is_finite() && b.is_finite() && *a >= 0.0 && b > a)
        .fold(None, |result, (a, b)| {
            Some(match result {
                None => (a, b),
                Some((start, end)) => (start.min(a), end.max(b)),
            })
        })
}

/// Union occupied spans first: overlaps must not create phantom gaps.
pub fn gaps(mut intervals: Vec<(f64, f64)>, end: f64, minimum: f64) -> Vec<(f64, f64)> {
    if !end.is_finite() || end <= 0.0 || !minimum.is_finite() || minimum <= 0.0 {
        return Vec::new();
    }
    intervals.retain(|(a, b)| a.is_finite() && b.is_finite() && b > a && *b > 0.0 && *a < end);
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cursor: f64 = 0.0;
    let mut result = Vec::new();
    for (a, b) in intervals {
        let a = a.clamp(0.0, end);
        if a - cursor >= minimum {
            result.push((cursor, a));
        }
        cursor = cursor.max(b.min(end));
    }
    if end - cursor >= minimum {
        result.push((cursor, end));
    }
    result
}

pub fn next_point(
    points: impl Iterator<Item = f64>,
    current: f64,
    forward: bool,
    epsilon: f64,
) -> Option<f64> {
    points
        .filter(|p| {
            p.is_finite()
                && if forward {
                    *p > current + epsilon
                } else {
                    *p < current - epsilon
                }
        })
        .reduce(|a, b| if forward { a.min(b) } else { a.max(b) })
}

pub fn fit_span(start: f64, end: f64, total: f64) -> (f32, f64) {
    let total = total.max(10.0);
    let width = ((end - start).max(0.04) * 1.2).min(total);
    let zoom = (total / width).clamp(1.0, 400.0) as f32;
    let view = total / zoom as f64;
    (
        zoom,
        ((start + end - view) / 2.0).clamp(0.0, (total - view).max(0.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_ignores_case_accents_and_word_order() {
        assert!(search_score("color seleccion", "Selecci\u{f3}n: Color", "").is_some());
    }
    #[test]
    fn search_requires_every_token_and_includes_details() {
        assert!(search_score("toma v2", "Toma 5", "V2 00:01:00").is_some());
        assert!(search_score("toma v3", "Toma 5", "V2").is_none());
    }
    #[test]
    fn title_matches_rank_above_metadata() {
        assert!(
            search_score("audio", "Audio", "").unwrap()
                < search_score("audio", "Clip", "audio").unwrap()
        );
    }
    #[test]
    fn empty_query_returns_results() {
        assert!(search_score("  ", "Clip", "").is_some());
    }
    #[test]
    fn span_ignores_invalid_data() {
        assert_eq!(
            span([(5.0, 8.0), (1.0, 3.0), (f64::NAN, 9.0), (8.0, 7.0)].into_iter()),
            Some((1.0, 8.0))
        );
        assert_eq!(span(std::iter::empty()), None);
    }
    #[test]
    fn overlapping_and_nested_clips_do_not_create_gaps() {
        assert_eq!(
            gaps(vec![(4.0, 8.0), (1.0, 6.0), (2.0, 3.0)], 10.0, 0.04),
            vec![(0.0, 1.0), (8.0, 10.0)]
        );
    }
    #[test]
    fn adjacent_clips_and_subframe_holes_are_not_gaps() {
        assert!(gaps(vec![(0.0, 2.0), (2.0, 4.0), (4.01, 6.0)], 6.0, 0.04).is_empty());
    }
    #[test]
    fn gaps_clamp_to_project_and_reject_nonfinite_spans() {
        assert_eq!(
            gaps(vec![(-2.0, 1.0), (f64::NAN, 3.0), (4.0, 12.0)], 5.0, 0.04),
            vec![(1.0, 4.0)]
        );
    }
    #[test]
    fn navigation_is_order_independent_and_does_not_wrap() {
        assert_eq!(
            next_point([9.0, 2.0, 5.0].into_iter(), 5.0, true, 0.01),
            Some(9.0)
        );
        assert_eq!(
            next_point([9.0, 2.0, 5.0].into_iter(), 5.0, false, 0.01),
            Some(2.0)
        );
        assert_eq!(next_point([1.0].into_iter(), 1.0, true, 0.01), None);
    }
    #[test]
    fn selection_fit_contains_both_edges() {
        let (zoom, scroll) = fit_span(40.0, 50.0, 100.0);
        assert!(scroll <= 40.0 && scroll + 100.0 / zoom as f64 >= 50.0);
        assert_eq!(fit_span(0.0, 100.0, 100.0), (1.0, 0.0));
    }
}
