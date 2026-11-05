/** One applied query for highlighting, counts and navigation; typing can run ahead of it. */
export class FindQuery {
  value = $state({ text: "", matchCase: false });
  private next = this.value;
  private timer: ReturnType<typeof setTimeout> | undefined;

  update(value: { text: string; matchCase: boolean }) {
    this.cancel();
    this.next = { ...value };
    if (!value.text) this.flush();
    else this.timer = setTimeout(() => this.flush(), 100);
  }

  /** Apply before navigating; a new query starts at its first match. */
  flush(): boolean {
    this.cancel();
    if (this.value.text === this.next.text && this.value.matchCase === this.next.matchCase) return false;
    this.value = this.next;
    return true;
  }

  cancel() {
    clearTimeout(this.timer);
    this.timer = undefined;
  }
}
