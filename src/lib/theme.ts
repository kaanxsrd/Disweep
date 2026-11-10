import { api } from "./api";

export type ThemeChoice = "system" | "light" | "dark";

const systemDark = window.matchMedia("(prefers-color-scheme: dark)");
let choice: ThemeChoice = "system";

function paint() {
  const resolved = choice === "system" ? (systemDark.matches ? "dark" : "light") : choice;
  document.documentElement.dataset.theme = resolved;
}

systemDark.addEventListener("change", paint);

export function applyTheme(next: ThemeChoice) {
  choice = next;
  paint();

  api.setWindowTheme(next).catch(() => {});
}

export function applyAppearance(settings: Record<string, string>) {
  applyTheme((settings.theme as ThemeChoice | undefined) ?? "system");
  document.documentElement.dataset.streamer = settings.streamerMode === "true" ? "true" : "false";
}
