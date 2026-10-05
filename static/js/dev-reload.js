// `just dev`'s reload (web::dev_reload). The stream names the server process;
// the page reloads only when that name changes. A reconnect to the same
// process — a proxy cutting the stream, a laptop waking — is not a restart.
(() => {
  let boot = null;
  let source = null;
  const open = () => {
    source = new EventSource('/_dev/reload');
    source.addEventListener('boot', (e) => {
      if (boot === null) boot = e.data;
      else if (e.data !== boot) {
        source.close();
        location.reload();
      }
    });
  };
  addEventListener('pageshow', () => { if (!source) open(); });
  addEventListener('pagehide', () => { source?.close(); source = null; });
})();
