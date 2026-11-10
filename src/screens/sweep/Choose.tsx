import { ArrowRight, ChevronDown } from "lucide-react";
import { useEffect, useState } from "react";
import { Button, Card, Chip, Field, Input, Label, Segmented, Toggle } from "@/components/ui";
import { api } from "@/lib/api";
import type { HasKind } from "@/lib/bindings/HasKind";
import type { Speed } from "@/lib/bindings/Speed";
import type { Strategy } from "@/lib/bindings/Strategy";
import { cn } from "@/lib/cn";
import { draftProblem, type TimeUnit, type WhenMode } from "@/lib/filters";
import { useApp } from "@/lib/store";
import { Steps } from "./Steps";
import { TargetPicker } from "./TargetPicker";

const hasOptions: { value: HasKind; label: string }[] = [
  { value: "link", label: "Link" },
  { value: "file", label: "File" },
  { value: "image", label: "Image" },
  { value: "video", label: "Video" },
  { value: "sound", label: "Sound" },
  { value: "embed", label: "Embed" },
  { value: "sticker", label: "Sticker" },
  { value: "poll", label: "Poll" },
];

const units: { value: TimeUnit; label: string }[] = [
  { value: "hours", label: "hours" },
  { value: "days", label: "days" },
  { value: "weeks", label: "weeks" },
  { value: "months", label: "months" },
  { value: "years", label: "years" },
];

export function ChooseStep() {
  const targets = useApp((state) => state.targets);
  const toggleTarget = useApp((state) => state.toggleTarget);
  const setTargets = useApp((state) => state.setTargets);
  const setStep = useApp((state) => state.setStep);
  const draft = useApp((state) => state.draft);
  const [patternProblem, setPatternProblem] = useState<string | null>(null);

  const selected = Object.keys(targets).length;
  const problem = draftProblem(draft) ?? patternProblem;

  return (
    <>
      <div className="mb-4 flex items-center justify-between">
        <div>
          <Steps current="choose" />
          <h1 className="mt-1.5 text-[26px] leading-tight font-semibold tracking-tight">
            What should be swept?
          </h1>
        </div>
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_minmax(340px,410px)] gap-4">
        <TargetPicker targets={targets} onToggle={toggleTarget} onReplace={setTargets} />
        <FilterPanel onPatternProblem={setPatternProblem} patternProblem={patternProblem} />
      </div>

      <div className="mt-4 flex items-center justify-between gap-4">
        <div className="min-w-0 text-soft">
          {problem ? (
            <span className="text-bad">{problem}</span>
          ) : selected === 0 ? (
            "Choose at least one conversation."
          ) : (
            <span>
              <span className="tabular font-medium text-ink">{selected}</span>{" "}
              {selected === 1 ? "place" : "places"} chosen. Nothing is deleted on the next step.
            </span>
          )}
        </div>
        <Button
          variant="primary"
          size="lg"
          disabled={selected === 0 || problem !== null}
          onClick={() => setStep("review")}
        >
          Preview <ArrowRight className="size-4" aria-hidden />
        </Button>
      </div>
    </>
  );
}

function FilterPanel({
  patternProblem,
  onPatternProblem,
}: {
  patternProblem: string | null;
  onPatternProblem: (problem: string | null) => void;
}) {
  const draft = useApp((state) => state.draft);
  const update = useApp((state) => state.updateDraft);
  const options = useApp((state) => state.options);
  const updateOptions = useApp((state) => state.updateOptions);
  const [advanced, setAdvanced] = useState(false);

  useEffect(() => {
    if (draft.pattern.trim() === "") {
      onPatternProblem(null);
      return;
    }
    const timer = setTimeout(() => {
      api
        .checkPattern(draft.pattern)
        .then(onPatternProblem)
        .catch(() => {});
    }, 200);
    return () => clearTimeout(timer);
  }, [draft.pattern, onPatternProblem]);

  const toggleHas = (kind: HasKind) =>
    update({
      has: draft.has.includes(kind)
        ? draft.has.filter((item) => item !== kind)
        : [...draft.has, kind],
    });

  return (
    <Card className="min-h-0 space-y-4 overflow-y-auto p-4">
      <div>
        <Label className="mb-2">When</Label>
        <Segmented<WhenMode>
          label="When"
          value={draft.when}
          onChange={(when) => update({ when })}
          options={[
            { value: "any", label: "Any time" },
            { value: "older", label: "Older than" },
            { value: "newer", label: "Newer than" },
            { value: "between", label: "Between" },
          ]}
        />
        {(draft.when === "older" || draft.when === "newer") && (
          <div className="mt-2 flex gap-2">
            <Input
              type="number"
              min={1}
              aria-label="Amount"
              className="w-24"
              value={Number.isNaN(draft.amount) ? "" : draft.amount}
              onChange={(event) => update({ amount: event.target.valueAsNumber })}
            />
            <select
              aria-label="Unit"
              value={draft.unit}
              onChange={(event) => update({ unit: event.target.value as TimeUnit })}
              className="h-9 flex-1 rounded-lg border border-line bg-canvas/60 px-2 text-sm"
            >
              {units.map((unit) => (
                <option key={unit.value} value={unit.value}>
                  {unit.label}
                </option>
              ))}
            </select>
          </div>
        )}
        {draft.when === "between" && (
          <div className="mt-2 grid grid-cols-2 gap-2">
            <Field label="From">
              <Input
                type="date"
                value={draft.from}
                onChange={(event) => update({ from: event.target.value })}
              />
            </Field>
            <Field label="To">
              <Input
                type="date"
                value={draft.to}
                onChange={(event) => update({ to: event.target.value })}
              />
            </Field>
          </div>
        )}
      </div>

      <div className="space-y-3">
        <Field label="Containing">
          <Input
            placeholder="Any text"
            value={draft.contains}
            onChange={(event) => update({ contains: event.target.value })}
          />
        </Field>
        <Field label="Keep messages containing" hint="Separate words with commas.">
          <Input
            placeholder="invoice, receipt"
            value={draft.keep}
            onChange={(event) => update({ keep: event.target.value })}
          />
        </Field>
      </div>

      <div>
        <Label className="mb-2">With any of</Label>
        <div className="flex flex-wrap gap-1.5">
          {hasOptions.map((option) => (
            <Chip
              key={option.value}
              active={draft.has.includes(option.value)}
              onClick={() => toggleHas(option.value)}
            >
              {option.label}
            </Chip>
          ))}
        </div>
      </div>

      <div className="space-y-3 border-t border-line pt-4">
        <Toggle
          label="Save a copy first"
          description="Keeps a readable transcript on this computer."
          checked={options.archive.enabled}
          onChange={(enabled) =>
            updateOptions({
              archive: { enabled, attachments: enabled && options.archive.attachments },
            })
          }
        />
        {options.archive.enabled && (
          <Toggle
            label="Include attachments"
            description="Downloads files and images too, which takes longer."
            checked={options.archive.attachments}
            onChange={(attachments) => updateOptions({ archive: { enabled: true, attachments } })}
          />
        )}
        <Toggle
          label="Include pinned messages"
          checked={draft.includePinned}
          onChange={(includePinned) => update({ includePinned })}
        />
      </div>

      <div className="border-t border-line pt-1">
        <button
          type="button"
          aria-expanded={advanced}
          onClick={() => setAdvanced(!advanced)}
          className="flex w-full items-center justify-between py-2 text-left font-medium"
        >
          Advanced
          <ChevronDown className={cn("size-4 transition", advanced && "rotate-180")} aria-hidden />
        </button>
        {advanced && (
          <div className="space-y-3 pt-1">
            <Field label="Regular expression" error={patternProblem} hint="Ignores case.">
              <Input
                className="font-mono"
                spellCheck={false}
                placeholder="^order #\d+$"
                value={draft.pattern}
                onChange={(event) => update({ pattern: event.target.value })}
              />
            </Field>
            <div className="grid grid-cols-2 gap-2">
              <Field label="From message ID">
                <Input
                  className="font-mono"
                  placeholder="This and newer"
                  value={draft.afterMessage}
                  onChange={(event) => update({ afterMessage: event.target.value })}
                />
              </Field>
              <Field label="To message ID">
                <Input
                  className="font-mono"
                  placeholder="This and older"
                  value={draft.beforeMessage}
                  onChange={(event) => update({ beforeMessage: event.target.value })}
                />
              </Field>
            </div>
            <Field label="Stop after" hint="Leave empty to sweep everything that matches.">
              <Input
                type="number"
                min={1}
                placeholder="No limit"
                value={options.limit ?? ""}
                onChange={(event) =>
                  updateOptions({
                    limit: event.target.value === "" ? null : event.target.valueAsNumber,
                  })
                }
              />
            </Field>
            <div>
              <div className="mb-1.5 text-[13px] font-medium text-soft">How to find messages</div>
              <Segmented<Strategy>
                label="How to find messages"
                value={options.strategy}
                onChange={(strategy) => updateOptions({ strategy })}
                options={[
                  { value: "auto", label: "Automatic" },
                  { value: "search", label: "Search" },
                  { value: "history", label: "Full history" },
                ]}
              />
            </div>
            <Toggle
              label="Thorough"
              description="After searching, also read the full history to catch anything search missed. Slow in busy channels."
              checked={options.thorough}
              onChange={(thorough) => updateOptions({ thorough })}
            />
            <div>
              <div className="mb-1.5 text-[13px] font-medium text-soft">Speed</div>
              <Segmented<Speed>
                label="Speed"
                value={options.speed}
                onChange={(speed) => updateOptions({ speed })}
                options={[
                  { value: "careful", label: "Careful" },
                  { value: "balanced", label: "Balanced" },
                  { value: "fast", label: "Fast" },
                ]}
              />
            </div>
            <Toggle
              label="Include age-restricted channels"
              checked={draft.includeNsfw}
              onChange={(includeNsfw) => update({ includeNsfw })}
            />
          </div>
        )}
      </div>
    </Card>
  );
}
