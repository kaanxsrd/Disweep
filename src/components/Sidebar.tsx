import {
  Bot,
  BrushCleaning,
  CalendarClock,
  ChevronDown,
  FileArchive,
  History,
  LogOut,
  Plus,
  Settings,
} from "lucide-react";
import { type ReactNode, useRef, useState } from "react";
import { toast } from "sonner";
import { Avatar, ProgressBar } from "@/components/ui";
import { api, errorText } from "@/lib/api";
import { cn } from "@/lib/cn";
import { count, percent } from "@/lib/format";
import { type Screen, useApp } from "@/lib/store";
import { LogoMark } from "./Logo";

const items: { screen: Screen; label: string; icon: ReactNode }[] = [
  { screen: "sweep", label: "Sweep", icon: <BrushCleaning /> },
  { screen: "activity", label: "Activity", icon: <History /> },
  { screen: "autosweep", label: "Autosweep", icon: <CalendarClock /> },
  { screen: "package", label: "Data package", icon: <FileArchive /> },
  { screen: "bot", label: "Bot", icon: <Bot /> },
  { screen: "settings", label: "Settings", icon: <Settings /> },
];

export function Sidebar({ onAddAccount }: { onAddAccount: () => void }) {
  const screen = useApp((state) => state.screen);
  const go = useApp((state) => state.go);
  const info = useApp((state) => state.info);
  const bot = useApp((state) => state.bot);
  const online = bot?.connection === "online";

  return (
    <nav className="flex w-[232px] shrink-0 flex-col border-r border-line bg-panel/70 backdrop-blur-sm">
      <div className="flex items-center gap-2.5 px-5 pt-5 pb-5">
        <LogoMark size={30} />
        <span className="text-[18px] font-semibold tracking-tight">Disweep</span>
        {info?.demo && (
          <span className="label ml-auto rounded bg-accent/14 px-1.5 py-0.5 text-accent!">
            Demo
          </span>
        )}
      </div>

      <div className="flex flex-1 flex-col gap-0.5 px-3">
        {items.map((item) => (
          <button
            key={item.screen}
            type="button"
            aria-current={screen === item.screen ? "page" : undefined}
            onClick={() => go(item.screen)}
            className={cn(
              "flex h-9 items-center gap-2.5 rounded-lg px-2.5 text-left transition [&>svg]:size-[17px]",
              screen === item.screen
                ? "bg-raised font-medium text-ink"
                : "text-soft hover:bg-raised/60 hover:text-ink",
            )}
          >
            {item.icon}
            {item.label}
            {item.screen === "bot" && online && (
              <span
                className="ml-auto size-2 rounded-full bg-good"
                role="img"
                aria-label="Online"
              />
            )}
          </button>
        ))}
      </div>

      <ActiveJobCard />
      <AccountMenu onAddAccount={onAddAccount} />
    </nav>
  );
}

function ActiveJobCard() {
  const jobs = useApp((state) => state.jobs);
  const watchJob = useApp((state) => state.watchJob);
  const job = Object.values(jobs).find(
    (candidate) => candidate.state === "running" || candidate.state === "paused",
  );
  if (!job) return null;
  const total = job.totalEstimate ?? 0;
  return (
    <button
      type="button"
      onClick={() => watchJob(job.id)}
      className="mx-3 mb-3 rounded-xl border border-line bg-raised/60 p-3 text-left transition hover:border-faint/60"
    >
      <div className="label">{job.state === "paused" ? "Paused" : "Sweeping"}</div>
      <div className="sensitive mt-1 truncate text-[13px] font-medium">{job.label}</div>
      <div className="mt-2">
        <ProgressBar
          value={percent(
            job.counters.deleted +
              job.counters.alreadyGone +
              job.counters.skipped +
              job.counters.failed,
            total,
          )}
          active={job.state === "running"}
        />
      </div>
      <div className="tabular mt-1.5 text-[12px] text-soft">
        {count(job.counters.deleted)} deleted
      </div>
    </button>
  );
}

function AccountMenu({ onAddAccount }: { onAddAccount: () => void }) {
  const accounts = useApp((state) => state.accounts);
  const accountId = useApp((state) => state.accountId);
  const selectAccount = useApp((state) => state.selectAccount);
  const setAccounts = useApp((state) => state.setAccounts);
  const [open, setOpen] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  const account = accounts.find((candidate) => candidate.id === accountId);
  if (!account) return null;

  const signOut = async () => {
    try {
      await api.removeAccount(account.id);
      setAccounts(await api.accounts());
    } catch (error) {
      toast.error(errorText(error));
    }
    setOpen(false);
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: this only notices focus leaving the menu.
    <div
      ref={container}
      className="relative border-t border-line p-3"
      onBlur={(event) => {
        if (!container.current?.contains(event.relatedTarget)) setOpen(false);
      }}
    >
      {open && (
        <div className="absolute right-3 bottom-[calc(100%-4px)] left-3 z-20 overflow-hidden rounded-xl border border-line bg-panel py-1 shadow-xl">
          {accounts.map((candidate) => (
            <button
              key={candidate.id}
              type="button"
              onClick={() => {
                selectAccount(candidate.id);
                setOpen(false);
              }}
              className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-raised"
            >
              <Avatar name={candidate.displayName} url={candidate.avatarUrl} size={22} />
              <span className="sensitive truncate text-[13px]">{candidate.displayName}</span>
            </button>
          ))}
          <div className="my-1 border-t border-line" />
          <button
            type="button"
            onClick={onAddAccount}
            className="flex w-full items-center gap-2 px-3 py-2 text-left text-[13px] hover:bg-raised"
          >
            <Plus className="size-4" aria-hidden /> Add an account
          </button>
          <button
            type="button"
            onClick={signOut}
            className="flex w-full items-center gap-2 px-3 py-2 text-left text-[13px] text-bad hover:bg-raised"
          >
            <LogOut className="size-4" aria-hidden /> Sign out
          </button>
        </div>
      )}
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-2.5 rounded-lg p-1 text-left transition hover:bg-raised"
      >
        <Avatar name={account.displayName} url={account.avatarUrl} size={30} />
        <div className="min-w-0 flex-1">
          <div className="sensitive truncate text-[13px] leading-tight font-medium">
            {account.displayName}
          </div>
          <div className="sensitive truncate text-[12px] leading-tight text-faint">
            @{account.username}
          </div>
        </div>
        <ChevronDown className="size-4 text-faint" aria-hidden />
      </button>
    </div>
  );
}
