export class History {
  constructor(document, limit = 150) {
    this.document = document;
    this.savedDocument = document;
    this.undoStack = [];
    this.redoStack = [];
    this.limit = limit;
  }

  commit(label, next, selectionState = {}) {
    if (next === this.document) return false;
    this.undoStack.push({ label, before: this.document, after: next, ...selectionState });
    if (this.undoStack.length > this.limit) this.undoStack.shift();
    this.document = next;
    this.redoStack = [];
    return true;
  }

  undo() {
    const command = this.undoStack.pop();
    if (!command) return false;
    this.redoStack.push(command);
    if (!command.auxiliaryOnly) this.document = command.before;
    command.auxiliary?.undo();
    return true;
  }

  redo() {
    const command = this.redoStack.pop();
    if (!command) return false;
    this.undoStack.push(command);
    if (!command.auxiliaryOnly) this.document = command.after;
    command.auxiliary?.redo();
    return true;
  }

  markSaved(document = this.document) { this.savedDocument = document; }
  markUnsaved() { this.savedDocument = null; }
  commitAuxiliary(label, auxiliary) {
    this.undoStack.push({ label, before: this.document, after: this.document, auxiliary, auxiliaryOnly: true });
    if (this.undoStack.length > this.limit) this.undoStack.shift();
    this.redoStack = [];
  }
  get dirty() { return this.document !== this.savedDocument; }
}
