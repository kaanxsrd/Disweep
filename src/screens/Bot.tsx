import {
  Bot as BotIcon,
  CircleCheck,
  ExternalLink,
  Info,
  Link2,
  Play,
  Trash2,
  TriangleAlert,
  Wand2,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import { toast } from "sonner";
import {
  Avatar,
  Badge,
  Button,
  Callout,
  Card,
  Field,
  Input,
  Label,
  Modal,
  PageHeader,
  Switch,
} from "@/components/ui";
import { api, errorText } from "@/lib/api";
import type { BotStatus } from "@/lib/bindings/BotStatus";
import type { CommandState } from "@/lib/bindings/CommandState";
import { cn } from "@/lib/cn";
import { ago, count } from "@/lib/format";
import { useApp } from "@/lib/store";

const PORTAL = "https://discord.com/developers/applications";

function connectionBadge(status: BotStatus) {
  switch (status.connection) {
    case "online":
      return <Badge tone="good">Online{status.simulated ? " (simulated)" : ""}</Badge>;
    case "connecting":
      return <Badge tone="accent">Connecting</Badge>;
    case "reconnecting":
      return <Badge tone="warn">Reconnecting</Badge>;
    case "failed":
      return <Badge tone="bad">Couldn't connect</Badge>;
    case "off":
      return <Badge>Off</Badge>;
  }
}

export function BotScreen() {
  const status = useApp((state) => state.bot);
  const demo = useApp((state) => state.info?.demo);
  const setBot = useApp((state) => state.setBot);
  const [replaceFor, setReplaceFor] = useState<string[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  if (!status) return null;

  const act = async (name: string, action: () => Promise<BotStatus>) => {
    setBusy(name);
    try {
      setBot(await action());
    } catch (error) {
      toast.error(errorText(error));
    } finally {
      setBusy(null);
    }
  };

  const configure = () => {
    if (status.foreignCommands.length > 0) {
      setReplaceFor(status.foreignCommands);
      return;
    }
    act("configure", () => api.botConfigure(false));
  };

  const app = status.application;
  const addedToken = status.hasToken;
  const configured = addedToken && status.registered;

  return (
    <div className="h-full overflow-y-auto px-7 pt-6 pb-6">
      <PageHeader
        eyebrow="Bot"
        title="Sweep from inside Discord"
        description="Connect your own Discord bot and use /sweep in any DM. Only you can see its replies."
        actions={
          <div className="flex items-center gap-3">
            {connectionBadge(status)}
            <Switch
              label="Bot online"
              checked={status.enabled}
              disabled={!configured || busy !== null}
              onChange={(enabled) => act("enable", () => api.botSetEnabled(enabled))}
            />
          </div>
        }
      />

      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] items-start gap-4">
        <div className="space-y-4">
          <Card className="p-5">
            <Label className="mb-3">Setup</Label>
            <ol className="space-y-4">
              <Step number={1} done={addedToken} title="Create a Discord application">
                <p>
                  Make a new application, open its Bot page and reset the token. Use one just for
                  this, because setup replaces its commands.
                </p>
                <Button
                  size="sm"
                  className="mt-2"
                  icon={<ExternalLink className="size-3.5" aria-hidden />}
                  onClick={() => api.openLink(PORTAL)}
                >
                  Open the Developer Portal
                </Button>
              </Step>

              <Step number={2} done={addedToken} title="Add the bot's token">
                {addedToken && app ? (
                  <div className="flex items-center gap-2.5">
                    <Avatar name={app.name} url={app.avatarUrl} size={28} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-medium text-ink">{app.name}</div>
                      {app.botUsername && (
                        <div className="truncate text-[12px] text-faint">@{app.botUsername}</div>
                      )}
                    </div>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label="Remove token"
                      icon={<Trash2 className="size-3.5" aria-hidden />}
                      onClick={() => act("clear", api.botClearToken)}
                    />
                  </div>
                ) : demo ? (
                  <Button
                    size="sm"
                    loading={busy === "demo"}
                    onClick={() => act("demo", api.demoBotSetup)}
                  >
                    Use the demo bot
                  </Button>
                ) : (
                  <TokenForm
                    busy={busy === "token"}
                    onSubmit={(token) => act("token", () => api.botSetToken(token))}
                  />
                )}
              </Step>

              <Step number={3} done={configured} title="Set up the commands">
                <p>Registers the slash commands and turns on installing to your account.</p>
                <Button
                  size="sm"
                  className="mt-2"
                  variant={configured ? "secondary" : "primary"}
                  disabled={!addedToken}
                  loading={busy === "configure"}
                  icon={<Wand2 className="size-3.5" aria-hidden />}
                  onClick={configure}
                >
                  {configured ? "Run setup again" : "Set up automatically"}
                </Button>
              </Step>

              <Step number={4} done={false} title="Add it to your account">
                <p>Do this once, then turn the bot on with the switch above.</p>
                <div className="mt-2 flex flex-wrap gap-2">
                  <Button
                    size="sm"
                    disabled={!status.links}
                    icon={<Link2 className="size-3.5" aria-hidden />}
                    onClick={() => status.links && api.openLink(status.links.account)}
                  >
                    Add to my account
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={!status.links}
                    onClick={() => status.links && api.openLink(status.links.server)}
                  >
                    Add to a server, for /purge
                  </Button>
                </div>
              </Step>
            </ol>
          </Card>

          {status.problem && (
            <Callout tone="bad" icon={<TriangleAlert />}>
              {status.problem}
            </Callout>
          )}
          {app && !app.supportsUserInstall && configured && (
            <Callout icon={<TriangleAlert />}>
              Discord didn't accept account installs for this application. In the Developer Portal,
              open Installation, turn on User Install, then run setup again.
            </Callout>
          )}
          {app?.hasConflictingEndpoint && (
            <Callout tone="bad" icon={<TriangleAlert />}>
              This application has an Interactions Endpoint URL, so commands would never reach
              Disweep. Clear it in the Developer Portal.
            </Callout>
          )}

          <Callout tone="accent" icon={<Info />}>
            <b className="font-medium">How /sweep works.</b> Discord doesn't let bots delete your
            messages in a DM, so the bot passes the command to this app, and the app deletes them
            with your signed-in account. Disweep has to be open or in your tray for commands to
            answer.
          </Callout>
        </div>

        <div className="space-y-4">
          <Card className="overflow-hidden">
            <div className="border-b border-line px-5 py-3">
              <Label>Commands</Label>
            </div>
            <CommandGroup
              title="Personal, for you only"
              commands={status.commands.filter((command) => command.scope === "personal")}
              onToggle={(key, enabled) => act(key, () => api.botSetCommand(key, enabled))}
            />
            <CommandGroup
              title="Server moderation"
              commands={status.commands.filter((command) => command.scope === "server")}
              onToggle={(key, enabled) => act(key, () => api.botSetCommand(key, enabled))}
            />
          </Card>

          {demo && status.hasToken && (
            <Card className="p-5">
              <Label className="mb-2">Try it</Label>
              <p className="mb-3 text-soft">
                Sends a pretend command through the real bot code. The result appears below.
              </p>
              <div className="flex flex-wrap gap-2">
                {["here", "status", "stats"].map((name) => (
                  <Button
                    key={name}
                    size="sm"
                    icon={<Play className="size-3.5" aria-hidden />}
                    onClick={() =>
                      api.demoBotCommand(name).catch((error) => toast.error(errorText(error)))
                    }
                  >
                    /sweep {name}
                  </Button>
                ))}
              </div>
            </Card>
          )}

          <Card className="overflow-hidden">
            <div className="border-b border-line px-5 py-3">
              <Label>Recent commands</Label>
            </div>
            {status.activity.length === 0 && (
              <div className="flex items-center justify-center gap-2 p-8 text-soft">
                <BotIcon className="size-4" aria-hidden /> Nothing yet.
              </div>
            )}
            {status.activity.slice(0, 12).map((entry) => (
              <div
                key={`${entry.at}-${entry.command}`}
                className="flex items-baseline gap-3 border-b border-line/60 px-5 py-2.5 last:border-0"
              >
                <code className="font-mono text-[12px] whitespace-nowrap text-accent">
                  {entry.command}
                </code>
                <span className="sensitive min-w-0 flex-1 truncate text-soft">{entry.summary}</span>
                <span className="text-[12px] whitespace-nowrap text-faint">{ago(entry.at)}</span>
              </div>
            ))}
          </Card>

          {app && (
            <div className="px-1 text-[12px] text-faint">
              {app.serverCount !== null && `Servers: ${count(Number(app.serverCount))}. `}
              {app.userInstallCount !== null && `Accounts: ${count(Number(app.userInstallCount))}.`}
              {status.latencyMs !== null && ` Latency ${status.latencyMs} ms.`}
            </div>
          )}
        </div>
      </div>

      {replaceFor && (
        <Modal
          title="Replace this application's commands?"
          onClose={() => setReplaceFor(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setReplaceFor(null)}>
                Cancel
              </Button>
              <Button
                variant="danger"
                onClick={() => {
                  setReplaceFor(null);
                  act("configure", () => api.botConfigure(true));
                }}
              >
                Replace them
              </Button>
            </>
          }
        >
          <p className="text-soft">
            This application already has commands that didn't come from Disweep. Setup will remove
            them:
          </p>
          <ul className="mt-2 font-mono text-[13px]">
            {replaceFor.map((name) => (
              <li key={name}>/{name}</li>
            ))}
          </ul>
        </Modal>
      )}
    </div>
  );
}

function Step({
  number,
  done,
  title,
  children,
}: {
  number: number;
  done: boolean;
  title: string;
  children: ReactNode;
}) {
  return (
    <li className="flex gap-3">
      {done ? (
        <CircleCheck className="mt-px size-[22px] shrink-0 text-good" aria-label="Done" />
      ) : (
        <span className="flex size-[22px] shrink-0 items-center justify-center rounded-full bg-raised font-mono text-[12px] text-soft">
          {number}
        </span>
      )}
      <div className="min-w-0 flex-1 text-[13px] text-soft">
        <div className="mb-0.5 text-sm font-medium text-ink">{title}</div>
        {children}
      </div>
    </li>
  );
}

function TokenForm({ busy, onSubmit }: { busy: boolean; onSubmit: (token: string) => void }) {
  const [token, setToken] = useState("");
  return (
    <form
      className="flex items-end gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(token);
        setToken("");
      }}
    >
      <div className="flex-1">
        <Field label="Bot token">
          <Input
            type="password"
            autoComplete="off"
            spellCheck={false}
            placeholder="Paste the token"
            value={token}
            onChange={(event) => setToken(event.target.value)}
          />
        </Field>
      </div>
      <Button type="submit" variant="primary" loading={busy} disabled={token.trim() === ""}>
        Save
      </Button>
    </form>
  );
}

function CommandGroup({
  title,
  commands,
  onToggle,
}: {
  title: string;
  commands: CommandState[];
  onToggle: (key: string, enabled: boolean) => void;
}) {
  return (
    <div>
      <div className="px-5 pt-3 pb-1 text-[12px] font-medium text-faint">{title}</div>
      {commands.map((command) => (
        <div key={command.key} className={cn("flex items-center gap-3 px-5 py-2")}>
          <div className="min-w-0 flex-1">
            <code className="font-mono text-[13px]">{command.display}</code>
            <div className="truncate text-[12px] text-soft">{command.description}</div>
          </div>
          <Switch
            label={command.display}
            checked={command.enabled}
            onChange={(enabled) => onToggle(command.key, enabled)}
          />
        </div>
      ))}
    </div>
  );
}
