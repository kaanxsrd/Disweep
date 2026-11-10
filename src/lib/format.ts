const numbers = new Intl.NumberFormat("en-US");

export function count(value: number): string {
  return numbers.format(value);
}

export function tick(value: number): string {
  if (value < 10_000) return count(value);
  if (value < 1_000_000) return `${Math.round(value / 1000)}K`;
  return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
}

export function duration(milliseconds: number): string {
  const minutes = Math.ceil(milliseconds / 60_000);
  if (milliseconds < 60_000) return "under a minute";
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return minutes % 60 === 0 ? `${hours} h` : `${hours} h ${minutes % 60} min`;
  const days = Math.floor(hours / 24);
  return hours % 24 === 0 ? `${days} d` : `${days} d ${hours % 24} h`;
}

const seconds = (value: number, unit: string) => `${value} ${unit}${value === 1 ? "" : "s"}`;

export function interval(totalSeconds: number): string {
  const units: [number, string][] = [
    [31_536_000, "year"],
    [2_592_000, "month"],
    [604_800, "week"],
    [86_400, "day"],
    [3_600, "hour"],
    [60, "minute"],
  ];
  for (const [size, unit] of units) {
    if (totalSeconds >= size && totalSeconds % size === 0) {
      return seconds(totalSeconds / size, unit);
    }
  }
  return seconds(Math.ceil(totalSeconds / 60), "minute");
}

const dateFormat = new Intl.DateTimeFormat(undefined, {
  year: "numeric",
  month: "short",
  day: "numeric",
});
const dateTimeFormat = new Intl.DateTimeFormat(undefined, {
  year: "numeric",
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

export function date(milliseconds: number): string {
  return dateFormat.format(new Date(milliseconds));
}

export function dateTime(milliseconds: number): string {
  return dateTimeFormat.format(new Date(milliseconds));
}

export function ago(milliseconds: number, now = Date.now()): string {
  const elapsed = now - milliseconds;
  if (elapsed < 60_000) return "just now";
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return seconds(days, "day").concat(" ago");
  return date(milliseconds);
}

export function percent(done: number, total: number): number {
  if (total <= 0) return 0;
  if (done >= total) return 100;
  return Math.min(99, Math.floor((done / total) * 100));
}
