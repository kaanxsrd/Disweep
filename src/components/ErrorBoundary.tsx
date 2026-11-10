import { ExternalLink, RotateCw, TriangleAlert } from "lucide-react";
import { Component, type ReactNode } from "react";
import { LogoMark } from "@/components/Logo";
import { Button, Card } from "@/components/ui";
import { api } from "@/lib/api";

const NEW_ISSUE = "https://github.com/kaanxsrd/Disweep/issues/new";

export type CrashScope = "screen" | "window";

const wording: Record<CrashScope, { title: string; next: string }> = {
  screen: { title: "This screen hit a problem", next: "Reload, or choose another screen." },
  window: { title: "Disweep hit a problem", next: "Reload to get back to them." },
};

export function describeError(error: unknown): string {
  if (error instanceof Error) return error.message ? `${error.name}: ${error.message}` : error.name;
  if (typeof error === "string" && error !== "") return error;
  return "Something went wrong.";
}

export function Crashed({ error, scope }: { error: unknown; scope: CrashScope }) {
  return (
    <div role="alert" className="flex h-full items-center justify-center p-8">
      <Card className="rise w-full max-w-lg p-6">
        {scope === "window" ? (
          <LogoMark size={36} />
        ) : (
          <TriangleAlert className="size-6 text-warn" aria-hidden />
        )}
        <h1 className="mt-3 text-[19px] font-semibold">{wording[scope].title}</h1>
        <p className="mt-1 text-soft">
          Sweeps, Autosweep rules and the bot keep running. {wording[scope].next}
        </p>
        <pre className="selectable mt-4 max-h-40 overflow-auto rounded-lg border border-line bg-canvas/60 p-3 font-mono text-[12px] break-words whitespace-pre-wrap text-soft">
          {describeError(error)}
        </pre>
        <p className="mt-2 text-[12px] text-faint">
          If it keeps happening, include this text when you report it.
        </p>
        <div className="mt-4 flex gap-2">
          <Button
            variant="primary"
            icon={<RotateCw className="size-4" aria-hidden />}
            onClick={() => window.location.reload()}
          >
            Reload
          </Button>
          <Button
            variant="ghost"
            icon={<ExternalLink className="size-4" aria-hidden />}
            onClick={() => api.openLink(NEW_ISSUE).catch(() => {})}
          >
            Report it
          </Button>
        </div>
      </Card>
    </div>
  );
}

interface BoundaryState {
  failed: boolean;
  error: unknown;
}

export class ErrorBoundary extends Component<
  { scope: CrashScope; children: ReactNode },
  BoundaryState
> {
  override state: BoundaryState = { failed: false, error: null };

  static getDerivedStateFromError(error: unknown): BoundaryState {
    return { failed: true, error };
  }

  override render() {
    if (this.state.failed) return <Crashed error={this.state.error} scope={this.props.scope} />;
    return this.props.children;
  }
}
