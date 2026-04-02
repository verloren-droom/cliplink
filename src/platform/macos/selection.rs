use super::*;

pub(super) fn index_set_from_rows(rows: &[usize]) -> Retained<NSMutableIndexSet> {
    let indexes = NSMutableIndexSet::new();
    for row in rows {
        indexes.addIndex(*row as NSUInteger);
    }
    indexes
}

pub(super) fn selected_rows_from_table(table_view: &NSTableView) -> Vec<usize> {
    let indexes = table_view.selectedRowIndexes();
    let mut rows = Vec::with_capacity(indexes.count());
    let mut index = indexes.firstIndex();
    while index != NSNotFound as NSUInteger {
        rows.push(index);
        index = indexes.indexGreaterThanIndex(index);
    }
    rows
}
