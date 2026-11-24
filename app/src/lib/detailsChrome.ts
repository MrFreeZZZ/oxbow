/** The details panel is one window setting, shared by every repository tab. */

export const DETAILS_PANEL_MIN = 320;

export const DETAILS_WIDTH_KEY = "oxbow.history.detailsWidth";
export const DETAILS_HIDDEN_KEY = "oxbow.history.detailsHidden";
export const CHANGES_EXPANDED_KEY = "oxbow.changes.expanded";

export interface DetailsChrome {
  panelWidth: number | null;
  detailsHidden: boolean;
  changesExpanded: boolean;
}

export function detailsChromeEqual(a: DetailsChrome, b: DetailsChrome): boolean {
  return a.panelWidth === b.panelWidth && a.detailsHidden === b.detailsHidden && a.changesExpanded === b.changesExpanded;
}

/** Apply one settings.json value. Other keys, and values of the wrong type, leave it as it is. */
export function applyDetailsChrome(chrome: DetailsChrome, key: string, value: unknown): DetailsChrome {
  if (key === DETAILS_WIDTH_KEY) {
    if (typeof value === "number" && Number.isFinite(value)) return { ...chrome, panelWidth: Math.max(value, DETAILS_PANEL_MIN) };
    if (value === null) return { ...chrome, panelWidth: null };
  }
  if (key === DETAILS_HIDDEN_KEY && (typeof value === "boolean" || value === null)) {
    return { ...chrome, detailsHidden: value === true };
  }
  if (key === CHANGES_EXPANDED_KEY && (typeof value === "boolean" || value === null)) {
    return { ...chrome, changesExpanded: value === true };
  }
  return chrome;
}
