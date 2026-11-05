import { invoke } from "~/api/invoke";
import type { Account, AccountDevice, LoginStep } from "~/api/types";

export const accountsApi = {
  /** The account this install is signed in to, or null. */
  getAccount: () => invoke<Account | null>("get_account"),
  /** Emails the code that proves the address. */
  accountRegister: (email: string) => invoke<void>("account_register", { email }),
  accountRegisterFinish: (email: string, code: string, password: string, name: string | null) =>
    invoke<Account>("account_register_finish", { email, code, password, name }),
  accountLogin: (email: string, password: string) =>
    invoke<LoginStep>("account_login", { email, password }),
  /** `remember` skips the code on this computer for 30 days. */
  accountLoginVerify: (challenge: string, code: string, remember: boolean) =>
    invoke<Account>("account_login_verify", { challenge, code, remember }),
  accountResetPassword: (email: string) => invoke<void>("account_reset_password", { email }),
  accountResetPasswordFinish: (email: string, code: string, password: string) =>
    invoke<void>("account_reset_password_finish", { email, code, password }),
  accountRename: (name: string | null) => invoke<Account>("account_rename", { name }),
  accountDevices: () => invoke<AccountDevice[]>("account_devices"),
  accountRemoveDevice: (id: string) => invoke<void>("account_remove_device", { id }),
  accountSignOut: () => invoke<void>("account_sign_out"),
  /** Ends every session but this one. */
  accountSignOutOthers: () => invoke<void>("account_sign_out_others"),
  accountDelete: (password: string) => invoke<void>("account_delete", { password }),
};
