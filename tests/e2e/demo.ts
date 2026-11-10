import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium, type Page } from "playwright-core";
import sharp from "sharp";

const root = join(import.meta.dirname, "../..");
const executable = join(root, "target/release/disweep.exe");
const port = 9233;
const profile = mkdtempSync(join(tmpdir(), "disweep-e2e-"));
const updateDocs = process.argv.includes("--screenshots");
const shotFolder = updateDocs ? join(root, "docs/assets") : join(profile, "shots");
mkdirSync(shotFolder, { recursive: true });

const problems: string[] = [];

const sightings: string[] = [];
let failed = false;
let currentPage: Page | undefined;

async function step(name: string, action: () => Promise<void>) {
  try {
    await action();
    console.log(`  ok   ${name}`);
  } catch (error) {
    failed = true;
    const picture = join(tmpdir(), "disweep-e2e-failure.png");
    await currentPage?.screenshot({ path: picture }).catch(() => {});
    console.error(`  FAIL ${name}\n${error}`);
    console.error(`  The window at that moment: ${await describeWindow()}\n  Picture: ${picture}`);

    const reported = [...problems, ...sightings];
    if (reported.length > 0) {
      console.error(`  Seen during the run:\n    ${reported.join("\n    ")}`);
    }
    throw error;
  }
}

async function describeWindow(): Promise<string> {
  if (!currentPage) return "not open yet";
  return currentPage
    .evaluate(() => {
      const container = document.getElementById("root");
      if (!container || container.childElementCount === 0) return "blank";
      const screen = document.querySelector('nav [aria-current="page"]')?.textContent?.trim();
      const headings = Array.from(document.querySelectorAll("h1, h2"), (heading) =>
        heading.textContent?.trim(),
      ).filter(Boolean);
      return `the ${screen ?? "unknown"} screen, headed ${headings.join(" / ") || "nothing"}`;
    })
    .catch((error: unknown) => `unreadable (${error})`);
}

async function shot(page: Page, name: string) {
  const png = await page.screenshot({ type: "png" });
  await sharp(png)
    .webp({ quality: 88 })
    .toFile(join(shotFolder, `screenshot-${name}.webp`));
}

async function waitForDebugger() {
  for (let attempt = 0; attempt < 150; attempt++) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (response.ok) return;
    } catch {}
    await Bun.sleep(200);
  }
  throw new Error("The app never opened its debugging port.");
}

async function nav(page: Page, name: string) {
  await page.getByRole("navigation").getByRole("button", { name, exact: true }).click();

  await page.waitForFunction(
    (label) => document.querySelector('nav [aria-current="page"]')?.textContent?.trim() === label,
    name,
    { polling: 100 },
  );
}

const app = Bun.spawn([executable, "--demo"], {
  env: {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    WEBVIEW2_USER_DATA_FOLDER: profile,
  },
  stdout: "ignore",
  stderr: "ignore",
});

try {
  await waitForDebugger();
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const context = browser.contexts()[0];
  if (!context) throw new Error("No browser context.");
  let page: Page | undefined;
  for (let attempt = 0; attempt < 100 && !page; attempt++) {
    page = context.pages().find((candidate) => candidate.url().includes("tauri.localhost"));
    if (!page) await Bun.sleep(200);
  }
  if (!page) {
    const urls = context.pages().map((candidate) => candidate.url());
    throw new Error(`The app's page wasn't found. Open pages: ${urls.join(", ")}`);
  }
  currentPage = page;
  page.setDefaultTimeout(30_000);
  page.on("pageerror", (error) => problems.push(`page error: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(`console: ${message.text()}`);
  });
  page.on("crash", () => sightings.push("the page crashed"));

  page.on("framenavigated", (frame) => {
    if (frame.parentFrame() === null) sightings.push(`the page loaded again: ${frame.url()}`);
  });

  await step("shows the demo sign-in", async () => {
    await page.getByRole("button", { name: "Enter the demo" }).waitFor();
  });

  await step("signs in to the simulated Discord", async () => {
    await page.getByRole("button", { name: "Enter the demo" }).click();
    await page.getByRole("heading", { name: "What should be swept?" }).waitFor();
  });

  await step("switches to the dark theme", async () => {
    await nav(page, "Settings");
    await page.getByRole("radio", { name: "Dark" }).click();
    const theme = await page.evaluate(() => document.documentElement.dataset.theme);
    if (theme !== "dark") throw new Error(`theme is ${theme}`);
    await nav(page, "Sweep");
  });

  await step("lists conversations and picks some", async () => {
    await page.getByRole("button", { name: /Mira/ }).first().click();
    await page.getByRole("button", { name: /Jonas/ }).first().click();
    await page.getByText("places chosen").waitFor();
    await page.getByRole("radio", { name: "Older than" }).click();
    await Bun.sleep(300);
    await shot(page, "choose");
  });

  await step("browses a server's channels", async () => {
    await page.getByRole("radio", { name: "Servers" }).click();
    await page.getByRole("button", { name: "Show channels in Rust Harbor" }).click();
    await page.getByRole("button", { name: /general/ }).waitFor();
    await page.getByRole("radio", { name: "Direct messages" }).click();
  });

  await step("refuses an impossible filter", async () => {
    await page.getByRole("button", { name: "Advanced" }).click();
    await page.getByPlaceholder("^order #\\d+$").fill("(unclosed");
    await page
      .getByText(/regular expression isn't valid/)
      .first()
      .waitFor();
    if (await page.getByRole("button", { name: "Preview" }).isEnabled()) {
      throw new Error("Preview should be disabled for a bad pattern.");
    }
    await page.getByPlaceholder("^order #\\d+$").fill("");
    await page.getByRole("button", { name: "Advanced" }).click();
  });

  let expected = 0;
  await step("previews without deleting", async () => {
    await page.getByRole("button", { name: "Preview" }).click();
    await page.getByRole("heading", { name: "Review this sweep" }).waitFor();
    const text = (await page.locator(".gradient-text").first().textContent()) ?? "";

    expected = Number(text.replace(/[~,]/g, ""));
    if (!(expected > 0)) throw new Error(`Preview found ${text}`);
    await Bun.sleep(200);
    await shot(page, "review");
  });

  await step("won't start until the confirmation is typed", async () => {
    const start = page.getByRole("button", { name: /^Delete/ });
    if (await start.isEnabled()) throw new Error("Delete was enabled before typing.");
    await page.getByLabel("Type SWEEP to confirm").fill("nope");
    if (await start.isEnabled()) throw new Error("Delete was enabled by the wrong word.");
    await page.getByLabel("Type SWEEP to confirm").fill("sweep");
    await start.click();
  });

  const deletedCount = async () => {
    const value = await page.locator('[data-stat="Deleted"] > div').first().textContent();
    return Number((value ?? "").replaceAll(",", ""));
  };

  await step("shows live progress", async () => {
    await page.getByRole("heading", { name: /Mira and 1 more/ }).waitFor();
    await page.waitForFunction(() => document.querySelectorAll("[role=progressbar]").length > 0);
    for (let wait = 0; wait < 200 && (await deletedCount()) < 40; wait++) await Bun.sleep(100);
    await shot(page, "sweep");
  });

  await step("pauses and resumes", async () => {
    await page.getByRole("button", { name: "Pause" }).click();
    await page.getByRole("button", { name: "Resume" }).waitFor();
    await Bun.sleep(600);
    const held = await deletedCount();
    await Bun.sleep(1200);
    if ((await deletedCount()) !== held) throw new Error("Messages were deleted while paused.");
    await page.getByRole("button", { name: "Resume" }).click();
    await page.getByRole("button", { name: "Pause" }).waitFor();
  });

  await step("finishes the sweep", async () => {
    await page.getByText("Finished.", { exact: false }).first().waitFor({ timeout: 180_000 });
    const finished = await deletedCount();
    if (finished < 1) throw new Error("Nothing was deleted.");
    console.log(`       previewed ${expected}, deleted ${finished}`);
  });

  await step("records it in Activity", async () => {
    await nav(page, "Activity");
    await page.getByText("Mira and 1 more").first().waitFor();
    await page.getByRole("img", { name: /Bar chart/ }).waitFor();
    await Bun.sleep(300);
    await shot(page, "activity");
  });

  await step("adds an autosweep rule", async () => {
    await nav(page, "Autosweep");
    await page.getByRole("button", { name: "New rule" }).click();
    await page.getByRole("dialog").getByRole("button", { name: /Noor/ }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Turn on" }).click();
    await page.getByText("Deletes your messages older than 1 month").waitFor();
  });

  await step("runs the rule now", async () => {
    await page.getByRole("button", { name: "Run now" }).click();
    await page.getByRole("heading", { name: "Noor", exact: true }).waitFor();
    await page.getByText("Finished.", { exact: false }).first().waitFor({ timeout: 120_000 });
  });

  await step("sets up the bot and runs a command", async () => {
    await nav(page, "Bot");
    await page.getByRole("button", { name: "Use the demo bot" }).click();
    await page.getByRole("button", { name: "Set up automatically" }).click();
    await page.getByRole("button", { name: "Run setup again" }).waitFor();
    await page.getByRole("switch", { name: "Bot online" }).click();
    await page.getByText("Online (simulated)").waitFor();
    await page.getByRole("button", { name: "/sweep here" }).click();
    await page.getByText(/Previewed .* messages in Mira/).waitFor();
    await page.getByRole("heading", { name: "Sweep from inside Discord" }).evaluate((heading) => {
      heading.scrollIntoView();
    });
    await Bun.sleep(200);
    await shot(page, "bot");
  });

  await step("switches a bot command off", async () => {
    await page.getByRole("switch", { name: "/purge" }).click();
    await page.waitForFunction(() =>
      Array.from(document.querySelectorAll("[role=switch]")).some(
        (item) =>
          item.getAttribute("aria-label") === "/purge" &&
          item.getAttribute("aria-checked") === "false",
      ),
    );
  });

  await step("streamer mode blurs names", async () => {
    await nav(page, "Settings");
    await page.getByRole("switch", { name: "Streamer mode" }).click();
    const streamer = await page.evaluate(() => document.documentElement.dataset.streamer);
    if (streamer !== "true") throw new Error("Streamer mode didn't turn on.");
    await page.getByRole("switch", { name: "Streamer mode" }).click();
  });

  await step("the light theme renders", async () => {
    await page.getByRole("radio", { name: "Light" }).click();
    await Bun.sleep(200);

    if (!updateDocs) await shot(page, "settings-light");
    await page.getByRole("radio", { name: "Dark" }).click();
  });

  await step("the interface logged no errors", async () => {
    if (problems.length > 0) throw new Error(problems.join("\n"));
  });

  await browser.close();
} finally {
  app.kill();
  await app.exited;

  await Bun.sleep(1500);
  if (!updateDocs) console.log(`\nScreenshots: ${shotFolder}`);
  else rmSync(profile, { recursive: true, force: true });
}

if (failed) process.exit(1);
console.log("\nAll flows passed.");
process.exit(0);
