import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Sidebar } from "@/components/Sidebar";
import { Button, Modal } from "@/components/ui";
import { api, errorText } from "@/lib/api";
import type { BotStatus } from "@/lib/bindings/BotStatus";
import type { EngineEvent } from "@/lib/bindings/EngineEvent";
import { useApp } from "@/lib/store";
import { applyAppearance } from "@/lib/theme";
import { ActivityScreen } from "@/screens/Activity";
import { AutosweepScreen } from "@/screens/Autosweep";
import { BotScreen } from "@/screens/Bot";
import { PackageScreen } from "@/screens/DataPackage";
import { SettingsScreen } from "@/screens/Settings";
import { SignInOptions, SignInScreen, useLoginEvents } from "@/screens/SignIn";
import { SweepScreen } from "@/screens/Sweep";

function useBoot(): boolean {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let stopped = false;
    const unlisteners: (() => void)[] = [];

    const load = async () => {
      const state = useApp.getState();
      const [info, accounts, settings, bot, active] = await Promise.all([
        api.appInfo(),
        api.accounts(),
        api.settings(),
        api.botStatus(),
        api.jobsActive(),
      ]);
      state.setInfo(info);
      state.setAccounts(accounts);
      state.setSettings(settings);
      state.setBot(bot);
      state.setJobs(active);
      applyAppearance(useApp.getState().settings);
      return info;
    };

    const start = async () => {
      unlisteners.push(
        await listen<EngineEvent>("engine", ({ payload }) => {
          useApp.getState().applyEngineEvent(payload);
        }),
        await listen<BotStatus>("bot", ({ payload }) => {
          useApp.getState().setBot(payload);
        }),
      );

      // Saved accounts are restored in the background; wait so the sign-in screen can't flash.
      const restored = new Promise<void>((resolve) => {
        listen("accounts-restored", () => resolve()).then((stop) => unlisteners.push(stop));
      });
      const first = await load();
      if (!first.restored) {
        await restored;
        await load();
      }
      if (!stopped) setReady(true);
      await api.uiReady();
    };

    start().catch((error) => toast.error(errorText(error)));
    return () => {
      stopped = true;
      for (const stop of unlisteners) stop();
    };
  }, []);

  return ready;
}

export function App() {
  const ready = useBoot();
  const accounts = useApp((state) => state.accounts);
  const screen = useApp((state) => state.screen);
  const [adding, setAdding] = useState(false);
  useLoginEvents(() => setAdding(false));

  if (!ready) return null;
  if (accounts.length === 0) return <SignInScreen />;

  return (
    <div className="relative flex h-full">
      <Sidebar onAddAccount={() => setAdding(true)} />
      <main className="min-w-0 flex-1 overflow-hidden">
        <div key={screen} className="rise h-full">
          {screen === "sweep" && <SweepScreen />}
          {screen === "activity" && <ActivityScreen />}
          {screen === "autosweep" && <AutosweepScreen />}
          {screen === "package" && <PackageScreen />}
          {screen === "bot" && <BotScreen />}
          {screen === "settings" && <SettingsScreen />}
        </div>
      </main>
      {adding && (
        <Modal
          title="Add an account"
          onClose={() => setAdding(false)}
          footer={
            <Button variant="ghost" onClick={() => setAdding(false)}>
              Close
            </Button>
          }
        >
          <SignInOptions onDone={() => setAdding(false)} />
        </Modal>
      )}
    </div>
  );
}
