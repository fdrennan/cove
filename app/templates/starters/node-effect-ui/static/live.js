// Reloads the page when the app changes (GET __live, beside the page), so every save shows at
// once. Its own file, so a mistake in the page's script cannot stop it.
(() => {
  let seen = null;
  setInterval(async () => {
    if (document.hidden) return;
    try {
      const { version } = await (await fetch('__live', { cache: 'no-store' })).json();
      if (seen !== null && version !== seen) location.reload();
      seen = version;
    } catch {
      // The server is restarting, or a login page answered: ask again next tick.
    }
  }, 700);
})();
