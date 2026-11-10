import { create } from "zustand";
import type { Account } from "./bindings/Account";
import type { AppInfo } from "./bindings/AppInfo";
import type { BotStatus } from "./bindings/BotStatus";
import type { EngineEvent } from "./bindings/EngineEvent";
import type { FeedItem } from "./bindings/FeedItem";
import type { JobOptions } from "./bindings/JobOptions";
import type { JobSnapshot } from "./bindings/JobSnapshot";
import type { Target } from "./bindings/Target";
import { emptyDraft, type FilterDraft } from "./filters";

export type Screen = "sweep" | "activity" | "autosweep" | "package" | "bot" | "settings";
export type SweepStep = "choose" | "review" | "run";

const MAX_FEED = 400;

export const defaultOptions: JobOptions = {
  strategy: "auto",
  thorough: false,
  speed: "balanced",
  limit: null,
  archive: { enabled: false, attachments: false },
};

interface State {
  info: AppInfo | null;
  accounts: Account[];
  accountId: string | null;
  screen: Screen;
  settings: Record<string, string>;
  jobs: Record<string, JobSnapshot>;
  feeds: Record<string, FeedItem[]>;
  previewProgress: { previewId: string; done: number; total: number; label: string } | null;
  bot: BotStatus | null;

  step: SweepStep;
  targets: Record<string, Target>;
  draft: FilterDraft;
  options: JobOptions;
  activeJobId: string | null;

  setInfo: (info: AppInfo) => void;
  setAccounts: (accounts: Account[]) => void;
  selectAccount: (id: string) => void;
  go: (screen: Screen) => void;
  setSettings: (entries: { key: string; value: string }[]) => void;
  setSetting: (key: string, value: string) => void;
  setBot: (bot: BotStatus) => void;
  setJobs: (jobs: JobSnapshot[]) => void;
  applyEngineEvent: (event: EngineEvent) => void;

  setStep: (step: SweepStep) => void;
  toggleTarget: (key: string, target: Target) => void;
  setTargets: (targets: Record<string, Target>) => void;
  clearTargets: () => void;
  updateDraft: (patch: Partial<FilterDraft>) => void;
  updateOptions: (patch: Partial<JobOptions>) => void;
  watchJob: (id: string) => void;
  newSweep: () => void;
}

export const useApp = create<State>((set) => ({
  info: null,
  accounts: [],
  accountId: null,
  screen: "sweep",
  settings: {},
  jobs: {},
  feeds: {},
  previewProgress: null,
  bot: null,

  step: "choose",
  targets: {},
  draft: emptyDraft,
  options: defaultOptions,
  activeJobId: null,

  setInfo: (info) => set({ info }),
  setAccounts: (accounts) =>
    set((state) => ({
      accounts,
      accountId: accounts.some((account) => account.id === state.accountId)
        ? state.accountId
        : (accounts[0]?.id ?? null),
    })),
  selectAccount: (accountId) => set({ accountId, targets: {}, step: "choose", activeJobId: null }),
  go: (screen) => set({ screen }),
  setSettings: (entries) =>
    set({ settings: Object.fromEntries(entries.map((entry) => [entry.key, entry.value])) }),
  setSetting: (key, value) => set((state) => ({ settings: { ...state.settings, [key]: value } })),
  setBot: (bot) => set({ bot }),
  setJobs: (jobs) =>
    set((state) => ({
      jobs: { ...state.jobs, ...Object.fromEntries(jobs.map((job) => [job.id, job])) },
    })),
  applyEngineEvent: (event) =>
    set((state) => {
      if (event.type === "job") {
        return { jobs: { ...state.jobs, [event.job.id]: event.job } };
      }
      if (event.type === "feed") {
        const merged = [...event.items.slice().reverse(), ...(state.feeds[event.jobId] ?? [])];
        return { feeds: { ...state.feeds, [event.jobId]: merged.slice(0, MAX_FEED) } };
      }
      return { previewProgress: event };
    }),

  setStep: (step) => set({ step }),
  toggleTarget: (key, target) =>
    set((state) => {
      const targets = { ...state.targets };
      if (targets[key]) {
        delete targets[key];
      } else {
        targets[key] = target;
      }
      return { targets };
    }),
  setTargets: (targets) => set({ targets }),
  clearTargets: () => set({ targets: {} }),
  updateDraft: (patch) => set((state) => ({ draft: { ...state.draft, ...patch } })),
  updateOptions: (patch) => set((state) => ({ options: { ...state.options, ...patch } })),
  watchJob: (id) => set({ activeJobId: id, step: "run", screen: "sweep" }),
  newSweep: () => set({ step: "choose", targets: {}, activeJobId: null, draft: emptyDraft }),
}));

/** The account everything currently applies to. */
export function useAccountId(): string {
  return useApp((state) => state.accountId) ?? "";
}
