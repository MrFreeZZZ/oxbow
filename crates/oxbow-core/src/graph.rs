//! Lane layout for the commit graph.
//!
//! The layout is pure: it takes commits in display order (children before parents) and decides
//! which column every commit sits in and which line segments connect the rows. It knows nothing
//! about gitoxide, so it can be tested with hand-made histories.
//!
//! Rules from the Oxbow design:
//! - the trunk (first-parent chain of `main`/`master`) always occupies column 0;
//! - every other branch gets its own column to the right and keeps it for its whole life,
//!   columns are reused only after a branch line has ended;
//! - a branch line runs down to the commit it forks from and curves into that commit's column;
//! - colors are stable per branch name; color 0 is reserved for the trunk.

use serde::Serialize;

/// Index of a commit in the display order.
pub type RowIndex = usize;

/// Number of colors in the branch palette, not counting the trunk color.
/// The muted palette has no red or pink (removed lines), no green (added lines), no yellow (tags)
/// and no two colors of nearly the same hue.
pub const PALETTE_SIZE: u8 = 9;

/// Neutral color for stashes, outside the branch palette.
pub const STASH_COLOR: u8 = PALETTE_SIZE + 1;

/// Color of the trunk lane.
pub const TRUNK_COLOR: u8 = 0;

/// A commit as the layout sees it.
#[derive(Debug, Clone, Default)]
pub struct GraphCommit {
    /// Rows of the parents, in parent order. `None` for a parent that is not part of the
    /// loaded history (shallow clone or a history limit).
    pub parents: Vec<Option<RowIndex>>,
    /// Whether this commit is on the trunk's first-parent chain.
    pub trunk: bool,
    /// Name used to color a branch line that starts at this commit (a branch tip).
    pub tip_name: Option<String>,
    /// Name of the branch a merge brought in, used to color the line of its second parent.
    pub merged_name: Option<String>,
    /// Whether this commit exists only locally (not reachable from any remote-tracking branch).
    pub unpushed: bool,
    /// A row on a line of its own that runs to the commit it was made on, without taking over
    /// that commit's branch line: a stash entry or the uncommitted changes.
    pub side: bool,
    /// Fixed color for the row, instead of one derived from a branch name.
    pub color: Option<u8>,
}

/// The layout of one row.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RowLayout {
    /// Column of the commit dot.
    pub column: u16,
    /// Color of the commit dot (0 = trunk, `1..=PALETTE_SIZE` = branch palette, `STASH_COLOR` = stash).
    pub color: u8,
    /// Colors of branches that fork from this commit, drawn as a ring around the dot.
    pub fork_colors: Vec<u8>,
    /// For a merge commit, colors of the branches it brought in.
    pub merge_colors: Vec<u8>,
    /// Lines from the center of this row to the center of the next row.
    pub segments: Vec<Segment>,
    /// Rightmost column that has a line or a dot in this row; the commit text starts after it.
    pub width: u16,
    /// Branch the commit belongs to: the nearest branch tip above it on its line, or the branch a
    /// merge brought it in from. `None` for trunk commits (the caller knows the trunk's name) and
    /// for lines whose branch is unknown.
    pub branch: Option<String>,
}

/// A line from the center of row `i` (at column `from`) to the center of row `i + 1`
/// (at column `to`). Equal columns mean a straight line, different columns an S-curve.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub from: u16,
    pub to: u16,
    pub color: u8,
    /// Dashed lines lead down from commits that have not been pushed yet.
    pub dashed: bool,
}

#[derive(Debug, Clone)]
struct Lane {
    /// Row of the commit this lane is heading to; `None` if that commit is not loaded,
    /// in which case the lane runs to the bottom of the graph.
    target: Option<RowIndex>,
    color: u8,
    dashed: bool,
    /// The lane continues the first-parent line of a branch, as opposed to the line of a merged branch.
    first_parent: bool,
    /// Column the lane leaves from in the row that created it, when it starts with a curve.
    origin: Option<u16>,
    /// Branch name of the commits on this line.
    branch: Option<String>,
    /// A side line (stash, uncommitted changes): the commit it leads to keeps its own branch color and name.
    side: bool,
}

/// Stable color for a branch name, never the trunk color.
pub fn color_for_name(name: &str) -> u8 {
    // FNV-1a: tiny and stable across platforms and releases.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    1 + (hash % u32::from(PALETTE_SIZE)) as u8
}

/// Lay out `commits`, which must be ordered children before parents.
pub fn layout(commits: &[GraphCommit]) -> Vec<RowLayout> {
    let mut lanes: Vec<Option<Lane>> = vec![None];
    let mut rows = Vec::with_capacity(commits.len());
    // Merge lines that join a lane which already exists, per row: (from, to column, color, dashed).
    let mut joins: Vec<Vec<Segment>> = Vec::with_capacity(commits.len());
    // Lanes as they are after each row.
    let mut after: Vec<Vec<Option<Lane>>> = Vec::with_capacity(commits.len());

    for (row, commit) in commits.iter().enumerate() {
        let waiting: Vec<usize> = (0..lanes.len())
            .filter(|&c| lanes[c].as_ref().is_some_and(|l| l.target == Some(row)))
            .collect();

        let is_side = |c: usize| lanes[c].as_ref().is_some_and(|l| l.side);
        let column = if commit.trunk {
            0
        } else if let Some(&col) = waiting
            .iter()
            .find(|&&c| c != 0 && !is_side(c))
            .or_else(|| waiting.iter().find(|&&c| c != 0))
        {
            col
        } else {
            free_column(&lanes, None)
        };
        ensure_len(&mut lanes, column);
        // The commit continues the line in its column, unless that is a side line.
        let continues = waiting.contains(&column) && !lanes[column].as_ref().is_some_and(|l| l.side);

        let color = if commit.trunk {
            TRUNK_COLOR
        } else if let Some(color) = commit.color {
            color
        } else if continues {
            lanes[column].as_ref().map_or(TRUNK_COLOR, |l| l.color)
        } else {
            match &commit.tip_name {
                Some(name) => color_for_name(name),
                None => color_for_name(&format!("#{row}")),
            }
        };

        // A branch tip further down a line (a stacked branch) names the commits below it.
        let branch = if commit.trunk || commit.side {
            None
        } else if continues {
            commit
                .tip_name
                .clone()
                .or_else(|| lanes[column].as_ref().and_then(|l| l.branch.clone()))
        } else {
            commit.tip_name.clone()
        };

        // Lines that end here. A branch line arriving through its first parent marks a fork point.
        let mut fork_colors = Vec::new();
        for &col in &waiting {
            let lane = lanes[col].take().expect("waiting lane exists");
            if col != column
                && lane.first_parent
                && !lane.side
                && lane.color != color
                && !fork_colors.contains(&lane.color)
            {
                fork_colors.push(lane.color);
            }
        }

        // Lines that leave here.
        let mut merge_colors = Vec::new();
        let mut row_joins = Vec::new();
        for (index, &parent) in commit.parents.iter().enumerate() {
            if index == 0 {
                lanes[column] = Some(Lane {
                    target: parent,
                    color,
                    dashed: commit.unpushed,
                    first_parent: true,
                    origin: None,
                    branch: branch.clone(),
                    side: commit.side,
                });
                continue;
            }

            // A merged branch: join a line already heading to that parent, or start a new one.
            let existing =
                parent.and_then(|p| (0..lanes.len()).find(|&c| lanes[c].as_ref().is_some_and(|l| l.target == Some(p))));
            if let Some(col) = existing {
                let lane_color = lanes[col].as_ref().map_or(color, |l| l.color);
                if !merge_colors.contains(&lane_color) {
                    merge_colors.push(lane_color);
                }
                if col != column {
                    row_joins.push(Segment {
                        from: column as u16,
                        to: col as u16,
                        color: lane_color,
                        dashed: commit.unpushed,
                    });
                }
                continue;
            }

            let parent_on_trunk = parent.is_some_and(|p| commits[p].trunk);
            let merged_branch = if parent_on_trunk {
                None
            } else {
                commit.merged_name.clone()
            };
            let merged_color = if parent_on_trunk {
                TRUNK_COLOR
            } else {
                match &commit.merged_name {
                    Some(name) => color_for_name(name),
                    None => color_for_name(&format!("#{row}:{index}")),
                }
            };
            if !merge_colors.contains(&merged_color) {
                merge_colors.push(merged_color);
            }
            let col = if parent_on_trunk && lanes[0].is_none() {
                0
            } else {
                free_column(&lanes, Some(column))
            };
            ensure_len(&mut lanes, col);
            lanes[col] = Some(Lane {
                target: parent,
                color: merged_color,
                dashed: commit.unpushed,
                first_parent: false,
                origin: Some(column as u16),
                branch: merged_branch,
                side: false,
            });
        }

        trim(&mut lanes);
        after.push(lanes.clone());
        joins.push(row_joins);
        // A curve leaves its origin only in the row that created the lane.
        for lane in lanes.iter_mut().flatten() {
            lane.origin = None;
        }
        rows.push(RowLayout {
            column: column as u16,
            color,
            fork_colors,
            merge_colors,
            segments: Vec::new(),
            width: column as u16,
            branch,
        });
    }

    for row in 0..rows.len() {
        let next_column = rows.get(row + 1).map(|r| r.column);
        let mut segments = Vec::new();
        for (col, lane) in after[row].iter().enumerate() {
            let Some(lane) = lane else { continue };
            let to = match (lane.target, next_column) {
                (Some(target), Some(next)) if target == row + 1 => next,
                _ => col as u16,
            };
            segments.push(Segment {
                from: lane.origin.unwrap_or(col as u16),
                to,
                color: lane.color,
                dashed: lane.dashed,
            });
        }
        segments.append(&mut joins[row]);
        rows[row].segments = segments;
    }

    // The commit text starts after the rightmost dot or line entering or leaving the row.
    for row in 0..rows.len() {
        let mut width = rows[row].column;
        for seg in &rows[row].segments {
            width = width.max(seg.from).max(seg.to);
        }
        if row > 0 {
            for seg in &rows[row - 1].segments {
                width = width.max(seg.to);
            }
        }
        rows[row].width = width;
    }
    rows
}

/// First unused column from 1 on, skipping `avoid`.
fn free_column(lanes: &[Option<Lane>], avoid: Option<usize>) -> usize {
    (1..)
        .find(|&c| Some(c) != avoid && lanes.get(c).is_none_or(Option::is_none))
        .expect("there is always a free column")
}

fn ensure_len(lanes: &mut Vec<Option<Lane>>, column: usize) {
    if lanes.len() <= column {
        lanes.resize(column + 1, None);
    }
}

fn trim(lanes: &mut Vec<Option<Lane>>) {
    while lanes.len() > 1 && lanes.last().is_some_and(Option::is_none) {
        lanes.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(parents: &[usize], trunk: bool) -> GraphCommit {
        GraphCommit {
            parents: parents.iter().map(|&p| Some(p)).collect(),
            trunk,
            ..Default::default()
        }
    }

    fn named(mut c: GraphCommit, tip: &str) -> GraphCommit {
        c.tip_name = Some(tip.into());
        c
    }

    fn straight(col: u16, color: u8) -> Segment {
        Segment {
            from: col,
            to: col,
            color,
            dashed: false,
        }
    }

    #[test]
    fn linear_trunk_is_one_straight_line() {
        let rows = layout(&[commit(&[1], true), commit(&[2], true), commit(&[], true)]);
        assert!(rows.iter().all(|r| r.column == 0 && r.color == TRUNK_COLOR));
        assert_eq!(rows[0].segments, vec![straight(0, 0)]);
        assert_eq!(rows[1].segments, vec![straight(0, 0)]);
        assert!(rows[2].segments.is_empty());
    }

    #[test]
    fn branch_forks_from_trunk_and_curves_into_its_fork_point() {
        // 0 feature tip -> 2 (fork point on trunk)
        // 1 main tip -> 2
        // 2 root
        let feature = named(commit(&[2], false), "feature");
        let rows = layout(&[feature, commit(&[2], true), commit(&[], true)]);
        let color = color_for_name("feature");
        assert_eq!((rows[0].column, rows[0].color), (1, color));
        assert_eq!(rows[1].column, 0);
        // Row 1: the trunk continues straight, the feature line curves into column 0 at row 2.
        assert_eq!(
            rows[1].segments,
            vec![
                straight(0, 0),
                Segment {
                    from: 1,
                    to: 0,
                    color,
                    dashed: false
                }
            ]
        );
        assert_eq!(rows[2].fork_colors, vec![color]);
        assert_eq!(rows[1].width, 1);
        assert_eq!(rows[0].branch.as_deref(), Some("feature"));
        assert_eq!(rows[1].branch, None);
    }

    #[test]
    fn stacked_branch_tip_names_the_commits_below_it() {
        // 0 top tip -> 1 (bottom tip) -> 2 (trunk root)
        let top = named(commit(&[1], false), "stack/2");
        let bottom = named(commit(&[2], false), "stack/1");
        let rows = layout(&[top, bottom, commit(&[], true)]);
        assert_eq!(rows[0].branch.as_deref(), Some("stack/2"));
        assert_eq!(rows[1].branch.as_deref(), Some("stack/1"));
        // The line keeps the color of the branch it started with.
        assert_eq!(rows[1].color, rows[0].color);
    }

    #[test]
    fn stash_line_leaves_the_branch_below_it_alone() {
        // 0 stash -> 1 (feature tip) -> 2 (trunk root)
        let stash = GraphCommit {
            side: true,
            color: Some(STASH_COLOR),
            unpushed: true,
            ..commit(&[1], false)
        };
        let feature = named(commit(&[2], false), "feature");
        let rows = layout(&[stash, feature, commit(&[], true)]);
        assert_eq!((rows[0].color, rows[0].branch.as_deref()), (STASH_COLOR, None));
        assert_eq!(rows[0].segments[0].color, STASH_COLOR);
        assert!(rows[0].segments[0].dashed);
        assert_eq!(rows[1].color, color_for_name("feature"));
        assert_eq!(rows[1].branch.as_deref(), Some("feature"));
        assert!(rows[1].fork_colors.is_empty());
    }

    #[test]
    fn merge_brings_a_branch_back_to_trunk() {
        // 0 merge (main) parents 1 (trunk), 2 (topic)
        // 1 trunk -> 3
        // 2 topic -> 3
        // 3 root
        let mut merge = commit(&[1, 2], true);
        merge.merged_name = Some("topic".into());
        let rows = layout(&[merge, commit(&[3], true), commit(&[3], false), commit(&[], true)]);
        let topic = color_for_name("topic");
        assert_eq!(rows[0].merge_colors, vec![topic]);
        // The merged line leaves the merge dot with a curve into column 1.
        assert_eq!(
            rows[0].segments,
            vec![
                straight(0, 0),
                Segment {
                    from: 0,
                    to: 1,
                    color: topic,
                    dashed: false
                }
            ]
        );
        assert_eq!((rows[2].column, rows[2].color), (1, topic));
        assert_eq!(rows[2].branch.as_deref(), Some("topic"));
        // The topic line forked from the root, so the root gets a ring in its color.
        assert_eq!(rows[3].fork_colors, vec![topic]);
        assert_eq!(
            rows[2].segments,
            vec![
                straight(0, 0),
                Segment {
                    from: 1,
                    to: 0,
                    color: topic,
                    dashed: false
                }
            ]
        );
    }

    #[test]
    fn freed_columns_are_reused() {
        // Two branches one after the other both fork from trunk; the second reuses column 1.
        let a = named(commit(&[1], false), "a");
        let b = named(commit(&[3], false), "b");
        let rows = layout(&[a, commit(&[2], true), b, commit(&[], true)]);
        assert_eq!(rows[0].column, 1);
        assert_eq!(rows[2].column, 1);
    }

    #[test]
    fn unpushed_commits_have_dashed_lines() {
        let mut tip = named(commit(&[1], false), "wip");
        tip.unpushed = true;
        let rows = layout(&[tip, commit(&[2], false), commit(&[], true)]);
        assert!(rows[0].segments[0].dashed);
        assert!(!rows[1].segments[0].dashed);
    }

    #[test]
    fn missing_parent_runs_to_the_bottom() {
        let mut c = commit(&[], true);
        c.parents = vec![None];
        let rows = layout(&[c]);
        assert_eq!(rows[0].segments, vec![straight(0, 0)]);
    }

    #[test]
    fn colors_are_stable_and_never_trunk() {
        for name in ["main", "feature/x", "", "renovate/serde-1.x"] {
            let c = color_for_name(name);
            assert_eq!(c, color_for_name(name));
            assert!((1..=PALETTE_SIZE).contains(&c));
        }
    }
}
