({ origin, token, headers, version }) => {
  if (window.top !== window || location.origin !== origin) return;

  const locale = 'zh_CN';
  Object.defineProperty(window, 'dsh', {
    value: Object.freeze({
      protocolVersion: 1,
      displayMode: 'embedded',
      getAuthToken: () => token,
      getLocale: () => locale,
      onLocaleChange: () => () => {},
    }),
  });

  if (!headers) return;

  const requestHeaders = { ...headers,
    'x-client-bundle-id': '',
    'x-client-platform': /Win/.test(navigator.platform) ? 'desktop-win'
      : /Mac/.test(navigator.platform) ? 'desktop-mac' : 'web',
    'x-client-version': version,
    'x-client-locale': locale,
    'x-client-timezone-offset': String(-new Date().getTimezoneOffset() * 60),
  };
  const send = window.fetch.bind(window);
  window.fetch = async (input, options) => {
    const request = new Request(input, options);
    if (new URL(request.url).origin !== origin) return send(request);
    // The browser checks the origin at each hop, before dispatching the request.
    const scoped = new Request(request, { mode: 'same-origin' });
    for (const [name, value] of Object.entries(requestHeaders)) scoped.headers.set(name, value);
    return send(scoped);
  };
}
