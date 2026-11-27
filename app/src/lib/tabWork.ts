/** The fetch timer and a focus reload belong to the repository on screen. */
export function repoWorkOnFocus(active: boolean): boolean {
  return active;
}

/** Schedule auto-fetch only for the visible repository that has remotes, when the setting is on. */
export function autoFetchScheduled(active: boolean, showingRepo: boolean, enabled: boolean): boolean {
  return repoWorkOnFocus(active) && showingRepo && enabled;
}
