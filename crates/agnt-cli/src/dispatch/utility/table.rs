//! Minimal column-aligned table rendering for utility command output.

// region:    --- Functions

/// Print a left-aligned table with a header row.
///
/// Column widths are the maximum of the header and every cell in that column.
pub(super) fn print_table(header: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = header.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(i) {
                *width = (*width).max(cell.len());
            }
        }
    }

    let header_cells: Vec<String> = header.iter().map(|h| (*h).to_string()).collect();
    println!("{}", format_row(&header_cells, &widths));
    for row in rows {
        println!("{}", format_row(row, &widths));
    }
}

// endregion: --- Functions

// region:    --- Support

fn format_row(cells: &[String], widths: &[usize]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let width = widths.get(i).copied().unwrap_or(0);
            format!("{cell:<width$}")
        })
        .collect::<Vec<_>>()
        .join("  ")
        .trim_end()
        .to_string()
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/dispatch/utility/table.rs"]
mod table_test;

// endregion: --- Tests
