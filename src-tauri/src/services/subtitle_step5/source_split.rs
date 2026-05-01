use super::constants::HARD_SPLIT_GAP_SECONDS;
use super::types::Step5Token;

pub(super) fn hard_pause_boundaries(tokens: &[Step5Token]) -> Vec<usize> {
    if tokens.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::<usize>::new();
    for index in 0..tokens.len() - 1 {
        let current = &tokens[index];
        let next = &tokens[index + 1];
        let gap = (next.start - current.end).max(0.0);
        if gap >= HARD_SPLIT_GAP_SECONDS {
            out.push(index + 1);
        }
    }
    out
}
