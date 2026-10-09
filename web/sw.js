// Service worker: guarda a aplicação para abrir sem rede. A API nunca passa pelo cache.
// O servidor preenche a versão e a lista de arquivos ao servir este arquivo.
const VERSAO = '__VERSAO__';
const ARQUIVOS = "__ARQUIVOS__";
const CACHE = `rubricas-${VERSAO}`;

self.addEventListener('install', (e) => {
  e.waitUntil(
    caches.open(CACHE)
      .then((c) => c.addAll(Array.isArray(ARQUIVOS) ? ARQUIVOS : ['./']))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches.keys()
      .then((nomes) => Promise.all(nomes.filter((n) => n.startsWith('rubricas-') && n !== CACHE).map((n) => caches.delete(n))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== 'GET' || url.origin !== location.origin) return;
  const escopo = new URL('./', self.location).pathname;
  if (url.pathname.startsWith(escopo + 'api/')) return;
  // As telas ficam depois do `#`: a única página é a raiz. O resto vai ao servidor.
  if (e.request.mode === 'navigate' && url.pathname !== escopo) return;
  e.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const pedido = e.request.mode === 'navigate' ? './' : e.request;
    const guardado = await cache.match(pedido, { ignoreSearch: true });
    if (guardado) return guardado;
    return fetch(e.request);
  })());
});
