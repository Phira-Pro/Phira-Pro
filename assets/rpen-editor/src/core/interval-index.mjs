export class IntervalIndex {
  constructor(items, start, end) {
    this.entries = items.map((item, index) => ({ item, index, start: start(item), end: end(item) })).sort((left, right) => left.start - right.start);
    const build = (low, high) => {
      if (low >= high) return null;
      const middle = Math.floor((low + high) / 2);
      const left = build(low, middle);
      const right = build(middle + 1, high);
      const entry = this.entries[middle];
      return { entry, left, right, maxEnd: Math.max(entry.end, left?.maxEnd ?? -Infinity, right?.maxEnd ?? -Infinity) };
    };
    this.root = build(0, this.entries.length);
  }

  query(start, end) {
    const matches = [];
    const visit = node => {
      if (!node || node.maxEnd < start) return;
      visit(node.left);
      if (node.entry.start > end) return;
      if (node.entry.end >= start) matches.push(node.entry);
      visit(node.right);
    };
    visit(this.root);
    return matches;
  }

  has(start, end) {
    const visit = node => {
      if (!node || node.maxEnd < start) return false;
      if (visit(node.left)) return true;
      if (node.entry.start > end) return false;
      return node.entry.end >= start || visit(node.right);
    };
    return visit(this.root);
  }
}
