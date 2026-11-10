import { useApp } from "@/lib/store";
import { ChooseStep } from "./sweep/Choose";
import { ReviewStep } from "./sweep/Review";
import { RunStep } from "./sweep/Run";

export function SweepScreen() {
  const step = useApp((state) => state.step);
  return (
    <div className="flex h-full flex-col px-7 pt-6 pb-5">
      {step === "choose" && <ChooseStep />}
      {step === "review" && <ReviewStep />}
      {step === "run" && <RunStep />}
    </div>
  );
}
