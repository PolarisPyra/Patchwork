//! How the keys move through the model picker's chips: Left and Right walk every chip in reading
//! order, Up and Down go between the groups, and both wrap round at every edge.

// Rust guideline compliant 2026-10-09

/// A place in the grid: the group's row, and the chip in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spot {
    pub row: usize,
    pub col: usize,
}

/// An arrow key, as the grid moves by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Left,
    Right,
    Up,
    Down,
}

/// Where `step` goes from `at`, in a grid whose rows hold `sizes` chips each. Up and Down keep
/// as near the same chip as the other row has.
#[must_use]
pub fn step(sizes: &[usize], at: Spot, step: Step) -> Spot {
    let rows = sizes.len();
    if rows == 0 || sizes.contains(&0) {
        return at;
    }
    let at = clamp(sizes, at);
    match step {
        Step::Up | Step::Down => {
            let row = if step == Step::Down {
                (at.row + 1) % rows
            } else {
                (at.row + rows - 1) % rows
            };
            Spot {
                row,
                col: at.col.min(sizes[row] - 1),
            }
        }
        Step::Left | Step::Right => {
            let total: usize = sizes.iter().sum();
            let index = flat(sizes, at);
            let next = if step == Step::Right {
                (index + 1) % total
            } else {
                (index + total - 1) % total
            };
            unflat(sizes, next)
        }
    }
}

/// `at` kept inside the grid after typing narrows it.
#[must_use]
pub fn clamp(sizes: &[usize], at: Spot) -> Spot {
    let Some(last) = sizes.len().checked_sub(1) else {
        return Spot::default();
    };
    let row = at.row.min(last);
    Spot {
        row,
        col: at.col.min(sizes[row].saturating_sub(1)),
    }
}

/// The chip's place in reading order, which Ctrl+1 to Ctrl+9 count by.
#[must_use]
pub fn flat(sizes: &[usize], at: Spot) -> usize {
    sizes.iter().take(at.row).sum::<usize>() + at.col
}

/// The spot of the `index`th chip in reading order, or the last one past the end.
#[must_use]
pub fn unflat(sizes: &[usize], mut index: usize) -> Spot {
    for (row, &size) in sizes.iter().enumerate() {
        if index < size {
            return Spot { row, col: index };
        }
        index -= size;
    }
    clamp(
        sizes,
        Spot {
            row: usize::MAX,
            col: usize::MAX,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: [usize; 3] = [2, 4, 1];

    #[test]
    fn right_walks_reading_order_and_wraps() {
        let mut at = Spot { row: 0, col: 1 };
        at = step(&GRID, at, Step::Right);
        assert_eq!(at, Spot { row: 1, col: 0 });
        at = step(&GRID, Spot { row: 2, col: 0 }, Step::Right);
        assert_eq!(at, Spot { row: 0, col: 0 });
        at = step(&GRID, at, Step::Left);
        assert_eq!(at, Spot { row: 2, col: 0 });
    }

    #[test]
    fn up_and_down_keep_the_column_where_they_can() {
        let at = step(&GRID, Spot { row: 1, col: 3 }, Step::Down);
        assert_eq!(at, Spot { row: 2, col: 0 });
        let at = step(&GRID, Spot { row: 0, col: 1 }, Step::Up);
        assert_eq!(at, Spot { row: 2, col: 0 });
        let at = step(&GRID, Spot { row: 2, col: 0 }, Step::Down);
        assert_eq!(at, Spot { row: 0, col: 0 });
    }

    #[test]
    fn flat_and_unflat_agree() {
        for i in 0..7 {
            assert_eq!(flat(&GRID, unflat(&GRID, i)), i);
        }
        assert_eq!(unflat(&GRID, 99), Spot { row: 2, col: 0 });
    }

    #[test]
    fn an_empty_grid_stays_put() {
        assert_eq!(step(&[], Spot::default(), Step::Right), Spot::default());
        assert_eq!(clamp(&[], Spot { row: 3, col: 3 }), Spot::default());
    }
}
