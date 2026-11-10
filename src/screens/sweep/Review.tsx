import { useMutation, useQuery } from "@tanstack/react-query";
import { ArrowLeft, LoaderCircle, Paperclip, TriangleAlert } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Callout, Card, Input, Label, ProgressBar, Stat } from "@/components/ui";
import { api, errorText } from "@/lib/api";
import type { JobSpec } from "@/lib/bindings/JobSpec";
import { toFilters } from "@/lib/filters";
import { count, date, duration, percent } from "@/lib/format";
import { useAccountId, useApp } from "@/lib/store";
import { Steps } from "./Steps";

const CONFIRM_WORD = "SWEEP";

function useSpec(): JobSpec {
  const accountId = useAccountId();
  const targets = useApp((state) => state.targets);
  const draft = useApp((state) => state.draft);
  const options = useApp((state) => state.options);
  // The clock is read once, so the preview and the sweep use the same cutoff.
  return useMemo(
    () => ({
      accountId,
      targets: Object.values(targets),
      filters: toFilters(draft, Date.now()),
      options,
      origin: { type: "app" },
    }),
    [accountId, targets, draft, options],
  );
}

export function ReviewStep() {
  const spec = useSpec();
  const setStep = useApp((state) => state.setStep);
  const watchJob = useApp((state) => state.watchJob);
  const progress = useApp((state) => state.previewProgress);
  const [typed, setTyped] = useState("");
  const previewId = useRef(crypto.randomUUID()).current;

  const preview = useQuery({
    queryKey: ["preview", previewId],
    queryFn: () => api.previewStart(previewId, spec),
    staleTime: Number.POSITIVE_INFINITY,
    gcTime: 0,
  });

  // Development runs effects twice, so a cancel only goes out if the screen really stays closed.
  const cancelTimer = useRef<number | undefined>(undefined);
  useEffect(() => {
    window.clearTimeout(cancelTimer.current);
    return () => {
      cancelTimer.current = window.setTimeout(() => {
        api.previewCancel(previewId).catch(() => {});
      }, 0);
    };
  }, [previewId]);

  const start = useMutation({
    mutationFn: () => {
      const expectations = new Map(
        preview.data?.targets.map((target) => [target.label, target.count]),
      );
      return api.jobStart({
        ...spec,
        targets: spec.targets.map((target) => ({
          ...target,
          expected: expectations.get(target.label) ?? null,
        })),
      });
    },
    onSuccess: (job) => {
      useApp.getState().setJobs([job]);
      watchJob(job.id);
    },
  });

  const back = (
    <Button
      variant="ghost"
      size="lg"
      icon={<ArrowLeft className="size-4" aria-hidden />}
      onClick={() => setStep("choose")}
    >
      Back
    </Button>
  );

  if (preview.isPending) {
    const current = progress?.previewId === previewId ? progress : null;
    return (
      <>
        <Steps current="review" />
        <div className="flex flex-1 flex-col items-center justify-center">
          <LoaderCircle className="mb-4 size-7 animate-spin text-accent" aria-hidden />
          <div className="text-[16px] font-medium">Working out what would be deleted</div>
          <p className="mt-1 text-soft">
            Nothing is being changed.{" "}
            {current && <span className="sensitive">Checking {current.label}.</span>}
          </p>
          {current && current.total > 1 && (
            <div className="mt-4 w-72">
              <ProgressBar value={percent(current.done, current.total)} active />
            </div>
          )}
          <Button className="mt-6" onClick={() => setStep("choose")}>
            Cancel
          </Button>
        </div>
      </>
    );
  }

  if (preview.isError) {
    return (
      <>
        <Steps current="review" />
        <div className="flex flex-1 flex-col items-center justify-center text-center">
          <TriangleAlert className="mb-3 size-6 text-bad" aria-hidden />
          <div className="text-[16px] font-medium">Couldn't preview this sweep</div>
          <p className="mt-1 max-w-md text-soft">{errorText(preview.error)}</p>
          <div className="mt-5 flex gap-2">
            {back}
            <Button size="lg" onClick={() => preview.refetch()}>
              Try again
            </Button>
          </div>
        </div>
      </>
    );
  }

  const result = preview.data;
  const approximate = result.exact ? "" : "about ";
  const unknownCount = result.total === 0 && !result.exact;
  const nothing = result.total === 0 && result.exact;
  const confirmed = typed.trim().toUpperCase() === CONFIRM_WORD;

  return (
    <>
      <div className="mb-4">
        <Steps current="review" />
        <h1 className="mt-1.5 text-[26px] leading-tight font-semibold tracking-tight">
          Review this sweep
        </h1>
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(320px,400px)_minmax(0,1fr)] gap-4">
        <div className="flex min-h-0 flex-col gap-4">
          <Card className="p-5">
            <Label>Will be deleted</Label>
            <div className="tabular gradient-text mt-1 text-[44px] leading-none font-semibold tracking-tight">
              {unknownCount ? "?" : `${result.exact ? "" : "~"}${count(result.total)}`}
            </div>
            <div className="mt-1 text-soft">
              {nothing
                ? "None of your messages match."
                : result.exact
                  ? "of your messages, permanently."
                  : "of your messages, permanently. An estimate from Discord's search."}
            </div>
            <div className="mt-4 grid grid-cols-2 gap-4 border-t border-line pt-4">
              <Stat label="Estimated time" value={duration(result.estimatedMs)} />
              <Stat label="Places" value={count(result.targets.length)} />
            </div>
          </Card>

          <Card className="min-h-0 flex-1 overflow-y-auto p-2">
            {result.targets.map((target, index) => (
              // Labels can repeat across servers, so position keeps these distinct.
              // biome-ignore lint/suspicious/noArrayIndexKey: the list never reorders here.
              <div key={index} className="rounded-lg px-3 py-2">
                <div className="flex items-baseline justify-between gap-3">
                  <span className="sensitive min-w-0 truncate font-medium">{target.label}</span>
                  <span className="tabular shrink-0 text-soft">
                    {target.exact ? "" : "~"}
                    {count(target.count)}
                  </span>
                </div>
                {target.note && <div className="mt-0.5 text-[12px] text-warn">{target.note}</div>}
              </div>
            ))}
          </Card>
        </div>

        <Card className="flex min-h-0 flex-col overflow-hidden">
          <div className="border-b border-line px-4 py-3">
            <Label>A sample of what will go</Label>
          </div>
          <div className="selectable min-h-0 flex-1 overflow-y-auto">
            {result.sample.length === 0 && (
              <div className="p-6 text-center text-soft">No sample available.</div>
            )}
            {result.sample.map((message) => (
              <div key={message.id} className="border-b border-line/60 px-4 py-2.5 last:border-0">
                <div className="flex items-center gap-2 text-[12px] text-faint">
                  <span className="sensitive truncate font-medium text-soft">
                    {message.conversation}
                  </span>
                  <span className="whitespace-nowrap">{date(message.sentAt)}</span>
                </div>
                <div className="sensitive mt-0.5 break-words whitespace-pre-wrap">
                  {message.content || <span className="text-faint">No text</span>}
                  {message.attachments > 0 && (
                    <span className="ml-2 inline-flex items-center gap-1 text-[12px] text-faint">
                      <Paperclip className="size-3" aria-hidden />
                      {message.attachments}
                    </span>
                  )}
                </div>
              </div>
            ))}
          </div>
        </Card>
      </div>

      <div className="mt-4 space-y-3">
        {!nothing && (
          <Callout icon={<TriangleAlert />} tone="bad">
            Deleted messages are gone for good. Discord has no undo, and neither does Disweep.
          </Callout>
        )}
        {start.isError && <div className="text-bad">{errorText(start.error)}</div>}
        <div className="flex items-center justify-between gap-4">
          {back}
          {!nothing && (
            <form
              className="flex items-center gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                if (confirmed) start.mutate();
              }}
            >
              <span className="text-soft">
                Type <b className="font-mono text-ink">{CONFIRM_WORD}</b> to confirm
              </span>
              <Input
                aria-label={`Type ${CONFIRM_WORD} to confirm`}
                className="w-32 font-mono uppercase"
                autoComplete="off"
                spellCheck={false}
                value={typed}
                onChange={(event) => setTyped(event.target.value)}
              />
              <Button
                type="submit"
                variant="danger"
                size="lg"
                disabled={!confirmed}
                loading={start.isPending}
              >
                Delete {approximate}
                {unknownCount ? "matches" : count(result.total)}
              </Button>
            </form>
          )}
        </div>
      </div>
    </>
  );
}
