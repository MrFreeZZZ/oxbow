// The draft saves of this window's Conflicts screens, outliving each screen (draftSaves.ts). They
// live apart from the screen so that closing the window can wait for them without loading it.

import { api } from "./api";
import { DraftSaves } from "./draftSaves";

export const draftSaves = new DraftSaves(api);
