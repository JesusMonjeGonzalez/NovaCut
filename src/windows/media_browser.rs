use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    All,
    Video,
    Audio,
    Generated,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Proxy {
    #[default]
    All,
    None,
    Available,
    Missing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Name,
    Duration,
    Usage,
}

#[derive(Default)]
pub struct Options {
    pub kind: Kind,
    pub offline_only: bool,
    pub proxy: Proxy,
    pub sort: Sort,
    pub descending: bool,
}

/// Validate before taking an edit snapshot or clearing the destination span.
pub fn validate_drop_destination(
    locked: bool,
    is_video: bool,
    has_video: bool,
    has_audio: bool,
) -> Result<(), &'static str> {
    if locked {
        Err("La pista de destino esta bloqueada")
    } else if is_video && !has_video {
        Err("Ese medio no tiene video")
    } else if !is_video && !has_audio {
        Err("Ese medio no tiene audio")
    } else {
        Ok(())
    }
}

/// UI-only cache: no directory walks, media probing, or persistent asset database.
#[derive(Default)]
pub struct FileStatus {
    entries: HashMap<PathBuf, bool>,
    checked_at: Option<Instant>,
}

impl FileStatus {
    pub fn begin_frame(&mut self) {
        if self
            .checked_at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(2))
        {
            self.entries.clear();
            self.checked_at = Some(Instant::now());
        }
    }

    pub fn is_file(&mut self, path: &PathBuf) -> bool {
        *self
            .entries
            .entry(path.clone())
            .or_insert_with(|| path.is_file())
    }
}

pub struct Use {
    pub index: usize,
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub duration: f64,
    pub start: f64,
    pub track: usize,
    pub offline: bool,
    pub proxy: Proxy,
}

pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub duration: f64,
    /// Timeline order, including uses whose individual names do not match search.
    pub uses: Vec<usize>,
    pub offline: bool,
    /// Unlinked, available, and missing linked proxies, counted per timeline use.
    pub proxies: [usize; 3],
    search: String,
}

impl Row {
    pub fn adjacent(&self, selected: Option<usize>, next: bool) -> Option<usize> {
        if self.uses.is_empty() {
            return None;
        }
        let position = selected.and_then(|index| self.uses.iter().position(|used| *used == index));
        let position = match (position, next) {
            (Some(position), true) => (position + 1) % self.uses.len(),
            (Some(position), false) => (position + self.uses.len() - 1) % self.uses.len(),
            (None, true) => 0,
            (None, false) => self.uses.len() - 1,
        };
        Some(self.uses[position])
    }
}

/// Group before filtering so every source action and count sees the full montage.
pub fn rows(mut uses: Vec<Use>, query: &str, options: &Options) -> Vec<Row> {
    uses.sort_by(|a, b| {
        a.start
            .total_cmp(&b.start)
            .then(a.track.cmp(&b.track))
            .then(a.index.cmp(&b.index))
    });
    let mut rows: Vec<Row> = Vec::new();
    let mut sources = HashMap::new();
    for used in uses {
        let position = if used.kind == Kind::Generated {
            rows.len()
        } else {
            *sources.entry(used.path.clone()).or_insert(rows.len())
        };
        if position == rows.len() {
            rows.push(Row {
                search: used.path.to_string_lossy().to_lowercase(),
                path: used.path,
                name: used.name.clone(),
                kind: used.kind,
                duration: 0.0,
                uses: Vec::new(),
                offline: false,
                proxies: [0; 3],
            });
        }
        let row = &mut rows[position];
        row.search.push('\n');
        row.search.push_str(&used.name.to_lowercase());
        row.uses.push(used.index);
        row.offline |= used.offline;
        if used.kind == Kind::Video {
            row.kind = Kind::Video;
        }
        if used.duration.is_finite() {
            row.duration = row.duration.max(used.duration);
        }
        if used.kind != Kind::Generated {
            row.proxies[match used.proxy {
                Proxy::Available => 1,
                Proxy::Missing => 2,
                _ => 0,
            }] += 1;
        }
    }
    let query = query.to_lowercase();
    rows.retain(|row| {
        (options.kind == Kind::All || options.kind == row.kind)
            && (!options.offline_only || row.offline)
            && match options.proxy {
                Proxy::All => true,
                Proxy::None => row.proxies[0] > 0,
                Proxy::Available => row.proxies[1] > 0,
                Proxy::Missing => row.proxies[2] > 0,
            }
            && query
                .split_whitespace()
                .all(|token| row.search.contains(token))
    });
    rows.sort_by(|a, b| {
        let primary = match options.sort {
            Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::Duration => a.duration.total_cmp(&b.duration),
            Sort::Usage => a.uses.len().cmp(&b.uses.len()),
        };
        let primary = if options.descending {
            primary.reverse()
        } else {
            primary
        };
        primary
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.uses[0].cmp(&b.uses[0]))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_drop_rejects_locked_video_and_audio_destinations() {
        for is_video in [false, true] {
            for has_video in [false, true] {
                for has_audio in [false, true] {
                    assert_eq!(
                        validate_drop_destination(true, is_video, has_video, has_audio),
                        Err("La pista de destino esta bloqueada"),
                    );
                }
            }
        }
    }

    #[test]
    fn media_drop_rejects_missing_destination_stream() {
        for has_video in [false, true] {
            assert_eq!(
                validate_drop_destination(false, false, has_video, false),
                Err("Ese medio no tiene audio"),
            );
        }
        for has_audio in [false, true] {
            assert_eq!(
                validate_drop_destination(false, true, false, has_audio),
                Err("Ese medio no tiene video"),
            );
        }
    }

    #[test]
    fn media_drop_accepts_compatible_unlocked_destinations() {
        assert_eq!(validate_drop_destination(false, true, true, false), Ok(()));
        assert_eq!(validate_drop_destination(false, true, true, true), Ok(()));
        assert_eq!(validate_drop_destination(false, false, false, true), Ok(()));
        assert_eq!(validate_drop_destination(false, false, true, true), Ok(()));
    }

    fn used(index: usize, path: &str, name: &str) -> Use {
        Use {
            index,
            path: path.into(),
            name: name.into(),
            kind: Kind::Video,
            duration: 10.0,
            start: index as f64,
            track: 0,
            offline: false,
            proxy: Proxy::None,
        }
    }

    #[test]
    fn token_search_preserves_all_uses_and_full_duration() {
        let mut first = used(0, "D:/RUSHES/scene.mov", "Wide");
        first.duration = 30.0;
        let rows = rows(
            vec![first, used(1, "D:/RUSHES/scene.mov", "Close")],
            "close RUSHES",
            &Options::default(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uses, [0, 1]);
        assert_eq!(rows[0].duration, 30.0);
        assert!(super::rows(
            vec![used(0, "a", "Wide")],
            "wide absent",
            &Options::default()
        )
        .is_empty());
    }

    #[test]
    fn filters_intersect_without_losing_mixed_proxy_uses() {
        let mut missing = used(0, "a", "A");
        missing.offline = true;
        missing.proxy = Proxy::Missing;
        let mut available = used(1, "a", "A");
        available.proxy = Proxy::Available;
        let options = Options {
            offline_only: true,
            proxy: Proxy::Missing,
            kind: Kind::Video,
            ..Options::default()
        };
        let rows = rows(vec![missing, available, used(2, "b", "B")], "", &options);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uses, [0, 1]);
        assert_eq!(rows[0].proxies, [0, 1, 1]);
    }

    #[test]
    fn generated_items_stay_separate_and_have_no_proxy_state() {
        let make = || {
            (0..2)
                .map(|i| {
                    let mut u = used(i, "", "Title");
                    u.kind = Kind::Generated;
                    u
                })
                .collect()
        };
        assert_eq!(
            rows(
                make(),
                "",
                &Options {
                    kind: Kind::Generated,
                    ..Options::default()
                }
            )
            .len(),
            2
        );
        assert!(rows(
            make(),
            "",
            &Options {
                proxy: Proxy::None,
                ..Options::default()
            }
        )
        .is_empty());
        assert!(rows(
            make(),
            "",
            &Options {
                offline_only: true,
                ..Options::default()
            }
        )
        .is_empty());
    }

    #[test]
    fn audio_filter_excludes_video() {
        let mut audio = used(1, "b.wav", "B");
        audio.kind = Kind::Audio;
        let rows = rows(
            vec![used(0, "a.mov", "A"), audio],
            "",
            &Options {
                kind: Kind::Audio,
                ..Options::default()
            },
        );
        assert_eq!(rows[0].uses, [1]);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn sorts_are_deterministic_and_reversible() {
        for sort in [Sort::Name, Sort::Duration, Sort::Usage] {
            let rows = rows(
                vec![used(1, "b", "same"), used(0, "a", "same")],
                "",
                &Options {
                    sort,
                    descending: true,
                    ..Options::default()
                },
            );
            assert_eq!(rows.iter().map(|r| r.uses[0]).collect::<Vec<_>>(), [0, 1]);
        }
        for sort in [Sort::Name, Sort::Duration, Sort::Usage] {
            let mut a = used(0, "a", "A");
            a.duration = 2.0;
            let rows = rows(
                vec![a, used(1, "b", "B"), used(2, "b", "B")],
                "",
                &Options {
                    sort,
                    descending: true,
                    ..Options::default()
                },
            );
            assert_eq!(rows[0].path, PathBuf::from("b"));
        }
    }

    #[test]
    fn navigation_wraps_in_timeline_order_with_stable_ties() {
        let mut early = used(2, "a", "A");
        early.start = 0.0;
        early.track = 1;
        let rows = rows(
            vec![used(1, "a", "A"), early, used(0, "a", "A")],
            "",
            &Options::default(),
        );
        let row = &rows[0];
        assert_eq!(row.uses, [0, 2, 1]);
        assert_eq!(row.adjacent(Some(0), false), Some(1));
        assert_eq!(row.adjacent(Some(1), true), Some(0));
        assert_eq!(row.adjacent(Some(0), true), Some(2));
        assert_eq!(row.adjacent(None, true), Some(0));
        assert_eq!(row.adjacent(Some(99), false), Some(1));
    }

    #[test]
    fn single_use_and_empty_results_are_safe() {
        let result = rows(vec![used(4, "a", "A")], "  \t ", &Options::default());
        assert_eq!(result[0].adjacent(Some(4), true), Some(4));
        assert_eq!(result[0].adjacent(None, false), Some(4));
        assert!(rows(Vec::new(), "", &Options::default()).is_empty());
        assert!(rows(
            vec![used(0, "a", "A")],
            "",
            &Options {
                offline_only: true,
                ..Options::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn file_status_reuses_results_until_expiry() {
        let mut cache = FileStatus::default();
        cache.begin_frame();
        let path = PathBuf::new();
        // A synthetic result proves repeated reads do not touch the filesystem.
        cache.entries.insert(path.clone(), true);
        cache.begin_frame();
        assert!(cache.is_file(&path));
        cache.checked_at = Some(Instant::now() - Duration::from_secs(3));
        cache.begin_frame();
        assert!(!cache.is_file(&path));
    }
}
