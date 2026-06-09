pub(super) fn ranges_to_boundary_ids(ranges: &[(usize, usize)]) -> Vec<usize> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::<usize>::new();
    for (index, (_start, end)) in ranges.iter().enumerate() {
        if index + 1 >= ranges.len() {
            continue;
        }
        out.push(end + 1);
    }
    out
}

pub(super) fn boundary_ids_to_ranges(
    boundaries: &[usize],
    token_len: usize,
) -> Vec<(usize, usize)> {
    if token_len == 0 {
        return Vec::new();
    }
    let mut sorted = boundaries
        .iter()
        .copied()
        .filter(|id| *id >= 1 && *id < token_len)
        .collect::<Vec<_>>();
    sorted.sort_unstable();
    sorted.dedup();
    let mut ranges = Vec::<(usize, usize)>::new();
    let mut start = 0usize;
    for boundary in sorted {
        let end = boundary.saturating_sub(1);
        if end < start {
            continue;
        }
        ranges.push((start, end));
        start = boundary;
    }
    if start < token_len {
        ranges.push((start, token_len - 1));
    }
    ranges
}
