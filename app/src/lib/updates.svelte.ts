import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { confirm } from "./confirm.svelte";
import { Latest } from "./latest";
import { prefs } from "./prefs.svelte";
import type { UpdateInfo, UpdateStatus } from "./types";
import { channelUnavailable, selectedChannel, updateNotice } from "./updates";

class Updates {
  info = $state<UpdateInfo | null>(null);
  status = $state<UpdateStatus | null>(null);
  problem = $state<string | null>(null);
  loading = $state(false);
  #loads = new Latest();

  get choice() { return prefs.get("oxbow.updates.channel"); }
  get channel() { return selectedChannel(this.info, this.choice); }
  get busy() { return !!this.status && ["checking", "downloading", "verifying", "preparing", "installing"].includes(this.status.state); }
  get restarting() { return this.status?.state === "preparing" || this.status?.state === "installing"; }
  get unsupported() { return channelUnavailable(this.info, this.choice); }
  get installed() { return this.status?.state === "installed"; }
  get canCheck() { return !!this.info && !this.loading && !this.busy && !this.problem && !this.unsupported && this.channel === this.info.channel && !this.ready && !this.installed; }
  get notice() { return updateNotice(this.info, this.choice, this.status, this.problem); }
  get available() { return !this.problem && (this.installed || (!this.unsupported && this.status?.channel === this.channel)) && this.status && "release" in this.status ? this.status.release : null; }
  get ready() { return this.status?.state === "ready" || this.status?.state === "installFailed"; }
  get canDownload() { return this.info?.canInstall && !this.busy && !!this.available && !this.ready && !this.installed; }

  async refresh() {
    const current = this.#loads.begin();
    this.loading = true;
    try {
      const [info, status] = await Promise.all([api.updateInfo(), api.updateStatus()]);
      if (!current()) return;
      this.info = info;
      this.status = status;
      this.problem = null;
    } catch {
      if (current()) this.problem = "Oxbow could not read update settings. Try again.";
    } finally {
      if (current()) this.loading = false;
    }
  }

  async check() {
    if (!this.info || this.problem) return this.refresh();
    if (!this.canCheck || !this.channel) return;
    this.#loads.drop();
    const channel = this.channel;
    this.status = { channel, state: "checking", checkedAt: null };
    try {
      this.status = await api.checkUpdates();
      await this.refresh();
    } catch {
      this.status = { channel, state: "failed", message: "Oxbow could not start the update check. Try again.", checkedAt: null };
    }
  }

  open() { api.openUpdateRelease().catch((error) => confirm.fail(error)); }
  async download() {
    if (!this.canDownload) return;
    try { this.status = await api.downloadUpdate(); } catch (error) { confirm.fail(error); await this.refresh(); }
  }
  cancel() { api.cancelUpdateDownload().catch((error) => confirm.fail(error)); }
  async install() {
    if ((!this.ready && !this.installed) || this.busy) return;
    try { this.status = await api.installUpdate(); } catch (error) { confirm.fail(error); await this.refresh(); }
  }
}

export const updates = new Updates();
listen<UpdateStatus>("updates-changed", (event) => { updates.status = event.payload; }).catch(() => {});
listen<{ key: string }>("settings-changed", (event) => {
  if (event.payload.key === "oxbow.updates.channel") void updates.refresh();
}).catch(() => {});
void updates.refresh();
