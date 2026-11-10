import { useVirtualizer } from "@tanstack/react-virtual";
import {
  CircleCheck,
  FolderOpen,
  Hourglass,
  Paperclip,
  Pause,
  Play,
  Plus,
  Square,
  TriangleAlert,
} from "lucide-react";
import { useRef } from "react";
import { toast } from "sonner";
import { Badge, Button, Callout, Card, Label, ProgressBar, Stat } from "@/components/ui";
import { api, errorText } from "@/lib/api";
import type { FeedItem } from "@/lib/bindings/FeedItem";
import type { JobSnapshot } from "@/lib/bindings/JobSnapshot";
import { cn } from "@/lib/cn";
import { count, date, duration, percent } from "@/lib/format";
import { useApp } from "@/lib/store";
import { Steps } from "./Steps";

const EMPTY_FEED: FeedItem[] = [];

export function processed(job: JobSnapshot): number {
  const { deleted, alreadyGone, skipped, failed } = job.counters;
  return deleted + alreadyGone + skipped + failed;
}

export function stateBadge(job: JobSnapshot) {
  switch (job.state) {
    case "queued":
      return <Badge>Waiting its turn</Badge>;
    case "running":
      return job.rateLimitedUntil ? (
        <Badge tone="warn">Waiting for Discord</Badge>
      ) : (
        <Badge tone="accent">Sweeping</Badge>
      );
    case "paused":
      return <Badge tone="warn">Paused</Badge>;
    case "completed":
      return <Badge tone="good">Finished</Badge>;
    case "stopped":
      return <Badge tone="warn">Stopped</Badge>;
    case "failed":
      return <Badge tone="bad">Failed</Badge>;
    case "interrupted":
      return <Badge tone="warn">Interrupted</Badge>;
  }
}

function run(action: Promise<unknown>) {
  action.catch((error) => toast.error(errorText(error)));
}

export function RunStep() {
  const jobId = useApp((state) => state.activeJobId);
  const job = useApp((state) => (jobId ? state.jobs[jobId] : undefined));
  const feed = useApp((state) => (jobId ? state.feeds[jobId] : undefined)) ?? EMPTY_FEED;
  const newSweep = useApp((state) => state.newSweep);

  if (!job) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center text-soft">
        This sweep is no longer available.
        <Button className="mt-4" onClick={newSweep}>
          Start a new sweep
        </Button>
      </div>
    );
  }

  const active = job.state === "running" || job.state === "queued" || job.state === "paused";
  const total = job.totalEstimate ?? 0;
  const done = processed(job);
  const progress = job.state === "completed" ? 100 : percent(done, total);
  const resumable =
    job.state === "failed" || job.state === "stopped" || job.state === "interrupted";

  return (
    <>
      <div className="mb-4 flex items-end justify-between gap-6">
        <div className="min-w-0">
          <Steps current="run" />
          <h1 className="sensitive mt-1.5 truncate text-[26px] leading-tight font-semibold tracking-tight">
            {job.label}
          </h1>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {job.state === "running" && (
            <Button
              icon={<Pause className="size-4" aria-hidden />}
              onClick={() => run(api.jobPause(job.id))}
            >
              Pause
            </Button>
          )}
          {job.state === "paused" && (
            <Button
              variant="primary"
              icon={<Play className="size-4" aria-hidden />}
              onClick={() => run(api.jobResume(job.id))}
            >
              Resume
            </Button>
          )}
          {active && (
            <Button
              icon={<Square className="size-3.5" aria-hidden />}
              onClick={() => run(api.jobStop(job.id))}
            >
              Stop
            </Button>
          )}
          {resumable && (
            <Button
              variant="primary"
              icon={<Play className="size-4" aria-hidden />}
              onClick={() => run(api.jobResumeFinished(job.id))}
            >
              Continue
            </Button>
          )}
          {job.archivePath && !active && (
            <Button
              icon={<FolderOpen className="size-4" aria-hidden />}
              onClick={() => job.archivePath && run(api.openArchive(job.archivePath))}
            >
              Open archive
            </Button>
          )}
          {!active && (
            <Button
              variant={resumable ? "secondary" : "primary"}
              icon={<Plus className="size-4" aria-hidden />}
              onClick={newSweep}
            >
              New sweep
            </Button>
          )}
        </div>
      </div>

      <Card className="p-5">
        <div className="mb-2 flex items-center justify-between">
          <div className="flex items-center gap-2">
            {stateBadge(job)}
            {job.pacingMs > 0 && active && (
              <span className="text-[12px] text-faint">
                One message every {(job.pacingMs / 1000).toFixed(1)}s
              </span>
            )}
          </div>
          <span className="tabular font-mono text-good">{progress}%</span>
        </div>
        <ProgressBar value={progress} active={job.state === "running"} />
        <div className="mt-5 grid grid-cols-5 gap-4">
          <Stat label="Deleted" value={count(job.counters.deleted)} />
          <Stat label="Kept" value={count(job.counters.skipped)} />
          <Stat label="Failed" value={count(job.counters.failed)} />
          <Stat
            label="Per minute"
            value={job.ratePerMinute > 0 ? count(Math.round(job.ratePerMinute)) : "–"}
          />
          <Stat
            label="Time left"
            value={job.state === "running" && job.etaMs !== null ? duration(job.etaMs) : "–"}
          />
        </div>
      </Card>

      <div className="mt-3 space-y-2 empty:hidden">
        {job.failure && (
          <Callout tone="bad" icon={<TriangleAlert />}>
            {job.failure.message}
          </Callout>
        )}
        {job.state === "completed" && (
          <Callout tone="accent" icon={<CircleCheck />}>
            Finished. {count(job.counters.deleted)} messages deleted
            {job.counters.alreadyGone > 0 &&
              `, ${count(job.counters.alreadyGone)} were already gone`}
            .
          </Callout>
        )}
        {job.notes.slice(0, 3).map((note) => (
          <Callout key={note} icon={<TriangleAlert />}>
            <span className="sensitive">{note}</span>
          </Callout>
        ))}
      </div>

      <div className="mt-4 grid min-h-0 flex-1 grid-cols-[minmax(250px,320px)_minmax(0,1fr)] gap-4">
        <Card className="min-h-0 overflow-y-auto p-2">
          <Label className="px-2.5 pt-1.5 pb-2">Places</Label>
          {job.targets.map((target, index) => (
            <div
              // biome-ignore lint/suspicious/noArrayIndexKey: targets keep their order for a job's life.
              key={index}
              className={cn(
                "rounded-lg px-2.5 py-2",
                job.currentTarget === index && "bg-raised/70",
              )}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="sensitive min-w-0 truncate font-medium">{target.label}</span>
                {target.state === "done" && (
                  <CircleCheck className="size-4 shrink-0 text-good" aria-label="Done" />
                )}
                {target.state === "active" && (
                  <Hourglass className="size-3.5 shrink-0 text-accent" aria-label="In progress" />
                )}
                {target.state === "skipped" && <Badge tone="warn">Skipped</Badge>}
              </div>
              <div className="tabular mt-0.5 text-[12px] text-faint">
                {count(target.deleted)} deleted
              </div>
              {target.note && <div className="mt-0.5 text-[12px] text-warn">{target.note}</div>}
            </div>
          ))}
        </Card>

        <Feed items={feed} />
      </div>
    </>
  );
}

const outcomeStyle = {
  deleted: { label: "Deleted", className: "text-good" },
  alreadyGone: { label: "Gone", className: "text-faint" },
  skipped: { label: "Kept", className: "text-warn" },
  failed: { label: "Failed", className: "text-bad" },
};

function Feed({ items }: { items: FeedItem[] }) {
  const scroller = useRef<HTMLDivElement>(null);
  const rows = useVirtualizer({
    count: items.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 56,
    overscan: 10,
    getItemKey: (index) => items[index]?.messageId ?? index,
  });

  return (
    <Card className="flex min-h-0 flex-col overflow-hidden">
      <div className="border-b border-line px-4 py-3">
        <Label>Live activity</Label>
      </div>
      <div ref={scroller} className="selectable min-h-0 flex-1 overflow-y-auto">
        {items.length === 0 && (
          <div className="p-8 text-center text-soft">Activity will appear here.</div>
        )}
        <div className="relative" style={{ height: rows.getTotalSize() }}>
          {rows.getVirtualItems().map((row) => {
            const item = items[row.index];
            if (!item) return null;
            const style = outcomeStyle[item.outcome];
            return (
              <div
                key={row.key}
                className="absolute inset-x-0 border-b border-line/50 px-4 py-2"
                style={{ height: row.size, transform: `translateY(${row.start}px)` }}
              >
                <div className="flex items-center gap-2 text-[12px]">
                  <span className={cn("font-mono font-medium", style.className)}>
                    {style.label}
                  </span>
                  <span className="sensitive min-w-0 truncate text-soft">{item.conversation}</span>
                  <span className="whitespace-nowrap text-faint">{date(item.sentAt)}</span>
                  {item.note && <span className="truncate text-faint">{item.note}</span>}
                </div>
                <div className="sensitive truncate text-[13px]">
                  {item.snippet || <span className="text-faint">No text</span>}
                  {item.attachments > 0 && (
                    <span className="ml-2 inline-flex items-center gap-1 text-[12px] text-faint">
                      <Paperclip className="size-3" aria-hidden />
                      {item.attachments}
                    </span>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </Card>
  );
}
