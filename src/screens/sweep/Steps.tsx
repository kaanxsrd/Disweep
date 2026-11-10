import { cn } from "@/lib/cn";
import type { SweepStep } from "@/lib/store";

const steps: { key: SweepStep; label: string }[] = [
  { key: "choose", label: "Choose" },
  { key: "review", label: "Review" },
  { key: "run", label: "Sweep" },
];

export function Steps({ current }: { current: SweepStep }) {
  const position = steps.findIndex((step) => step.key === current);
  return (
    <ol className="flex items-center gap-3 font-mono text-[12px] tracking-[0.1em] uppercase">
      {steps.map((step, index) => (
        <li key={step.key} className="flex items-center gap-3">
          {index > 0 && <span className="text-faint/60">/</span>}
          <span
            aria-current={index === position ? "step" : undefined}
            className={cn(index === position ? "text-accent" : "text-faint")}
          >
            0{index + 1} {step.label}
          </span>
        </li>
      ))}
    </ol>
  );
}
