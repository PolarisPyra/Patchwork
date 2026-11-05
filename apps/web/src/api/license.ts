import { invoke } from "~/api/invoke";
import type { LicenseStatus } from "~/api/types";

export const licenseApi = {
  /** The plan in effect, and the license key it comes from. */
  licenseStatus: () => invoke<LicenseStatus>("license_status"),
  /** Takes a license key in place of any before it; fails for a key Patchwork did not sign, or
   * one for another account than the one signed in. */
  setLicenseKey: (key: string) => invoke<LicenseStatus>("set_license_key", { key }),
  removeLicenseKey: () => invoke<LicenseStatus>("remove_license_key"),
  /** Starts the free trial, which the account service gives each account once. */
  startTrial: () => invoke<LicenseStatus>("start_trial"),
};
