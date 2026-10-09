//! Lane layout for the commit graph.
//!
//! The layout is pure: it takes commits in display order (children before parents) and decides
//! which column every commit sits in and which line segments connect the rows. It knows nothing
//! about gitoxide, so it can be tested with hand-made histories.
//!
//! Rules from the Oxbow design:
//! - the trunk (first-parent chain of `main`/`master`) occupies column 0;
//! - every other branch gets its own column to the right and keeps it for its whole life,
//!   columns are reused only after a branch line has ended;
//! - a branch line runs down to the commit it forks from and curves into that commit's column;
//! - colors are stable per branch name; color 0 is reserved for the trunk (the production branch);
//! - the checked-out branch's line takes column 0 from the top of the graph down to its fork
//!   point. Only the trunk moves out of its way, into the column that line had, and curves back
//!   at the fork point; every other line keeps its column and color.

use std::sync::atomic::{AtomicU8, Ordering};

use serde::Serialize;

/// Index of a commit in the display order.
pub type RowIndex = usize;

/// Most branch colors a palette has, not counting the trunk color: the Oxbow palette's nine.
/// Mineral, Paper and Signal have seven. None of them has red or pink (removed lines), green
/// (added lines) or yellow (tags).
pub const MAX_BRANCH_COLORS: u8 = 9;

/// Branch colors of the default palette, Mineral.
pub const DEFAULT_BRANCH_COLORS: u8 = 7;

/// Neutral color for stashes, outside every branch palette.
pub const STASH_COLOR: u8 = 10;

/// Gray for commits that belong to no branch: made on a detached `HEAD`.
pub const NO_BRANCH_COLOR: u8 = 11;

static BRANCH_COLORS: AtomicU8 = AtomicU8::new(DEFAULT_BRANCH_COLORS);

/// How many branch colors the chosen palette has (Settings › Themes › Branch palette).
pub fn set_branch_colors(count: u8) {
    BRANCH_COLORS.store(count.clamp(1, MAX_BRANCH_COLORS), Ordering::Relaxed);
}

/// Branch colors of the chosen palette: names hash into `1..=branch_colors()`.
pub fn branch_colors() -> u8 {
    BRANCH_COLORS.load(Ordering::Relaxed)
}

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
    /// Commit id. A line that has no branch name takes its color from the id of the commit it
    /// starts at, so it keeps that color as newer commits push it down.
    pub id: String,
    /// The row of uncommitted changes, a side row on top of `HEAD`.
    pub worktree: bool,
}

/// The layout of a whole history.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub rows: Vec<RowLayout>,
    /// Gray dashed lines from the top of the graph down to a commit, filling an empty column
    /// above a line that starts below newer commits of other branches.
    pub lead_ins: Vec<LeadIn>,
}

/// A gray dashed line in `column` from the top of the graph down to the dot of `row`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LeadIn {
    pub column: u16,
    pub row: RowIndex,
}

/// The layout of one row.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RowLayout {
    /// Column of the commit dot.
    pub column: u16,
    /// Color of the commit dot (0 = trunk, `1..=branch_colors()` = branch palette, `STASH_COLOR` = stash).
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
    /// The trunk's own line, drawn thicker than the others.
    pub thick: bool,
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
    /// The trunk's first-parent line.
    trunk: bool,
    /// The line of commits with a fixed color (made on a detached `HEAD`): a commit below it on a
    /// branch continues its branch's line instead, and doesn't take this color.
    fixed: bool,
}

/// Stable color for a branch name, never the trunk color.
pub fn color_for_name(name: &str) -> u8 {
    // FNV-1a: tiny and stable across platforms and releases.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    1 + (hash % u32::from(branch_colors())) as u8
}

/// Lay out `commits`, which must be ordered children before parents. `focus` is the row of the
/// checked-out commit: its line moves to column 0.
pub fn layout(commits: &[GraphCommit], focus: Option<RowIndex>) -> Graph {
    let plain = Plan::plain(commits.len());
    let mut rows = place(commits, &plain);
    let plan = focus.and_then(|row| Plan::focused(commits, &rows, row));
    if let Some(plan) = &plan {
        rows = place(commits, plan);
    }

    let mut lead_ins = Vec::new();
    let top_left = plan.as_ref().and_then(|p| p.left.iter().position(|&l| l));
    if let Some(top) = top_left.filter(|&t| t > 0) {
        lead_ins.push(LeadIn { column: 0, row: top });
    }
    // The trunk starts below newer work: its column stays empty above it, unless the focused line is there.
    if let Some(tip) = commits.iter().position(|c| c.trunk).filter(|&t| t > 0) {
        let column = rows[tip].column;
        if column != 0 || top_left.is_none_or(|top| top > tip) {
            lead_ins.push(LeadIn { column, row: tip });
        }
    }
    Graph { rows, lead_ins }
}

/// Where the trunk and the focused line go.
struct Plan {
    /// Rows drawn in column 0: the focused line and the uncommitted changes on top of it.
    left: Vec<bool>,
    /// Column 0 belongs to the focused line rather than the trunk, until `until`.
    focused: bool,
    /// Column of the trunk while column 0 is the focused line's.
    trunk_column: usize,
    /// Row where the focused line ends, at the commit it forks from; `None` if it runs to the bottom.
    until: Option<RowIndex>,
}

impl Plan {
    fn plain(len: usize) -> Plan {
        Plan {
            left: vec![false; len],
            focused: false,
            trunk_column: 0,
            until: None,
        }
    }

    /// The plan that puts the line of `head` in column 0, from the layout without one.
    fn focused(commits: &[GraphCommit], plain: &[RowLayout], head: RowIndex) -> Option<Plan> {
        let first_parent = |row: RowIndex| commits[row].parents.first().copied().flatten();
        let mut plan = Plan::plain(commits.len());
        let mut top = head;
        if commits[head].trunk {
            // The trunk is in column 0 already. Uncommitted changes on its newest commit go right above it.
            if commits.iter().position(|c| c.trunk) != Some(head) {
                return None;
            }
        } else {
            let column = usize::from(plain[head].column);
            plan.focused = true;
            plan.left[head] = true;
            // Down to the commit the line forks from.
            let mut row = head;
            plan.until = loop {
                match first_parent(row) {
                    Some(p) if usize::from(plain[p].column) == column && !commits[p].trunk && !commits[p].side => {
                        plan.left[p] = true;
                        row = p;
                    }
                    Some(p) => break Some(p),
                    None => break None,
                }
            };
            // Up through the branches stacked on it, which share the line.
            while let Some(child) = (0..top)
                .rev()
                .find(|&r| !commits[r].side && usize::from(plain[r].column) == column && first_parent(r) == Some(top))
            {
                plan.left[child] = true;
                top = child;
            }
            let trunk_above = commits[..plan.until.unwrap_or(commits.len())].iter().any(|c| c.trunk);
            plan.trunk_column = if trunk_above { column } else { 0 };
        }
        if let Some(wip) = (0..top).find(|&r| commits[r].worktree && first_parent(r) == Some(top)) {
            plan.left[wip] = true;
        }
        plan.left.contains(&true).then_some(plan)
    }

    /// Whether column 0 is the focused line's in `row`.
    fn focused_at(&self, row: RowIndex) -> bool {
        self.focused && self.until.is_none_or(|until| row < until)
    }

    fn trunk_column_at(&self, row: RowIndex) -> usize {
        if self.focused_at(row) { self.trunk_column } else { 0 }
    }
}

fn place(commits: &[GraphCommit], plan: &Plan) -> Vec<RowLayout> {
    let mut lanes: Vec<Option<Lane>> = vec![None];
    let mut rows = Vec::with_capacity(commits.len());
    // Merge lines that join a lane which already exists, per row: (from, to column, color, dashed).
    let mut joins: Vec<Vec<Segment>> = Vec::with_capacity(commits.len());
    // Lanes as they are after each row.
    let mut after: Vec<Vec<Option<Lane>>> = Vec::with_capacity(commits.len());

    for (row, commit) in commits.iter().enumerate() {
        let trunk_column = plan.trunk_column_at(row);
        let focused = plan.focused_at(row);
        let reserved = |c: usize| c == trunk_column || (focused && c == 0);
        let waiting: Vec<usize> = (0..lanes.len())
            .filter(|&c| lanes[c].as_ref().is_some_and(|l| l.target == Some(row)))
            .collect();

        // Which of the lines arriving here goes on through this commit: a branch line before a side
        // line or the line of commits on no branch, a line of pushed commits before one of local
        // work forked from it, then the leftmost.
        let rank = |c: usize| {
            lanes[c]
                .as_ref()
                .map_or((true, true, true), |l| (l.side, l.fixed, l.dashed))
        };
        let column = if plan.left[row] {
            0
        } else if commit.trunk {
            trunk_column
        } else if let Some(&col) = waiting.iter().filter(|&&c| !reserved(c)).min_by_key(|&&c| rank(c)) {
            col
        } else {
            free_column(&lanes, &[trunk_column])
        };
        ensure_len(&mut lanes, column);
        // The commit continues the line in its column, unless that is a side line.
        let continues = waiting.contains(&column) && !lanes[column].as_ref().is_some_and(|l| l.side);
        // It takes that line's color and branch, unless the line's color belongs to its own commits.
        let inherits = continues && !lanes[column].as_ref().is_some_and(|l| l.fixed);

        let color = if commit.trunk {
            TRUNK_COLOR
        } else if let Some(color) = commit.color {
            color
        } else if inherits {
            lanes[column].as_ref().map_or(TRUNK_COLOR, |l| l.color)
        } else {
            match &commit.tip_name {
                Some(name) => color_for_name(name),
                None => color_for_name(&format!("#{}", commit.id)),
            }
        };

        // A branch tip further down a line (a stacked branch) names the commits below it.
        let branch = if commit.trunk || commit.side {
            None
        } else if inherits {
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
            if lane.first_parent && !lane.side && lane.color != color && !fork_colors.contains(&lane.color) {
                fork_colors.push(lane.color);
            }
        }

        // The focused line ended here: the trunk curves back into column 0.
        if plan.focused && plan.until == Some(row) && plan.trunk_column != 0 && lanes[0].is_none() {
            let moved = lanes.get_mut(plan.trunk_column).and_then(|l| l.take_if(|l| l.trunk));
            if let Some(mut lane) = moved {
                lane.origin = Some(plan.trunk_column as u16);
                lanes[0] = Some(lane);
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
                    trunk: commit.trunk,
                    fixed: commit.color.is_some() && !commit.side,
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
                        thick: false,
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
                    None => match parent {
                        Some(p) => color_for_name(&format!("#{}", commits[p].id)),
                        None => color_for_name(&format!("#{}:{index}", commit.id)),
                    },
                }
            };
            if !merge_colors.contains(&merged_color) {
                merge_colors.push(merged_color);
            }
            ensure_len(&mut lanes, trunk_column);
            let col = if parent_on_trunk && lanes[trunk_column].is_none() && !(focused && trunk_column == 0) {
                trunk_column
            } else {
                free_column(&lanes, &[column, trunk_column])
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
                trunk: false,
                fixed: false,
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
                thick: lane.trunk,
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
fn free_column(lanes: &[Option<Lane>], avoid: &[usize]) -> usize {
    (1..)
        .find(|&c| !avoid.contains(&c) && lanes.get(c).is_none_or(Option::is_none))
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
            thick: color == TRUNK_COLOR,
        }
    }

    #[test]
    fn linear_trunk_is_one_straight_line() {
        let rows = layout(&[commit(&[1], true), commit(&[2], true), commit(&[], true)], None).rows;
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
        let rows = layout(&[feature, commit(&[2], true), commit(&[], true)], None).rows;
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
                    dashed: false,
                    thick: false,
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
        let rows = layout(&[top, bottom, commit(&[], true)], None).rows;
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
        let rows = layout(&[stash, feature, commit(&[], true)], None).rows;
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
        let rows = layout(
            &[merge, commit(&[3], true), commit(&[3], false), commit(&[], true)],
            None,
        )
        .rows;
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
                    dashed: false,
                    thick: false,
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
                    dashed: false,
                    thick: false,
                }
            ]
        );
    }

    #[test]
    fn freed_columns_are_reused() {
        // Two branches one after the other both fork from trunk; the second reuses column 1.
        let a = named(commit(&[1], false), "a");
        let b = named(commit(&[3], false), "b");
        let rows = layout(&[a, commit(&[2], true), b, commit(&[], true)], None).rows;
        assert_eq!(rows[0].column, 1);
        assert_eq!(rows[2].column, 1);
    }

    #[test]
    fn unpushed_commits_have_dashed_lines() {
        let mut tip = named(commit(&[1], false), "wip");
        tip.unpushed = true;
        let rows = layout(&[tip, commit(&[2], false), commit(&[], true)], None).rows;
        assert!(rows[0].segments[0].dashed);
        assert!(!rows[1].segments[0].dashed);
    }

    #[test]
    fn missing_parent_runs_to_the_bottom() {
        let mut c = commit(&[], true);
        c.parents = vec![None];
        let rows = layout(&[c], None).rows;
        assert_eq!(rows[0].segments, vec![straight(0, 0)]);
    }

    fn curve(from: u16, to: u16, color: u8) -> Segment {
        Segment {
            from,
            to,
            color,
            dashed: false,
            thick: color == TRUNK_COLOR,
        }
    }

    #[test]
    fn checked_out_branch_takes_column_zero_and_the_trunk_steps_aside() {
        // 0 main tip -> 2
        // 1 feature tip (HEAD) -> 2
        // 2 root, where feature forks from main
        let feature = named(commit(&[2], false), "feature");
        let color = color_for_name("feature");
        let graph = layout(&[commit(&[2], true), feature, commit(&[], true)], Some(1));
        let rows = &graph.rows;
        assert_eq!((rows[0].column, rows[1].column, rows[2].column), (1, 0, 0));
        // Colors don't change, only columns.
        assert_eq!((rows[0].color, rows[1].color), (TRUNK_COLOR, color));
        assert_eq!(rows[0].segments, vec![straight(1, TRUNK_COLOR)]);
        // The trunk curves back into column 0 at the fork point; the feature line runs straight into it.
        assert_eq!(rows[1].segments, vec![straight(0, color), curve(1, 0, TRUNK_COLOR)]);
        assert_eq!(rows[2].fork_colors, vec![color]);
        // Column 0 is empty above the feature's tip.
        assert_eq!(graph.lead_ins, vec![LeadIn { column: 0, row: 1 }]);
    }

    #[test]
    fn the_whole_stack_moves_and_the_trunk_keeps_a_lead_in() {
        // 0 stack/2 -> 1 stack/1 (HEAD) -> 3 root; 2 main tip -> 3
        let top = named(commit(&[1], false), "stack/2");
        let bottom = named(commit(&[3], false), "stack/1");
        let graph = layout(&[top, bottom, commit(&[3], true), commit(&[], true)], Some(1));
        let columns: Vec<u16> = graph.rows.iter().map(|r| r.column).collect();
        assert_eq!(columns, [0, 0, 1, 0]);
        assert_eq!(graph.lead_ins, vec![LeadIn { column: 1, row: 2 }]);
    }

    #[test]
    fn a_nested_branch_moves_only_its_own_commits() {
        // 0 base tip -> 2; 1 nested tip (HEAD) -> 2, forked from base; 2 base -> 3; 3 root
        let base = named(commit(&[2], false), "base");
        let child = named(commit(&[2], false), "nested");
        let graph = layout(&[base, child, commit(&[3], false), commit(&[], true)], Some(1));
        let rows = &graph.rows;
        assert_eq!((rows[0].column, rows[1].column, rows[2].column), (1, 0, 1));
        assert_eq!(rows[2].color, color_for_name("base"));
        // The nested line curves into its fork point on base.
        assert!(rows[1].segments.contains(&curve(0, 1, color_for_name("nested"))));
        assert_eq!(rows[2].fork_colors, vec![color_for_name("nested")]);
    }

    #[test]
    fn uncommitted_changes_sit_right_above_the_trunk_tip() {
        // 0 uncommitted -> 2 (main tip, HEAD); 1 feature -> 3; 3 root
        let wip = GraphCommit {
            side: true,
            worktree: true,
            color: Some(TRUNK_COLOR),
            unpushed: true,
            ..commit(&[2], false)
        };
        let feature = named(commit(&[3], false), "feature");
        let graph = layout(&[wip, feature, commit(&[3], true), commit(&[], true)], Some(2));
        let columns: Vec<u16> = graph.rows.iter().map(|r| r.column).collect();
        assert_eq!(columns, [0, 1, 0, 0]);
        assert!(graph.lead_ins.is_empty());
    }

    #[test]
    fn without_focus_the_trunk_gets_a_lead_in_when_it_starts_lower() {
        let feature = named(commit(&[1], false), "feature");
        let graph = layout(&[feature, commit(&[], true)], None);
        assert_eq!(graph.lead_ins, vec![LeadIn { column: 0, row: 1 }]);
    }

    #[test]
    fn commits_on_no_branch_keep_their_gray_to_themselves() {
        // 0 made on a detached HEAD -> 2; 1 feature tip -> 2; 2 feature -> 3; 3 root
        let detached = GraphCommit {
            color: Some(NO_BRANCH_COLOR),
            unpushed: true,
            ..commit(&[2], false)
        };
        let feature = named(commit(&[2], false), "feature");
        let graph = layout(&[detached, feature, commit(&[3], false), commit(&[], true)], Some(0));
        let rows = &graph.rows;
        // The feature line carries on below the fork; the gray line ends there with a ring.
        assert_eq!((rows[0].column, rows[1].column, rows[2].column), (0, 1, 1));
        assert_eq!(rows[2].color, color_for_name("feature"));
        assert_eq!(rows[2].branch.as_deref(), Some("feature"));
        assert_eq!(rows[2].fork_colors, vec![NO_BRANCH_COLOR]);
    }

    #[test]
    fn local_work_forked_from_a_pushed_branch_leaves_it_its_line() {
        // 0 local tip (not pushed) -> 2; 1 pushed tip -> 2; 2 pushed -> 3; 3 root
        let mut local = named(commit(&[2], false), "experiment");
        local.unpushed = true;
        let pushed = named(commit(&[2], false), "auth");
        let rows = layout(&[local, pushed, commit(&[3], false), commit(&[], true)], None).rows;
        assert_eq!((rows[0].column, rows[1].column, rows[2].column), (1, 2, 2));
        assert_eq!(rows[2].branch.as_deref(), Some("auth"));
        assert_eq!(rows[2].fork_colors, vec![color_for_name("experiment")]);
    }

    #[test]
    fn colors_are_stable_and_never_trunk() {
        for name in ["main", "feature/x", "", "renovate/serde-1.x"] {
            let c = color_for_name(name);
            assert_eq!(c, color_for_name(name));
            assert!((1..=branch_colors()).contains(&c));
        }
    }
}
