// Reloads the page when a file in static/ changes (server.js GET /__live), so
// every save shows at once. Its own file, loaded first: a mistake in app.js
// stops the app and not this, and the save that fixes it still reloads the page.
(() => {
  let seen = null;
  setInterval(async () => {
    if (document.hidden) return;
    try {
      const { version } = await (await fetch('/__live', { cache: 'no-store' })).json();
      if (seen !== null && version !== seen) location.reload();
      seen = version;
    } catch {
      // The server is restarting, or a login page answered: ask again next tick.
    }
  }, 700);
})();
