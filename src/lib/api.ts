import { invoke } from "@tauri-apps/api/core";
import type { Account } from "./bindings/Account";
import type { AppInfo } from "./bindings/AppInfo";
import type { BotStatus } from "./bindings/BotStatus";
import type { ChannelEntry } from "./bindings/ChannelEntry";
import type { FailureRecord } from "./bindings/FailureRecord";
import type { Inventory } from "./bindings/Inventory";
import type { JobSnapshot } from "./bindings/JobSnapshot";
import type { JobSpec } from "./bindings/JobSpec";
import type { PackageIndex } from "./bindings/PackageIndex";
import type { Preview } from "./bindings/Preview";
import type { Rule } from "./bindings/Rule";
import type { RuleDraft } from "./bindings/RuleDraft";
import type { SettingEntry } from "./bindings/SettingEntry";
import type { Stats } from "./bindings/Stats";

export const api = {
  uiReady: () => invoke<void>("ui_ready"),
  appInfo: () => invoke<AppInfo>("app_info"),
  setWindowTheme: (theme: string) => invoke<void>("window_set_theme", { theme }),

  accounts: () => invoke<Account[]>("accounts_list"),
  addToken: (token: string) => invoke<Account>("account_add_token", { token }),
  openLogin: () => invoke<void>("account_open_login"),
  removeAccount: (id: string) => invoke<void>("account_remove", { id }),
  demoSignIn: () => invoke<Account>("demo_sign_in"),

  inventory: (account: string) => invoke<Inventory>("inventory", { account }),
  guildChannels: (account: string, guild: string) =>
    invoke<ChannelEntry[]>("guild_channels", { account, guild }),
  checkPattern: (pattern: string) => invoke<string | null>("check_pattern", { pattern }),

  previewStart: (previewId: string, spec: JobSpec) =>
    invoke<Preview>("preview_start", { previewId, spec }),
  previewCancel: (previewId: string) => invoke<void>("preview_cancel", { previewId }),

  jobStart: (spec: JobSpec) => invoke<JobSnapshot>("job_start", { spec }),
  jobPause: (id: string) => invoke<JobSnapshot>("job_pause", { id }),
  jobResume: (id: string) => invoke<JobSnapshot>("job_resume", { id }),
  jobStop: (id: string) => invoke<void>("job_stop", { id }),
  jobsActive: () => invoke<JobSnapshot[]>("jobs_active"),
  jobsHistory: (limit: number) => invoke<JobSnapshot[]>("jobs_history", { limit }),
  jobFailures: (id: string) => invoke<FailureRecord[]>("job_failures", { id }),
  jobResumeFinished: (id: string) => invoke<JobSnapshot>("job_resume_finished", { id }),
  jobForget: (id: string) => invoke<void>("job_forget", { id }),
  stats: (account: string) => invoke<Stats>("stats", { account }),

  rules: () => invoke<Rule[]>("rules_list"),
  ruleAdd: (draft: RuleDraft) => invoke<Rule>("rule_add", { draft }),
  ruleSetEnabled: (id: string, enabled: boolean) =>
    invoke<Rule>("rule_set_enabled", { id, enabled }),
  ruleDelete: (id: string) => invoke<void>("rule_delete", { id }),
  ruleRun: (id: string) => invoke<JobSnapshot>("rule_run", { id }),

  packagePick: () => invoke<string | null>("package_pick"),
  packageImport: (account: string, path: string) =>
    invoke<PackageIndex>("package_import", { account, path }),
  packageGet: (account: string) => invoke<PackageIndex | null>("package_get", { account }),
  packageForget: (account: string) => invoke<void>("package_forget", { account }),

  botStatus: () => invoke<BotStatus>("bot_status"),
  botSetToken: (token: string) => invoke<BotStatus>("bot_set_token", { token }),
  botClearToken: () => invoke<BotStatus>("bot_clear_token"),
  botConfigure: (replaceForeign: boolean) => invoke<BotStatus>("bot_configure", { replaceForeign }),
  botSetEnabled: (enabled: boolean) => invoke<BotStatus>("bot_set_enabled", { enabled }),
  botSetCommand: (key: string, enabled: boolean) =>
    invoke<BotStatus>("bot_set_command", { key, enabled }),
  demoBotSetup: () => invoke<BotStatus>("demo_bot_setup"),
  demoBotCommand: (subcommand: string) => invoke<void>("demo_bot_command", { subcommand }),

  settings: () => invoke<SettingEntry[]>("settings_all"),
  setSetting: (key: string, value: string) => invoke<void>("setting_set", { key, value }),
  autostartGet: () => invoke<boolean>("autostart_get"),
  autostartSet: (enabled: boolean) => invoke<void>("autostart_set", { enabled }),
  archiveFolderPick: () => invoke<string | null>("archive_folder_pick"),

  openLink: (url: string) => invoke<void>("open_link", { url }),
  openArchive: (path: string) => invoke<void>("open_archive", { path }),
  wipeLocalData: () => invoke<void>("wipe_local_data"),
};

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}
