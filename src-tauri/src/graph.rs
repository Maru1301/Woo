//! Incremental, renderer-independent layout for newest-to-oldest, topological history.
//! A lane owns one expected future commit hash. Each hash has at most one active
//! owner; processing that commit replaces its lane with its first parent when
//! possible. Other parents claim reusable vacant lanes, in Git parent order.

use serde::Serialize;
use std::collections::HashSet;

const MAX_LANES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphState {
    lanes: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRow {
    /// Index into the parallel CommitInfo array is the row identity.
    pub node_lane: u16,
    pub lane_count: u16,
    pub incoming: bool,
    /// Lanes passing unchanged from the top to the bottom of this row.
    pub continuations: Vec<u16>,
    /// One target lane per parent, in CommitInfo.parentHashes order.
    pub parent_lanes: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphError {
    InvalidState,
    DuplicateCommit,
    TooManyLanes,
}

fn valid_hash(hash: &str) -> bool {
    (hash.len() == 40 || hash.len() == 64) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl GraphState {
    pub fn decode(encoded: &str) -> Result<Self, GraphError> {
        if encoded.is_empty() {
            return Ok(Self::default());
        }
        let tokens: Vec<_> = encoded.split(',').collect();
        if tokens.len() > MAX_LANES || tokens.last() == Some(&"-") {
            return Err(GraphError::InvalidState);
        }
        let mut seen = HashSet::with_capacity(tokens.len());
        let mut lanes = Vec::with_capacity(tokens.len());
        for token in tokens {
            if token == "-" {
                lanes.push(None);
            } else if valid_hash(token) && seen.insert(token) {
                lanes.push(Some(token.to_owned()));
            } else {
                return Err(GraphError::InvalidState);
            }
        }
        Ok(Self { lanes })
    }

    pub fn encode(&self) -> String {
        self.lanes
            .iter()
            .map(|lane| lane.as_deref().unwrap_or("-"))
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn active_count(&self) -> usize {
        self.lanes.iter().filter(|lane| lane.is_some()).count()
    }

    fn find(&self, hash: &str) -> Option<usize> {
        self.lanes
            .iter()
            .position(|lane| lane.as_deref() == Some(hash))
    }

    fn vacant(&mut self) -> Result<usize, GraphError> {
        if let Some(index) = self.lanes.iter().position(Option::is_none) {
            return Ok(index);
        }
        if self.lanes.len() >= MAX_LANES {
            return Err(GraphError::TooManyLanes);
        }
        self.lanes.push(None);
        Ok(self.lanes.len() - 1)
    }

    /// Processes one commit. The caller supplies commits in Git's topological
    /// history order and never replays a page through the same state.
    pub fn layout(&mut self, hash: &str, parents: &[String]) -> Result<GraphRow, GraphError> {
        let mut parent_seen = HashSet::with_capacity(parents.len());
        if !valid_hash(hash)
            || parents
                .iter()
                .any(|parent| !valid_hash(parent) || parent == hash || !parent_seen.insert(parent))
        {
            return Err(GraphError::InvalidState);
        }
        let top_len = self.lanes.len();
        let expected = self.find(hash);
        let node = match expected {
            Some(index) => index,
            None => self.vacant()?,
        };
        let continuations = self
            .lanes
            .iter()
            .enumerate()
            .filter_map(|(index, lane)| (index != node && lane.is_some()).then_some(index as u16))
            .collect();
        self.lanes[node] = None;
        let mut parent_lanes = Vec::with_capacity(parents.len());
        for (index, parent) in parents.iter().enumerate() {
            let lane = if let Some(existing) = self.find(parent) {
                existing
            } else if index == 0 {
                node
            } else {
                self.vacant()?
            };
            if self.lanes[lane].is_none() {
                self.lanes[lane] = Some(parent.clone());
            }
            parent_lanes.push(lane as u16);
        }
        while self.lanes.last() == Some(&None) {
            self.lanes.pop();
        }
        let lane_count = top_len.max(self.lanes.len()).max(node + 1);
        Ok(GraphRow {
            node_lane: node as u16,
            lane_count: lane_count as u16,
            incoming: expected.is_some(),
            continuations,
            parent_lanes,
        })
    }
}

pub fn layout_page(
    state: &mut GraphState,
    commits: &[crate::history::CommitInfo],
) -> Result<Vec<GraphRow>, GraphError> {
    let mut rows = Vec::with_capacity(commits.len());
    let mut seen = HashSet::with_capacity(commits.len());
    for commit in commits {
        if !seen.insert(&commit.hash) {
            return Err(GraphError::DuplicateCommit);
        }
        rows.push(state.layout(&commit.hash, &commit.parent_hashes)?);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn h(n: usize) -> String {
        format!("{n:040x}")
    }
    fn row(state: &mut GraphState, n: usize, parents: &[usize]) -> GraphRow {
        state
            .layout(
                &h(n),
                &parents.iter().map(|parent| h(*parent)).collect::<Vec<_>>(),
            )
            .unwrap()
    }

    #[test]
    fn linear_and_root_terminate() {
        let mut state = GraphState::default();
        let a = row(&mut state, 3, &[2]);
        let b = row(&mut state, 2, &[1]);
        let c = row(&mut state, 1, &[]);
        assert_eq!(
            (a.node_lane, a.incoming, a.parent_lanes),
            (0, false, vec![0])
        );
        assert_eq!(
            (b.node_lane, b.incoming, b.parent_lanes),
            (0, true, vec![0])
        );
        assert_eq!((c.node_lane, c.incoming, c.parent_lanes), (0, true, vec![]));
        assert_eq!(state.active_count(), 0);
    }

    #[test]
    fn merge_divergence_and_lane_reuse() {
        let mut state = GraphState::default();
        let merge = row(&mut state, 5, &[4, 3]);
        assert_eq!(merge.parent_lanes, vec![0, 1]);
        assert_eq!(row(&mut state, 4, &[2]).continuations, vec![1]);
        let side = row(&mut state, 3, &[2]);
        assert_eq!(side.parent_lanes, vec![0]);
        assert_eq!(state.active_count(), 1);
        assert_eq!(row(&mut state, 2, &[1]).node_lane, 0);
        row(&mut state, 1, &[]);
        assert_eq!(row(&mut state, 9, &[8]).node_lane, 0);
    }

    #[test]
    fn octopus_nested_and_deterministic_page_continuation() {
        let run = || {
            let mut state = GraphState::default();
            let first = row(&mut state, 10, &[9, 8, 7]);
            let token = state.encode();
            let mut continued = GraphState::decode(&token).unwrap();
            let second = row(&mut continued, 9, &[6]);
            let third = row(&mut continued, 8, &[6]);
            let fourth = row(&mut continued, 7, &[6]);
            (first, second, third, fourth, continued.encode())
        };
        let a = run();
        assert_eq!(a.0.parent_lanes, vec![0, 1, 2]);
        assert_eq!(a.2.parent_lanes, vec![0]);
        assert_eq!(a.3.parent_lanes, vec![0]);
        assert_eq!(a, run());
    }

    #[test]
    fn nested_repeated_merges_keep_parent_order_and_compact_width() {
        let mut state = GraphState::default();
        let a = row(&mut state, 20, &[19, 18]);
        let b = row(&mut state, 19, &[17, 16]);
        let c = row(&mut state, 17, &[15]);
        let d = row(&mut state, 16, &[15]);
        let e = row(&mut state, 18, &[14]);
        let f = row(&mut state, 15, &[14]);
        let g = row(&mut state, 14, &[]);
        assert_eq!(a.parent_lanes, vec![0, 1]);
        assert_eq!(b.parent_lanes, vec![0, 2]);
        assert_eq!(d.parent_lanes, vec![0]);
        assert_eq!(f.parent_lanes, vec![1]);
        assert_eq!(g.parent_lanes, Vec::<u16>::new());
        assert_eq!(state.active_count(), 0);
        assert!(vec![a, b, c, d, e, f, g]
            .iter()
            .all(|row| row.lane_count <= 3));
    }

    #[test]
    fn new_head_reuses_an_internal_vacancy() {
        let mut state = GraphState::default();
        row(&mut state, 10, &[9, 8, 7]);
        row(&mut state, 9, &[6]);
        row(&mut state, 8, &[6]);
        assert_eq!(state.lanes, vec![Some(h(6)), None, Some(h(7))]);
        let new_head = row(&mut state, 20, &[19]);
        assert_eq!(new_head.node_lane, 1);
        assert_eq!(new_head.lane_count, 3);
    }

    #[test]
    fn rejects_invalid_or_duplicate_state() {
        assert_eq!(GraphState::decode("-"), Err(GraphError::InvalidState));
        assert_eq!(
            GraphState::decode(&format!("{},{}", h(1), h(1))),
            Err(GraphError::InvalidState)
        );
        let mut state = GraphState::default();
        assert_eq!(state.layout(&h(1), &[h(1)]), Err(GraphError::InvalidState));
        assert_eq!(state, GraphState::default());
    }
}
