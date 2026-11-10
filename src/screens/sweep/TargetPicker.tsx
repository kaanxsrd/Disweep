import { useQuery } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronRight, Hash, LoaderCircle, Search, Server, Users } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { Avatar, Button, Card, Checkbox, EmptyState, Input, Segmented } from "@/components/ui";
import { api, errorText } from "@/lib/api";
import type { ConversationEntry } from "@/lib/bindings/ConversationEntry";
import type { GuildEntry } from "@/lib/bindings/GuildEntry";
import type { Target } from "@/lib/bindings/Target";
import { cn } from "@/lib/cn";
import { ago } from "@/lib/format";
import { useAccountId } from "@/lib/store";

type Tab = "dms" | "servers";

const dmKey = (id: string) => `dm:${id}`;
const guildKey = (id: string) => `guild:${id}`;
const channelKey = (id: string) => `channel:${id}`;

function dmTarget(conversation: ConversationEntry): Target {
  return {
    scope: { type: "private", channelId: conversation.id },
    label: conversation.name,
    expected: null,
  };
}

interface PickerProps {
  targets: Record<string, Target>;
  onToggle: (key: string, target: Target) => void;
  onReplace: (targets: Record<string, Target>) => void;
}

/** The list of conversations and servers. Shared by the sweep flow and new rules. */
export function TargetPicker({ targets, onToggle, onReplace }: PickerProps) {
  const account = useAccountId();
  const [tab, setTab] = useState<Tab>("dms");
  const [search, setSearch] = useState("");
  const inventory = useQuery({
    queryKey: ["inventory", account],
    queryFn: () => api.inventory(account),
  });

  const needle = search.trim().toLowerCase();
  const conversations = useMemo(
    () =>
      (inventory.data?.conversations ?? []).filter((item) =>
        item.name.toLowerCase().includes(needle),
      ),
    [inventory.data, needle],
  );
  const guilds = useMemo(
    () => (inventory.data?.guilds ?? []).filter((item) => item.name.toLowerCase().includes(needle)),
    [inventory.data, needle],
  );

  const allVisibleSelected =
    conversations.length > 0 && conversations.every((item) => targets[dmKey(item.id)]);

  const toggleAllVisible = () => {
    const next = { ...targets };
    for (const item of conversations) {
      if (allVisibleSelected) delete next[dmKey(item.id)];
      else next[dmKey(item.id)] = dmTarget(item);
    }
    onReplace(next);
  };

  return (
    <Card className="flex min-h-0 flex-col overflow-hidden">
      <div className="space-y-3 border-b border-line p-3">
        <Segmented
          label="What to browse"
          value={tab}
          onChange={setTab}
          options={[
            { value: "dms", label: "Direct messages" },
            { value: "servers", label: "Servers" },
          ]}
        />
        <div className="relative">
          <Search
            className="pointer-events-none absolute top-2.5 left-3 size-4 text-faint"
            aria-hidden
          />
          <Input
            aria-label="Search"
            placeholder={tab === "dms" ? "Search conversations" : "Search servers"}
            className="pl-9"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
        </div>
      </div>

      <div className="min-h-0 flex-1">
        {inventory.isPending && (
          <EmptyState
            icon={<LoaderCircle className="size-5 animate-spin" aria-hidden />}
            title="Loading your conversations"
          />
        )}
        {inventory.isError && (
          <EmptyState icon={<Server className="size-5" aria-hidden />} title="Couldn't load these">
            <p className="mb-3">{errorText(inventory.error)}</p>
            <Button onClick={() => inventory.refetch()}>Try again</Button>
          </EmptyState>
        )}
        {inventory.isSuccess && tab === "dms" && (
          <ConversationList
            conversations={conversations}
            targets={targets}
            onToggle={onToggle}
            allSelected={allVisibleSelected}
            onToggleAll={toggleAllVisible}
          />
        )}
        {inventory.isSuccess && tab === "servers" && (
          <div className="h-full overflow-y-auto p-1.5">
            {guilds.map((guild) => (
              <GuildRow
                key={guild.id}
                guild={guild}
                targets={targets}
                onToggle={onToggle}
                onReplace={onReplace}
              />
            ))}
            {guilds.length === 0 && (
              <EmptyState
                icon={<Server className="size-5" aria-hidden />}
                title="No servers found"
              />
            )}
          </div>
        )}
      </div>
    </Card>
  );
}

function ConversationList({
  conversations,
  targets,
  onToggle,
  allSelected,
  onToggleAll,
}: {
  conversations: ConversationEntry[];
  targets: Record<string, Target>;
  onToggle: PickerProps["onToggle"];
  allSelected: boolean;
  onToggleAll: () => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const rows = useVirtualizer({
    count: conversations.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 48,
    overscan: 12,
  });

  if (conversations.length === 0) {
    return <EmptyState icon={<Users className="size-5" aria-hidden />} title="Nothing here" />;
  }

  return (
    <div className="flex h-full flex-col">
      <button
        type="button"
        onClick={onToggleAll}
        className="flex items-center gap-3 border-b border-line px-4 py-2 text-left text-[13px] text-soft hover:text-ink"
      >
        <Checkbox checked={allSelected} label="All shown" />
        Select all {conversations.length} shown
      </button>
      <div ref={scroller} className="min-h-0 flex-1 overflow-y-auto">
        <div className="relative" style={{ height: rows.getTotalSize() }}>
          {rows.getVirtualItems().map((row) => {
            const conversation = conversations[row.index];
            if (!conversation) return null;
            const key = dmKey(conversation.id);
            const selected = Boolean(targets[key]);
            return (
              <button
                key={conversation.id}
                type="button"
                aria-pressed={selected}
                onClick={() => onToggle(key, dmTarget(conversation))}
                className={cn(
                  "absolute inset-x-0 flex items-center gap-3 px-4 text-left transition",
                  selected ? "bg-accent/8" : "hover:bg-raised/70",
                )}
                style={{ height: row.size, transform: `translateY(${row.start}px)` }}
              >
                <Checkbox checked={selected} label={conversation.name} />
                <Avatar name={conversation.name} url={conversation.iconUrl} size={30} />
                <span className="sensitive min-w-0 flex-1 truncate font-medium">
                  {conversation.name}
                </span>
                {conversation.isGroup && (
                  <Users className="size-3.5 text-faint" aria-label="Group" />
                )}
                {conversation.lastMessageAt !== null && (
                  <span className="text-[12px] whitespace-nowrap text-faint">
                    {ago(conversation.lastMessageAt)}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function GuildRow({ guild, targets, onToggle, onReplace }: PickerProps & { guild: GuildEntry }) {
  const account = useAccountId();
  const [open, setOpen] = useState(false);
  const channels = useQuery({
    queryKey: ["channels", account, guild.id],
    queryFn: () => api.guildChannels(account, guild.id),
    enabled: open,
  });

  const wholeKey = guildKey(guild.id);
  const whole = Boolean(targets[wholeKey]);
  const inGuild = (channels.data ?? []).filter((channel) => targets[channelKey(channel.id)]).length;

  const toggleWhole = () => {
    const next = { ...targets };
    if (whole) {
      delete next[wholeKey];
    } else {
      // A whole-server sweep covers every channel, so individual picks would repeat the work.
      for (const channel of channels.data ?? []) delete next[channelKey(channel.id)];
      next[wholeKey] = {
        scope: { type: "guild", guildId: guild.id },
        label: guild.name,
        expected: null,
      };
    }
    onReplace(next);
  };

  return (
    <div className="rounded-lg">
      <div className={cn("flex items-center rounded-lg", whole && "bg-accent/8")}>
        <button
          type="button"
          aria-expanded={open}
          aria-label={`Show channels in ${guild.name}`}
          onClick={() => setOpen(!open)}
          className="flex size-11 shrink-0 items-center justify-center text-faint hover:text-ink"
        >
          <ChevronRight className={cn("size-4 transition", open && "rotate-90")} aria-hidden />
        </button>
        <button
          type="button"
          aria-pressed={whole}
          onClick={toggleWhole}
          className="flex min-w-0 flex-1 items-center gap-3 py-2 pr-3 text-left"
        >
          <Checkbox checked={whole} label={`All of ${guild.name}`} />
          <Avatar name={guild.name} url={guild.iconUrl} size={28} square />
          <span className="sensitive min-w-0 flex-1 truncate font-medium">{guild.name}</span>
          <span className="text-[12px] text-faint">
            {whole ? "Entire server" : inGuild > 0 ? `${inGuild} chosen` : ""}
          </span>
        </button>
      </div>
      {open && (
        <div className="mb-1 ml-11 border-l border-line pl-2">
          {channels.isPending && <div className="px-2 py-2 text-[13px] text-faint">Loading…</div>}
          {channels.isError && (
            <div className="px-2 py-2 text-[13px] text-bad">{errorText(channels.error)}</div>
          )}
          {channels.data?.map((channel, index) => {
            const key = channelKey(channel.id);
            const selected = Boolean(targets[key]);
            const header =
              channel.category !== channels.data?.[index - 1]?.category ? channel.category : null;
            return (
              <div key={channel.id}>
                {header && <div className="label px-2 pt-2 pb-1">{header}</div>}
                <button
                  type="button"
                  aria-pressed={selected}
                  disabled={whole}
                  onClick={() =>
                    onToggle(key, {
                      scope: { type: "channel", guildId: guild.id, channelId: channel.id },
                      label: `#${channel.name}`,
                      expected: null,
                    })
                  }
                  className={cn(
                    "flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-left transition disabled:opacity-40",
                    selected ? "bg-accent/8" : "hover:bg-raised/70",
                  )}
                >
                  <Checkbox checked={selected || whole} label={channel.name} />
                  <Hash className="size-3.5 text-faint" aria-hidden />
                  <span className="sensitive min-w-0 flex-1 truncate">{channel.name}</span>
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
