import { listen } from "@tauri-apps/api/event";
import { KeyRound, LogIn, Play, ShieldCheck, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { LogoMark } from "@/components/Logo";
import { Button, Callout, Card, Field, Input, Label } from "@/components/ui";
import { api, errorText } from "@/lib/api";
import type { Account } from "@/lib/bindings/Account";
import type { LoginEvent } from "@/lib/bindings/LoginEvent";
import { useApp } from "@/lib/store";

/** Picks up the result of the Discord sign-in window. */
export function useLoginEvents(onSuccess: () => void) {
  useEffect(() => {
    const pending = listen<LoginEvent>("login", async ({ payload }) => {
      if (payload.type === "failure") {
        toast.error(payload.message);
        return;
      }
      useApp.getState().setAccounts(await api.accounts());
      useApp.getState().selectAccount(payload.account.id);
      onSuccess();
    });
    return () => {
      pending.then((stop) => stop());
    };
  }, [onSuccess]);
}

export function SignInOptions({ onDone }: { onDone?: () => void }) {
  const demo = useApp((state) => state.info?.demo);
  const setAccounts = useApp((state) => state.setAccounts);
  const selectAccount = useApp((state) => state.selectAccount);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const finish = async (account: Account) => {
    setAccounts(await api.accounts());
    selectAccount(account.id);
    onDone?.();
  };

  const submit = async () => {
    setBusy(true);
    setProblem(null);
    try {
      await finish(await api.addToken(token));
      setToken("");
    } catch (error) {
      setProblem(errorText(error));
    } finally {
      setBusy(false);
    }
  };

  if (demo) {
    return (
      <Button
        variant="primary"
        size="lg"
        className="w-full"
        loading={busy}
        icon={<Play className="size-4" aria-hidden />}
        onClick={async () => {
          setBusy(true);
          try {
            await finish(await api.demoSignIn());
          } catch (error) {
            setProblem(errorText(error));
          } finally {
            setBusy(false);
          }
        }}
      >
        Enter the demo
      </Button>
    );
  }

  return (
    <div className="space-y-4">
      <Button
        variant="primary"
        size="lg"
        className="w-full"
        icon={<LogIn className="size-4" aria-hidden />}
        onClick={() => api.openLogin().catch((error) => toast.error(errorText(error)))}
      >
        Sign in with Discord
      </Button>
      <div className="flex items-center gap-3 text-[12px] text-faint">
        <span className="h-px flex-1 bg-line" />
        or use a token
        <span className="h-px flex-1 bg-line" />
      </div>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
        className="space-y-2"
      >
        <Field label="Account token" error={problem}>
          <Input
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={token}
            onChange={(event) => setToken(event.target.value)}
            placeholder="Paste your token"
          />
        </Field>
        <Button
          type="submit"
          className="w-full"
          loading={busy}
          disabled={token.trim() === ""}
          icon={<KeyRound className="size-4" aria-hidden />}
        >
          Continue
        </Button>
      </form>
    </div>
  );
}

export function SignInScreen() {
  const demo = useApp((state) => state.info?.demo);
  const settings = useApp((state) => state.settings);
  const setSetting = useApp((state) => state.setSetting);
  const accepted = demo || settings.acceptedRisk === "true";

  return (
    <div className="relative flex h-full items-center justify-center p-8">
      <div className="rise grid w-full max-w-[920px] grid-cols-[1.1fr_1fr] items-center gap-14">
        <div>
          <div className="flex items-center gap-3">
            <LogoMark size={46} />
            <span className="text-[34px] font-semibold tracking-tight">
              Di<span className="gradient-text">sweep</span>
            </span>
          </div>
          <h1 className="mt-7 text-[32px] leading-[1.15] font-semibold tracking-tight">
            Sweep your messages.
            <br />
            <span className="text-soft">Review it first.</span>
          </h1>
          <p className="mt-4 max-w-md text-[15px] text-soft">
            Find your own messages across DMs and servers, see exactly what would go, and delete
            them only once you've confirmed it.
          </p>
          <ul className="mt-6 space-y-2.5 text-soft">
            {[
              "Only ever deletes messages you wrote",
              "Nothing changes until you type a confirmation",
              "Your login stays in your system's credential store",
            ].map((line) => (
              <li key={line} className="flex items-center gap-2.5">
                <ShieldCheck className="size-4 shrink-0 text-accent" aria-hidden />
                {line}
              </li>
            ))}
          </ul>
        </div>

        <Card className="p-6">
          <Label>01 / Connect</Label>
          <h2 className="mt-1 mb-4 text-[19px] font-semibold">
            {demo ? "Try it with made-up data" : "Connect your account"}
          </h2>

          {!demo && (
            <div className="mb-4 space-y-3">
              <Callout icon={<TriangleAlert />}>
                Automating a personal account isn't allowed by{" "}
                <button
                  type="button"
                  className="underline"
                  onClick={() => api.openLink("https://discord.com/terms")}
                >
                  Discord's Terms of Service
                </button>
                . Discord can suspend accounts for it, and deleted messages can't be recovered.
              </Callout>
              <label className="flex items-start gap-2.5 text-[13px]">
                <input
                  type="checkbox"
                  className="mt-0.5 size-4 accent-[var(--accent)]"
                  checked={accepted}
                  onChange={(event) => {
                    const value = String(event.target.checked);
                    setSetting("acceptedRisk", value);
                    api.setSetting("acceptedRisk", value).catch(() => {});
                  }}
                />
                I understand the risk and I'm using my own account.
              </label>
            </div>
          )}

          {accepted && <SignInOptions />}
          {!demo && (
            <p className="mt-4 text-[12px] text-faint">
              Want a look first? Run <code className="font-mono">disweep --demo</code> to explore
              with no account.
            </p>
          )}
        </Card>
      </div>
    </div>
  );
}
