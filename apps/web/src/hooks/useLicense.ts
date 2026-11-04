import { useEffect, useSyncExternalStore } from "react";
import { api, type Feature, type LicenseStatus } from "~/api";

// One status for the whole window, so a key entered in Settings unlocks the chat at once.
let status: LicenseStatus | null = null;
let loading: Promise<void> | null = null;
const listeners = new Set<() => void>();

function publish(next: LicenseStatus): void {
  status = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The plan in effect and the key it comes from, or null until it has been read. */
export function useLicense(): LicenseStatus | null {
  useEffect(() => {
    loading ??= api.licenseStatus().then(publish, () => {});
  }, []);
  return useSyncExternalStore(subscribe, () => status);
}

/** Whether the plan in effect includes `feature`; false until the plan has been read. */
export function useFeature(feature: Feature): boolean {
  return useLicense()?.features.includes(feature) ?? false;
}

/** Takes a license key; rejects with the reason for a key Patchwork did not sign. */
export async function setLicenseKey(key: string): Promise<void> {
  publish(await api.setLicenseKey(key));
}

export async function removeLicenseKey(): Promise<void> {
  publish(await api.removeLicenseKey());
}
