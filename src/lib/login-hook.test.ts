import { describe, expect, it, vi } from "vitest";
import source from "../../src-tauri/src/login_hook.js?raw";

const TOKEN = "MTA1NjQ3.GZk2Lw.abcdefghijklmnopqrstuvwxyz";

class FakeHeaders {
  constructor(private values: Record<string, string> = {}) {}

  get(name: string) {
    return this.values[name.toLowerCase()] ?? null;
  }
}

class FakeRequest {
  headers = new FakeHeaders();
}

function load(hostname = "discord.com", isTopFrame = true) {
  const replace = vi.fn();
  const originalSetHeader = vi.fn();
  const originalFetch = vi.fn(() => "fetched");

  class FakeXhr {
    setRequestHeader(name: string, value: string) {
      originalSetHeader(name, value);
    }
  }

  const saved: { token?: string } = {};
  let poll: () => void = () => {};
  const fakeWindow: Record<string, unknown> = {
    location: { hostname, replace },
    fetch: originalFetch,
    localStorage: { getItem: (key: string) => (key === "token" ? (saved.token ?? null) : null) },
    setInterval: (callback: () => void) => {
      poll = callback;
      return 1;
    },
    clearInterval: vi.fn(),
  };
  fakeWindow.top = isTopFrame ? fakeWindow : {};

  new Function("window", "XMLHttpRequest", "Headers", "Request", source)(
    fakeWindow,
    FakeXhr,
    FakeHeaders,
    FakeRequest,
  );

  return {
    replace,
    originalSetHeader,
    originalFetch,
    xhr: new FakeXhr(),
    fetch: fakeWindow.fetch as (...args: unknown[]) => unknown,
    saved,
    poll: () => poll(),
  };
}

describe("the sign-in hook", () => {
  it("delivers the token from an XHR header", () => {
    const page = load();
    page.xhr.setRequestHeader("Authorization", TOKEN);
    expect(page.replace).toHaveBeenCalledOnce();
    expect(page.replace.mock.calls[0]?.[0]).toBe(
      `https://disweep-auth.invalid/__NONCE__?token=${encodeURIComponent(TOKEN)}`,
    );
  });

  it.each([
    ["a plain object", { Authorization: TOKEN }],
    ["a list of pairs", [["authorization", TOKEN]]],
    ["a Headers object", new FakeHeaders({ authorization: TOKEN })],
  ])("delivers the token from fetch headers given as %s", (_, headers) => {
    const page = load();
    page.fetch("https://discord.com/api/v10/users/@me", { headers });
    expect(page.replace).toHaveBeenCalledOnce();
  });

  it("still lets the page's own requests through", () => {
    const page = load();
    page.xhr.setRequestHeader("Authorization", TOKEN);
    expect(page.originalSetHeader).toHaveBeenCalledWith("Authorization", TOKEN);
    expect(page.fetch("https://discord.com/x", { headers: {} })).toBe("fetched");
  });

  it("delivers only once", () => {
    const page = load();
    page.xhr.setRequestHeader("Authorization", TOKEN);
    page.xhr.setRequestHeader("Authorization", TOKEN);
    expect(page.replace).toHaveBeenCalledOnce();
  });

  it("ignores other headers and values that can't be tokens", () => {
    const page = load();
    page.xhr.setRequestHeader("X-Fingerprint", TOKEN);
    page.xhr.setRequestHeader("Authorization", "undefined");
    page.xhr.setRequestHeader("Authorization", "");
    expect(page.replace).not.toHaveBeenCalled();
  });

  it("falls back to the token saved in local storage", () => {
    const page = load();
    page.poll();
    expect(page.replace).not.toHaveBeenCalled();

    page.saved.token = JSON.stringify(TOKEN);
    page.poll();
    expect(page.replace).toHaveBeenCalledOnce();
  });

  it("survives malformed saved data", () => {
    const page = load();
    page.saved.token = "{not json";
    expect(() => page.poll()).not.toThrow();
    expect(page.replace).not.toHaveBeenCalled();
  });

  it("does nothing on any other site or inside a frame", () => {
    const elsewhere = load("evil.example");
    elsewhere.xhr.setRequestHeader("Authorization", TOKEN);
    expect(elsewhere.replace).not.toHaveBeenCalled();

    const framed = load("discord.com", false);
    framed.xhr.setRequestHeader("Authorization", TOKEN);
    expect(framed.replace).not.toHaveBeenCalled();
  });
});
