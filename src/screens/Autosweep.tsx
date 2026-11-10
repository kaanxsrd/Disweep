import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CalendarClock, Play, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button, Card, EmptyState, Field, Input, Modal, PageHeader, Switch } from "@/components/ui";
import { api, errorText } from "@/lib/api";
import type { Rule } from "@/lib/bindings/Rule";
import type { Target } from "@/lib/bindings/Target";
import { emptyDraft, type TimeUnit, toFilters, unitMilliseconds } from "@/lib/filters";
import { ago, interval } from "@/lib/format";
import { useAccountId, useApp } from "@/lib/store";
import { TargetPicker } from "./sweep/TargetPicker";

export function AutosweepScreen() {
  const [adding, setAdding] = useState(false);
  const client = useQueryClient();
  const account = useAccountId();
  const watchJob = useApp((state) => state.watchJob);
  const rules = useQuery({ queryKey: ["rules"], queryFn: api.rules });
  const mine = (rules.data ?? []).filter((rule) => rule.accountId === account);

  const refresh = () => client.invalidateQueries({ queryKey: ["rules"] });
  const act = async (action: Promise<unknown>) => {
    try {
      await action;
    } catch (error) {
      toast.error(errorText(error));
    }
    await refresh();
  };

  const runNow = async (rule: Rule) => {
    try {
      const job = await api.ruleRun(rule.id);
      useApp.getState().setJobs([job]);
      watchJob(job.id);
      await refresh();
    } catch (error) {
      toast.error(errorText(error));
    }
  };

  return (
    <div className="flex h-full flex-col overflow-y-auto px-7 pt-6 pb-6">
      <PageHeader
        eyebrow="Autosweep"
        title="Keep chats clean automatically"
        description="Rules delete your messages once they pass a certain age. They run while Disweep is open or in your tray."
        actions={
          <Button
            variant="primary"
            icon={<Plus className="size-4" aria-hidden />}
            onClick={() => setAdding(true)}
          >
            New rule
          </Button>
        }
      />

      <Card className="overflow-hidden">
        {mine.length === 0 && (
          <EmptyState icon={<CalendarClock className="size-5" aria-hidden />} title="No rules yet">
            Add one to keep a conversation tidy, like deleting your messages after 30 days.
          </EmptyState>
        )}
        {mine.map((rule) => (
          <div
            key={rule.id}
            className="flex items-center gap-4 border-b border-line/60 px-5 py-3.5 last:border-0"
          >
            <Switch
              label={`Turn ${rule.label} on or off`}
              checked={rule.enabled}
              onChange={(enabled) => act(api.ruleSetEnabled(rule.id, enabled))}
            />
            <div className="min-w-0 flex-1">
              <div className="sensitive truncate font-medium">{rule.label}</div>
              <div className="text-[13px] text-soft">
                Deletes your messages older than {interval(rule.olderThanSecs)}, checked every{" "}
                {interval(rule.everySecs)}
              </div>
            </div>
            <div className="text-right text-[12px] text-faint">
              {rule.lastRunAt ? `Last ran ${ago(rule.lastRunAt)}` : "Hasn't run yet"}
            </div>
            <Button
              size="sm"
              icon={<Play className="size-3.5" aria-hidden />}
              onClick={() => runNow(rule)}
            >
              Run now
            </Button>
            <Button
              size="sm"
              variant="ghost"
              aria-label="Delete rule"
              icon={<Trash2 className="size-3.5" aria-hidden />}
              onClick={() => act(api.ruleDelete(rule.id))}
            />
          </div>
        ))}
      </Card>

      {adding && <NewRule onClose={() => setAdding(false)} onCreated={refresh} />}
    </div>
  );
}

const intervals = [
  { label: "Every hour", seconds: 3_600 },
  { label: "Every 6 hours", seconds: 21_600 },
  { label: "Every day", seconds: 86_400 },
  { label: "Every week", seconds: 604_800 },
];

function NewRule({ onClose, onCreated }: { onClose: () => void; onCreated: () => void }) {
  const account = useAccountId();
  const [targets, setTargets] = useState<Record<string, Target>>({});
  const [amount, setAmount] = useState(30);
  const [unit, setUnit] = useState<TimeUnit>("days");
  const [every, setEvery] = useState(86_400);

  const create = useMutation({
    mutationFn: () => {
      const list = Object.values(targets);
      const first = list[0]?.label ?? "Rule";
      return api.ruleAdd({
        accountId: account,
        label: list.length > 1 ? `${first} and ${list.length - 1} more` : first,
        targets: list,
        olderThanSecs: Math.round((amount * unitMilliseconds(unit)) / 1000),
        everySecs: every,
        filters: toFilters(emptyDraft, Date.now()),
      });
    },
    onSuccess: () => {
      onCreated();
      onClose();
    },
  });

  const toggle = (key: string, target: Target) => {
    const next = { ...targets };
    if (next[key]) delete next[key];
    else next[key] = target;
    setTargets(next);
  };

  const valid = Object.keys(targets).length > 0 && Number.isFinite(amount) && amount > 0;

  return (
    <Modal
      title="New rule"
      wide
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={!valid}
            loading={create.isPending}
            onClick={() => create.mutate()}
          >
            Turn on
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-3">
        <Field label="Delete my messages older than">
          <div className="flex gap-2">
            <Input
              type="number"
              min={1}
              aria-label="Amount"
              className="w-20"
              value={Number.isNaN(amount) ? "" : amount}
              onChange={(event) => setAmount(event.target.valueAsNumber)}
            />
            <select
              aria-label="Unit"
              value={unit}
              onChange={(event) => setUnit(event.target.value as TimeUnit)}
              className="h-9 flex-1 rounded-lg border border-line bg-canvas/60 px-2 text-sm"
            >
              <option value="hours">hours</option>
              <option value="days">days</option>
              <option value="weeks">weeks</option>
              <option value="months">months</option>
              <option value="years">years</option>
            </select>
          </div>
        </Field>
        <Field label="Check">
          <select
            value={every}
            onChange={(event) => setEvery(Number(event.target.value))}
            className="h-9 w-full rounded-lg border border-line bg-canvas/60 px-2 text-sm"
          >
            {intervals.map((option) => (
              <option key={option.seconds} value={option.seconds}>
                {option.label}
              </option>
            ))}
          </select>
        </Field>
      </div>
      <div className="mt-3 grid h-[300px]">
        <TargetPicker targets={targets} onToggle={toggle} onReplace={setTargets} />
      </div>
      {create.isError && <div className="mt-2 text-bad">{errorText(create.error)}</div>}
    </Modal>
  );
}
