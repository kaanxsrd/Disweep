import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, FolderOpen, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import {
  Button,
  Callout,
  Card,
  Input,
  Label,
  Modal,
  PageHeader,
  Segmented,
  Toggle,
} from "@/components/ui";
import { api, errorText } from "@/lib/api";
import { useApp } from "@/lib/store";
import { applyAppearance, type ThemeChoice } from "@/lib/theme";

const ERASE_WORD = "ERASE";

export function SettingsScreen() {
  const settings = useApp((state) => state.settings);
  const setLocal = useApp((state) => state.setSetting);
  const info = useApp((state) => state.info);
  const setInfo = useApp((state) => state.setInfo);
  const client = useQueryClient();
  const [erasing, setErasing] = useState(false);

  const autostart = useQuery({
    queryKey: ["autostart"],
    queryFn: api.autostartGet,
    enabled: !info?.demo,
  });

  const save = (key: string, value: string) => {
    setLocal(key, value);
    applyAppearance({ ...useApp.getState().settings });
    api.setSetting(key, value).catch((error) => toast.error(errorText(error)));
  };

  const flag = (key: string, fallback: boolean) =>
    settings[key] === undefined ? fallback : settings[key] === "true";

  const pickFolder = async () => {
    try {
      const folder = await api.archiveFolderPick();
      if (folder && info) setInfo({ ...info, archiveFolder: folder });
    } catch (error) {
      toast.error(errorText(error));
    }
  };

  return (
    <div className="h-full overflow-y-auto px-7 pt-6 pb-6">
      <PageHeader eyebrow="Settings" title="Settings" />

      <div className="max-w-2xl space-y-4">
        <Card className="space-y-4 p-5">
          <Label>Appearance</Label>
          <div className="flex items-center justify-between gap-4">
            <div className="text-sm font-medium">Theme</div>
            <div className="w-64">
              <Segmented<ThemeChoice>
                label="Theme"
                value={(settings.theme as ThemeChoice | undefined) ?? "system"}
                onChange={(theme) => save("theme", theme)}
                options={[
                  { value: "system", label: "System" },
                  { value: "light", label: "Light" },
                  { value: "dark", label: "Dark" },
                ]}
              />
            </div>
          </div>
          <Toggle
            label="Streamer mode"
            description="Blurs names and message text, so it's safe to share your screen."
            checked={flag("streamerMode", false)}
            onChange={(value) => save("streamerMode", String(value))}
          />
        </Card>

        <Card className="space-y-4 p-5">
          <Label>Running in the background</Label>
          <Toggle
            label="Stay in the tray when closed"
            description="Used while a sweep, an autosweep rule or the bot is active."
            checked={flag("closeToTray", true)}
            onChange={(value) => save("closeToTray", String(value))}
          />
          <Toggle
            label="Start with my computer"
            description="Opens quietly in the tray, so rules and the bot keep working."
            disabled={info?.demo}
            checked={autostart.data ?? false}
            onChange={async (value) => {
              try {
                await api.autostartSet(value);
                await client.invalidateQueries({ queryKey: ["autostart"] });
              } catch (error) {
                toast.error(errorText(error));
              }
            }}
          />
          <Toggle
            label="Notify me when a sweep ends"
            checked={flag("notifications", true)}
            onChange={(value) => save("notifications", String(value))}
          />
        </Card>

        <Card className="space-y-3 p-5">
          <Label>Archives</Label>
          <div className="flex items-center gap-3">
            <div className="selectable min-w-0 flex-1 truncate font-mono text-[13px] text-soft">
              {info?.archiveFolder}
            </div>
            <Button
              size="sm"
              icon={<FolderOpen className="size-3.5" aria-hidden />}
              onClick={pickFolder}
            >
              Change
            </Button>
          </div>
        </Card>

        <Card className="space-y-3 p-5">
          <Label>About</Label>
          <div className="flex items-center justify-between">
            <div>
              <div className="font-medium">Disweep {info?.version}</div>
              <div className="text-[13px] text-soft">
                Free software under the GNU GPL, version 3.
              </div>
            </div>
            <Button
              size="sm"
              icon={<ExternalLink className="size-3.5" aria-hidden />}
              onClick={() => api.openLink("https://github.com/kaanxsrd/Disweep")}
            >
              Project page
            </Button>
          </div>
        </Card>

        <Card className="space-y-3 border-bad/30 p-5">
          <Label>Erase</Label>
          <div className="flex items-center justify-between gap-4">
            <div>
              <div className="font-medium">Erase Disweep's data on this computer</div>
              <div className="text-[13px] text-soft">
                Signs out, stops everything and removes saved history and rules. Saved archives stay
                where they are.
              </div>
            </div>
            <Button variant="danger" size="sm" onClick={() => setErasing(true)}>
              Erase
            </Button>
          </div>
        </Card>
      </div>

      {erasing && <EraseDialog onClose={() => setErasing(false)} />}
    </div>
  );
}

function EraseDialog({ onClose }: { onClose: () => void }) {
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const setAccounts = useApp((state) => state.setAccounts);
  const setSettings = useApp((state) => state.setSettings);
  const setBot = useApp((state) => state.setBot);
  const confirmed = typed.trim().toUpperCase() === ERASE_WORD;

  const erase = async () => {
    setBusy(true);
    try {
      await api.wipeLocalData();
      setSettings([]);
      setBot(await api.botStatus());
      setAccounts([]);
    } catch (error) {
      toast.error(errorText(error));
      setBusy(false);
    }
  };

  return (
    <Modal
      title="Erase Disweep's data?"
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="danger" disabled={!confirmed} loading={busy} onClick={erase}>
            Erase everything
          </Button>
        </>
      }
    >
      <Callout tone="bad" icon={<TriangleAlert />}>
        Your Discord messages aren't affected. This only clears what Disweep has stored here.
      </Callout>
      <div className="mt-3">
        <Input
          aria-label={`Type ${ERASE_WORD} to confirm`}
          className="font-mono uppercase"
          placeholder={`Type ${ERASE_WORD}`}
          value={typed}
          onChange={(event) => setTyped(event.target.value)}
        />
      </div>
    </Modal>
  );
}
