import type { Option } from "./settingsRows";
import type { UpdateChannel, UpdateInfo, UpdateStatus } from "./types";

export type ChannelChoice = "auto" | UpdateChannel;
export const channelLabel = (channel: UpdateChannel) => channel === "release" ? "Release" : "Preview";

export function selectedChannel(info: UpdateInfo | null, choice: ChannelChoice): UpdateChannel | null {
  return choice === "auto" ? info?.buildChannel ?? null : choice;
}

export function updateChannels(info: UpdateInfo | null, choice: ChannelChoice): Option[] {
  const options: Option[] = [{ value: "auto", label: info ? `Build default (${channelLabel(info.buildChannel)})` : "Build default" }];
  for (const channel of info?.availableChannels ?? []) options.push({ value: channel, label: channelLabel(channel) });
  if (choice !== "auto" && !options.some((o) => o.value === choice)) options.push({ value: choice, label: `${channelLabel(choice)} (unavailable on this device)` });
  return options;
}

export function channelUnavailable(info: UpdateInfo | null, choice: ChannelChoice): boolean {
  const channel = selectedChannel(info, choice);
  return !!info && !!channel && !info.availableChannels.includes(channel);
}

export function channelDescription(info: UpdateInfo | null, choice: ChannelChoice): string {
  const channel = selectedChannel(info, choice);
  if (!channel) return "Reading the channel of this build…";
  if (channelUnavailable(info, choice)) return channel === "preview"
    ? "Preview is available only for macOS Apple Silicon. Select Release on this device."
    : "Updates are not available for this operating system or architecture.";
  return channel === "preview" ? "Preview builds for macOS Apple Silicon" : "Published GitHub releases";
}

export function installedVersion(info: UpdateInfo | null): string {
  if (!info) return "Reading the installed version…";
  const platforms: Record<string, string> = { "darwin-aarch64": "Apple Silicon", "darwin-x86_64": "Intel Mac", "linux-x86_64": "Linux x64", "windows-x86_64": "Windows x64" };
  const platform = platforms[info.platform ?? ""];
  return `Oxbow ${info.version} · ${channelLabel(info.buildChannel)}${platform ? ` · ${platform}` : ""}`;
}

export function updateNotice(info: UpdateInfo | null, choice: ChannelChoice, status: UpdateStatus | null, problem: string | null): { title: string; message: string; error: boolean } {
  if (problem) return { title: "Couldn’t read update settings", message: problem, error: true };
  const channel = selectedChannel(info, choice);
  if (!info || !channel) return { title: "Reading update settings…", message: "", error: false };
  if (status?.state === "installed") return { title: `Oxbow ${status.release.version} is installed`, message: status.message, error: true };
  if (channelUnavailable(info, choice)) return { title: "Channel unavailable on this device", message: channelDescription(info, choice), error: false };
  if (status?.state === "checking") return { title: "Checking for updates…", message: status.channel === channel ? `Looking for a newer ${channelLabel(channel)} build on GitHub.` : "Finishing the current update check…", error: false };
  if (!status || status.channel !== channel || status.state === "idle") return { title: "No update check yet", message: `Check for a newer ${channelLabel(channel)} build on GitHub.`, error: false };
  if (status.state === "downloadFailed") return { title: "Couldn’t download the update", message: status.message, error: true };
  if (status.state === "installFailed") return { title: "Update installation postponed", message: status.message, error: true };
  if (status.state === "downloading") return { title: `Downloading Oxbow ${status.release.version}…`, message: downloadProgress(status.received, status.total).label, error: false };
  if (status.state === "verifying") return { title: "Verifying the update signature…", message: "Oxbow checks the package and its signed version before installation.", error: false };
  if (status.state === "ready") return { title: `Oxbow ${status.release.version} is ready to install`, message: "Signature verified. Restart when you’re ready.", error: false };
  if (status.state === "preparing") return { title: "Preparing to restart…", message: "Checking running commands and saving drafts in every window.", error: false };
  if (status.state === "installing") return { title: "Installing the update…", message: "Oxbow will restart when installation finishes.", error: false };
  if (status.state === "failed") return { title: "Couldn’t check for updates", message: status.message, error: true };
  if (status.state === "unsupported") return { title: "Updates unavailable on this device", message: status.message, error: false };
  if (status.state === "available") return { title: `Oxbow ${status.release.version} is available`, message: info.canInstall ? `A newer ${channelLabel(channel)} build is available.` : "Install the package from the release page. In-app installation is unavailable for this build or package type.", error: false };
  const checked = status.checkedAt === null ? "" : ` Last checked at ${new Date(status.checkedAt).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" })}.`;
  return { title: "No newer update available", message: `No newer ${channelLabel(channel)} build is available.${checked}`, error: false };
}

export function downloadProgress(received: number, total: number | null): { percent: number | null; label: string } {
  const mb = (n: number) => `${(Math.max(0, n) / 1024 / 1024).toFixed(1)} MB`;
  const percent = total !== null && total > 0 ? Math.min(100, Math.floor(Math.max(0, received) / total * 100)) : null;
  return { percent, label: percent === null ? `${mb(received)} downloaded` : `${mb(received)} of ${mb(total!)} · ${percent}%` };
}
