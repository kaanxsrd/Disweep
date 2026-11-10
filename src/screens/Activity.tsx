import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Eye, FolderOpen, History, Play, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Button, Card, EmptyState, Label, Modal, PageHeader, Stat } from "@/components/ui";
import { api, errorText } from "@/lib/api";
import type { DayCount } from "@/lib/bindings/DayCount";
import type { JobSnapshot } from "@/lib/bindings/JobSnapshot";
import { count, date, dateTime, tick } from "@/lib/format";
import { useAccountId, useApp } from "@/lib/store";
import { stateBadge } from "./sweep/Run";

export function ActivityScreen() {
  const account = useAccountId();
  const client = useQueryClient();
  const watchJob = useApp((state) => state.watchJob);
  const liveJobs = useApp((state) => state.jobs);
  const [details, setDetails] = useState<JobSnapshot | null>(null);

  const stats = useQuery({ queryKey: ["stats", account], queryFn: () => api.stats(account) });
  const history = useQuery({ queryKey: ["history"], queryFn: () => api.jobsHistory(200) });

  const finishedCount = Object.values(liveJobs).filter(
    (job) => job.state !== "running" && job.state !== "queued" && job.state !== "paused",
  ).length;
  // biome-ignore lint/correctness/useExhaustiveDependencies: this refetches whenever one finishes.
  useEffect(() => {
    client.invalidateQueries({ queryKey: ["history"] });
    client.invalidateQueries({ queryKey: ["stats"] });
  }, [finishedCount, client]);

  const jobs = (history.data ?? [])
    .filter((job) => job.accountId === account)
    .map((job) => liveJobs[job.id] ?? job);

  const resume = async (job: JobSnapshot) => {
    try {
      const resumed = await api.jobResumeFinished(job.id);
      useApp.getState().setJobs([resumed]);
      watchJob(resumed.id);
    } catch (error) {
      toast.error(errorText(error));
    }
  };

  const forget = async (job: JobSnapshot) => {
    try {
      await api.jobForget(job.id);
      await client.invalidateQueries({ queryKey: ["history"] });
    } catch (error) {
      toast.error(errorText(error));
    }
  };

  return (
    <div className="flex h-full flex-col overflow-y-auto px-7 pt-6 pb-6">
      <PageHeader eyebrow="Activity" title="What you've swept" />

      <div className="grid grid-cols-[minmax(220px,300px)_minmax(0,1fr)] gap-4">
        <Card className="flex flex-col justify-between gap-5 p-5">
          <Stat label="Deleted in total" value={count(stats.data?.totalDeleted ?? 0)} />
          <Stat label="Sweeps finished" value={count(stats.data?.jobsFinished ?? 0)} />
        </Card>
        <Card className="p-5">
          <DeletedByDay days={stats.data?.days ?? []} />
        </Card>
      </div>

      <Card className="mt-4 overflow-hidden">
        <div className="border-b border-line px-5 py-3">
          <Label>Sweeps</Label>
        </div>
        {jobs.length === 0 && (
          <EmptyState icon={<History className="size-5" aria-hidden />} title="No sweeps yet">
            Finished sweeps and anything that was interrupted will be listed here.
          </EmptyState>
        )}
        {jobs.map((job) => {
          const canResume =
            job.state === "interrupted" || job.state === "stopped" || job.state === "failed";
          const active =
            job.state === "running" || job.state === "queued" || job.state === "paused";
          return (
            <div
              key={job.id}
              className="flex items-center gap-4 border-b border-line/60 px-5 py-3 last:border-0"
            >
              <div className="min-w-0 flex-1">
                <div className="sensitive truncate font-medium">{job.label}</div>
                <div className="text-[12px] text-faint">
                  {dateTime(job.createdAt)}
                  {job.origin.type === "bot" && " · from the bot"}
                  {job.origin.type === "rule" && " · from autosweep"}
                </div>
              </div>
              <div className="tabular w-24 text-right">{count(job.counters.deleted)}</div>
              <div className="w-32">{stateBadge(job)}</div>
              <div className="flex w-[116px] justify-end gap-1">
                {active && (
                  <Button size="sm" onClick={() => watchJob(job.id)}>
                    View
                  </Button>
                )}
                {canResume && (
                  <Button
                    size="sm"
                    aria-label="Continue this sweep"
                    onClick={() => resume(job)}
                    icon={<Play className="size-3.5" aria-hidden />}
                  />
                )}
                {!active && (
                  <>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label="Details"
                      onClick={() => setDetails(job)}
                      icon={<Eye className="size-3.5" aria-hidden />}
                    />
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label="Remove from this list"
                      onClick={() => forget(job)}
                      icon={<Trash2 className="size-3.5" aria-hidden />}
                    />
                  </>
                )}
              </div>
            </div>
          );
        })}
      </Card>

      {details && <JobDetails job={details} onClose={() => setDetails(null)} />}
    </div>
  );
}

function JobDetails({ job, onClose }: { job: JobSnapshot; onClose: () => void }) {
  const failures = useQuery({
    queryKey: ["failures", job.id],
    queryFn: () => api.jobFailures(job.id),
  });
  return (
    <Modal title="Sweep details" wide onClose={onClose}>
      <div className="sensitive mb-3 font-medium">{job.label}</div>
      <div className="grid grid-cols-4 gap-4">
        <Stat label="Deleted" value={count(job.counters.deleted)} />
        <Stat label="Already gone" value={count(job.counters.alreadyGone)} />
        <Stat label="Kept" value={count(job.counters.skipped)} />
        <Stat label="Failed" value={count(job.counters.failed)} />
      </div>
      {job.archivePath && (
        <Button
          className="mt-4"
          icon={<FolderOpen className="size-4" aria-hidden />}
          onClick={() =>
            job.archivePath &&
            api.openArchive(job.archivePath).catch((error) => toast.error(errorText(error)))
          }
        >
          Open archive
        </Button>
      )}
      {job.notes.length > 0 && (
        <div className="mt-4">
          <Label className="mb-1.5">Notes</Label>
          <ul className="sensitive space-y-1 text-[13px] text-soft">
            {job.notes.map((note) => (
              <li key={note}>{note}</li>
            ))}
          </ul>
        </div>
      )}
      {(failures.data?.length ?? 0) > 0 && (
        <div className="mt-4">
          <Label className="mb-1.5">Couldn't be deleted</Label>
          <div className="selectable space-y-1.5 text-[13px]">
            {failures.data?.map((failure) => (
              <div key={failure.messageId} className="flex justify-between gap-3">
                <span className="sensitive truncate text-soft">{failure.conversation}</span>
                <span className="text-right text-faint">
                  {failure.reason}
                  {failure.code !== null && ` (${failure.code})`}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
    </Modal>
  );
}

const DAYS_SHOWN = 30;
const CHART_HEIGHT = 132;
const CHART_LEFT = 44;
const CHART_BOTTOM = 22;
const CHART_WIDTH = 720;
const BAR_MAX_WIDTH = 16;

function lastThirtyDays(days: DayCount[], now: Date): DayCount[] {
  const known = new Map(days.map((day) => [day.day, day.deleted]));
  const series: DayCount[] = [];
  for (let back = DAYS_SHOWN - 1; back >= 0; back--) {
    const moment = new Date(
      Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() - back),
    );
    const key = moment.toISOString().slice(0, 10);
    series.push({ day: key, deleted: known.get(key) ?? 0 });
  }
  return series;
}

function niceCeiling(value: number): number {
  const magnitude = 10 ** Math.floor(Math.log10(Math.max(value, 1)));
  for (const step of [1, 1.5, 2, 3, 4, 5, 6, 8, 10]) {
    if (step * magnitude >= value) return step * magnitude;
  }
  return 10 * magnitude;
}

function DeletedByDay({ days }: { days: DayCount[] }) {
  const [hovered, setHovered] = useState<number | null>(null);
  const [asTable, setAsTable] = useState(false);
  const series = useMemo(() => lastThirtyDays(days, new Date()), [days]);
  const activeDays = series.filter((day) => day.deleted > 0);
  const peak = Math.max(...series.map((day) => day.deleted), 1);
  const ceiling = niceCeiling(peak);

  const plotHeight = CHART_HEIGHT - CHART_BOTTOM;
  const plotWidth = CHART_WIDTH - CHART_LEFT;
  const slot = plotWidth / DAYS_SHOWN;
  const barWidth = Math.min(BAR_MAX_WIDTH, slot - 2);
  const y = (value: number) => plotHeight - (value / ceiling) * (plotHeight - 6);

  const header = (
    <div className="mb-3 flex items-center justify-between">
      <Label>Messages deleted per day, last 30 days</Label>
      {activeDays.length >= 2 && (
        <button
          type="button"
          className="text-[12px] text-soft underline-offset-2 hover:text-ink hover:underline"
          onClick={() => setAsTable(!asTable)}
        >
          {asTable ? "Show chart" : "Show table"}
        </button>
      )}
    </div>
  );

  if (activeDays.length < 2) {
    return (
      <>
        {header}
        <div className="flex h-[132px] items-center justify-center text-soft">
          {activeDays.length === 0
            ? "A chart will appear after you've swept on a couple of days."
            : `${count(activeDays[0]?.deleted ?? 0)} deleted on ${date(Date.parse(activeDays[0]?.day ?? ""))}. Sweep on another day to see a trend.`}
        </div>
      </>
    );
  }

  if (asTable) {
    return (
      <>
        {header}
        <div className="selectable h-[132px] overflow-y-auto">
          <table className="w-full text-[13px]">
            <thead className="sr-only">
              <tr>
                <th>Day</th>
                <th>Deleted</th>
              </tr>
            </thead>
            <tbody>
              {activeDays
                .slice()
                .reverse()
                .map((day) => (
                  <tr key={day.day} className="border-b border-line/50 last:border-0">
                    <td className="py-1 text-soft">{date(Date.parse(day.day))}</td>
                    <td className="tabular py-1 text-right">{count(day.deleted)}</td>
                  </tr>
                ))}
            </tbody>
          </table>
        </div>
      </>
    );
  }

  const active = hovered === null ? null : series[hovered];
  const ticks = [0, ceiling / 2, ceiling];

  return (
    <>
      {header}
      <div className="relative">
        <svg
          viewBox={`0 0 ${CHART_WIDTH} ${CHART_HEIGHT}`}
          className="block h-[132px] w-full"
          role="img"
          aria-label={`Bar chart of messages deleted each day. Peak of ${count(peak)}.`}
          onMouseLeave={() => setHovered(null)}
        >
          {ticks.map((value) => (
            <g key={value}>
              <line
                x1={CHART_LEFT}
                x2={CHART_WIDTH}
                y1={y(value)}
                y2={y(value)}
                stroke="var(--line)"
                strokeWidth={1}
                vectorEffect="non-scaling-stroke"
              />
              <text
                x={CHART_LEFT - 8}
                y={y(value) + 4}
                textAnchor="end"
                fontSize={11}
                fill="var(--faint)"
              >
                {tick(value)}
              </text>
            </g>
          ))}
          {series.map((day, index) => {
            const x = CHART_LEFT + index * slot + (slot - barWidth) / 2;
            const top = y(day.deleted);
            const height = plotHeight - top;
            return (
              <g key={day.day}>
                {day.deleted > 0 && (
                  <path
                    d={`M${x} ${plotHeight}V${top + 4}Q${x} ${top} ${x + 4} ${top}H${x + barWidth - 4}Q${x + barWidth} ${top} ${x + barWidth} ${top + 4}V${plotHeight}Z`}
                    fill="var(--chart)"
                    opacity={hovered === null || hovered === index ? 1 : 0.45}
                    style={{ height }}
                  />
                )}
                {}
                {/* biome-ignore lint/a11y/noStaticElementInteractions: the table view is the keyboard path. */}
                <rect
                  x={CHART_LEFT + index * slot}
                  y={0}
                  width={slot}
                  height={plotHeight}
                  fill="transparent"
                  onMouseEnter={() => setHovered(index)}
                />
              </g>
            );
          })}
          {[0, 10, 20, 29].map((index) => {
            const day = series[index];
            if (!day) return null;

            const anchor = index === 0 ? "start" : index === 29 ? "end" : "middle";
            const x =
              index === 0
                ? CHART_LEFT
                : index === 29
                  ? CHART_WIDTH
                  : CHART_LEFT + index * slot + slot / 2;
            return (
              <text
                key={day.day}
                x={x}
                y={CHART_HEIGHT - 6}
                textAnchor={anchor}
                fontSize={11}
                fill="var(--faint)"
              >
                {new Date(Date.parse(day.day)).toLocaleDateString(undefined, {
                  month: "short",
                  day: "numeric",
                  timeZone: "UTC",
                })}
              </text>
            );
          })}
        </svg>
        {active && hovered !== null && (
          <div
            className="pointer-events-none absolute top-0 z-10 -translate-x-1/2 rounded-lg border border-line bg-panel px-2.5 py-1.5 text-[12px] shadow-lg"
            style={{
              left: `${Math.min(88, Math.max(12, ((CHART_LEFT + (hovered + 0.5) * slot) / CHART_WIDTH) * 100))}%`,
            }}
          >
            <div className="text-soft">{date(Date.parse(active.day))}</div>
            <div className="tabular font-medium">{count(active.deleted)} deleted</div>
          </div>
        )}
      </div>
    </>
  );
}
