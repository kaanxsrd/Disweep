import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowRight, ExternalLink, FileArchive, Search, Trash2 } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import {
  Avatar,
  Badge,
  Button,
  Card,
  Checkbox,
  EmptyState,
  Input,
  Label,
  PageHeader,
  Stat,
} from "@/components/ui";
import { api, errorText } from "@/lib/api";
import type { PackageChannel } from "@/lib/bindings/PackageChannel";
import type { Target } from "@/lib/bindings/Target";
import { cn } from "@/lib/cn";
import { count, date } from "@/lib/format";
import { useAccountId, useApp } from "@/lib/store";

const kindLabel = { direct: "DM", group: "Group", server: "Server", other: "Other" };

export function PackageScreen() {
  const account = useAccountId();
  const client = useQueryClient();
  const saved = useQuery({
    queryKey: ["package", account],
    queryFn: () => api.packageGet(account),
  });

  const choose = useMutation({
    mutationFn: async () => {
      const path = await api.packagePick();
      if (!path) return null;
      return api.packageImport(account, path);
    },
    onSuccess: (index) => {
      if (index) client.setQueryData(["package", account], index);
    },
    onError: (error) => toast.error(errorText(error)),
  });

  const forget = async () => {
    try {
      await api.packageForget(account);
      client.setQueryData(["package", account], null);
    } catch (error) {
      toast.error(errorText(error));
    }
  };

  const index = saved.data;

  return (
    <div className="flex h-full flex-col px-7 pt-6 pb-5">
      <PageHeader
        eyebrow="Data package"
        title="Reach what a scan can't"
        description="Discord's data package lists every message you've sent, including closed DMs and servers you've left."
        actions={
          index ? (
            <Button
              variant="ghost"
              icon={<Trash2 className="size-4" aria-hidden />}
              onClick={forget}
            >
              Forget package
            </Button>
          ) : undefined
        }
      />

      {!index && (
        <Card className="flex-1">
          <EmptyState
            icon={<FileArchive className="size-5" aria-hidden />}
            title="Import your data package"
          >
            <ol className="mt-2 mb-5 space-y-1 text-left">
              <li>1. In Discord, open Settings, then Data &amp; Privacy.</li>
              <li>2. Choose Request Data and include Messages.</li>
              <li>3. When Discord emails it to you, choose the package.zip here.</li>
            </ol>
            <div className="flex justify-center gap-2">
              <Button variant="primary" loading={choose.isPending} onClick={() => choose.mutate()}>
                Choose package.zip
              </Button>
              <Button
                icon={<ExternalLink className="size-4" aria-hidden />}
                onClick={() =>
                  api.openLink(
                    "https://support.discord.com/hc/en-us/articles/360004027692-Requesting-a-Copy-of-your-Data",
                  )
                }
              >
                How to request it
              </Button>
            </div>
          </EmptyState>
        </Card>
      )}

      {index && <PackageBrowser channels={index.channels} total={Number(index.totalMessages)} />}
    </div>
  );
}

function targetFor(channel: PackageChannel): Target {
  return {
    scope: { type: "package", channelId: channel.id },
    label: channel.guildName ? `${channel.name} (${channel.guildName})` : channel.name,
    expected: Number(channel.messageCount),
  };
}

function PackageBrowser({ channels, total }: { channels: PackageChannel[]; total: number }) {
  const setTargets = useApp((state) => state.setTargets);
  const setStep = useApp((state) => state.setStep);
  const go = useApp((state) => state.go);
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const scroller = useRef<HTMLDivElement>(null);

  const visible = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return channels.filter((channel) =>
      `${channel.name} ${channel.guildName ?? ""}`.toLowerCase().includes(needle),
    );
  }, [channels, search]);

  const rows = useVirtualizer({
    count: visible.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 50,
    overscan: 12,
  });

  const toggle = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };

  const allShown = visible.length > 0 && visible.every((channel) => selected.has(channel.id));
  const toggleAll = () => {
    const next = new Set(selected);
    for (const channel of visible) {
      if (allShown) next.delete(channel.id);
      else next.add(channel.id);
    }
    setSelected(next);
  };

  const review = () => {
    const chosen = channels.filter((channel) => selected.has(channel.id));
    setTargets(
      Object.fromEntries(chosen.map((channel) => [`package:${channel.id}`, targetFor(channel)])),
    );
    setStep("review");
    go("sweep");
  };

  const selectedCount = channels
    .filter((channel) => selected.has(channel.id))
    .reduce((sum, channel) => sum + Number(channel.messageCount), 0);

  return (
    <>
      <Card className="mb-4 grid grid-cols-3 gap-4 p-5">
        <Stat label="Messages in package" value={count(total)} />
        <Stat label="Conversations" value={count(channels.length)} />
        <Stat label="Selected" value={count(selectedCount)} />
      </Card>

      <Card className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <div className="flex items-center gap-3 border-b border-line p-3">
          <button
            type="button"
            onClick={toggleAll}
            className="flex items-center gap-2.5 pl-1 text-[13px] whitespace-nowrap text-soft hover:text-ink"
          >
            <Checkbox checked={allShown} label="All shown" />
            Select all
          </button>
          <div className="relative flex-1">
            <Search
              className="pointer-events-none absolute top-2.5 left-3 size-4 text-faint"
              aria-hidden
            />
            <Input
              aria-label="Search the package"
              placeholder="Search conversations"
              className="pl-9"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </div>
        </div>

        <div ref={scroller} className="min-h-0 flex-1 overflow-y-auto">
          {visible.length === 0 && (
            <div className="p-8 text-center text-soft">Nothing matches.</div>
          )}
          <div className="relative" style={{ height: rows.getTotalSize() }}>
            {rows.getVirtualItems().map((row) => {
              const channel = visible[row.index];
              if (!channel) return null;
              const isSelected = selected.has(channel.id);
              return (
                <button
                  key={channel.id}
                  type="button"
                  aria-pressed={isSelected}
                  onClick={() => toggle(channel.id)}
                  className={cn(
                    "absolute inset-x-0 flex items-center gap-3 px-4 text-left transition",
                    isSelected ? "bg-accent/8" : "hover:bg-raised/70",
                  )}
                  style={{ height: row.size, transform: `translateY(${row.start}px)` }}
                >
                  <Checkbox checked={isSelected} label={channel.name} />
                  <Avatar name={channel.name} size={30} />
                  <div className="min-w-0 flex-1">
                    <div className="sensitive truncate font-medium">{channel.name}</div>
                    {channel.guildName && (
                      <div className="sensitive truncate text-[12px] text-faint">
                        {channel.guildName}
                      </div>
                    )}
                  </div>
                  <Badge>{kindLabel[channel.kind]}</Badge>
                  <span className="w-28 text-right text-[12px] text-faint">
                    {channel.lastMessageAt !== null && date(Number(channel.lastMessageAt))}
                  </span>
                  <span className="tabular w-16 text-right">
                    {count(Number(channel.messageCount))}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      </Card>

      <div className="mt-4 flex items-center justify-between">
        <Label>Deletes by ID, so no searching is needed</Label>
        <Button variant="primary" size="lg" disabled={selected.size === 0} onClick={review}>
          Review {selected.size > 0 ? count(selectedCount) : ""} messages{" "}
          <ArrowRight className="size-4" aria-hidden />
        </Button>
      </div>
    </>
  );
}
