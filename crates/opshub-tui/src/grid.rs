use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Compute `n` near-square cells inside `area`.
///
/// Strategy: pick `cols = ceil(sqrt(n))`, then `rows = ceil(n / cols)`. The
/// last row gets whatever is left over. This is what tmux does for
/// `split-window` tiling and matches the user's expectation from 5pane-launcher.
pub fn tile(area: Rect, n: usize) -> Vec<Rect> {
    if n == 0 {
        return vec![];
    }
    let cols = (n as f64).sqrt().ceil() as usize;
    let rows = n.div_ceil(cols);

    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Ratio(1, rows as u32); rows])
        .split(area);

    let mut out: Vec<Rect> = Vec::with_capacity(n);
    let mut remaining = n;
    for row_area in row_areas.iter() {
        let this_row_cols = remaining.min(cols);
        remaining = remaining.saturating_sub(this_row_cols);
        let col_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![
                Constraint::Ratio(1, this_row_cols as u32);
                this_row_cols
            ])
            .split(*row_area);
        for c in col_areas.iter() {
            out.push(*c);
        }
        if remaining == 0 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_pane_fills_area() {
        let area = Rect::new(0, 0, 100, 40);
        let cells = tile(area, 1);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0], area);
    }

    #[test]
    fn four_panes_are_2x2() {
        let cells = tile(Rect::new(0, 0, 100, 40), 4);
        assert_eq!(cells.len(), 4);
    }

    #[test]
    fn five_panes_last_row_has_two() {
        // ceil(sqrt(5)) = 3 cols, ceil(5/3) = 2 rows; row 0 has 3, row 1 has 2.
        let cells = tile(Rect::new(0, 0, 120, 40), 5);
        assert_eq!(cells.len(), 5);
    }

    #[test]
    fn eight_panes_produce_8_cells() {
        let cells = tile(Rect::new(0, 0, 120, 40), 8);
        assert_eq!(cells.len(), 8);
    }

    #[test]
    fn zero_panes_is_empty() {
        assert!(tile(Rect::new(0, 0, 10, 10), 0).is_empty());
    }
}
