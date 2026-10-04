'use strict';

// 記録のlabel等はクライアントが自由に入れられるので、DOMはtextContentだけで組み立てる

const TOKEN_KEY = 'upstream-save-viewer-token';

let token = loadToken();
// 一覧のPromise。トークンを変えたら捨てるので、古いトークンでの応答はここに戻らない
let summaries = null;

function loadToken() {
  try {
    return localStorage.getItem(TOKEN_KEY) || '';
  } catch {
    return '';
  }
}

function saveToken(t) {
  token = t;
  summaries = null;
  filters.label = filters.client = filters.af = null;
  try {
    if (t) {
      localStorage.setItem(TOKEN_KEY, t);
    } else {
      localStorage.removeItem(TOKEN_KEY);
    }
  } catch {
    // 保存できなくても、このタブを開いている間はメモリ上の値で読める
  }
}

function el(tag, props, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (k === 'class') {
      e.className = v;
    } else if (k.startsWith('on')) {
      e.addEventListener(k.slice(2), v);
    } else {
      e.setAttribute(k, v);
    }
  }
  for (const c of children.flat(Infinity)) {
    if (c === null || c === undefined || c === false) continue;
    e.append(c instanceof Node ? c : String(c));
  }
  return e;
}

class ApiError extends Error {}

async function api(path, init = {}) {
  const headers = token ? { 'x-token': token } : {};
  const res = await fetch(path, { ...init, headers });
  if (res.ok) return res.json();
  if (res.status === 401) {
    throw new ApiError(token ? 'トークンが閲覧用として通らない。貼り直すか消すこと' : '閲覧用トークンが要る');
  }
  if (res.status === 404) throw new ApiError('見つからない（非公開の記録はトークン無しでは見えない）');
  if (res.status === 429) throw new ApiError('混んでいる。少し待って再読み込みすること');
  throw new ApiError(`HTTP ${res.status}`);
}

function getSummaries() {
  if (!summaries) {
    const p = api('/api/traces');
    summaries = p;
    p.catch(() => {
      if (summaries === p) summaries = null;
    });
  }
  return summaries;
}

function formatTs(ts) {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleString('ja-JP', {
    year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit', second: '2-digit',
  });
}

function asLink(as) {
  const m = /^AS(\d+)$/.exec(as);
  if (!m) return el('span', {}, as);
  return el('a', { href: `https://bgp.he.net/AS${m[1]}`, target: '_blank', rel: 'noopener noreferrer' }, as);
}

function asnLink(asn) {
  return asn === undefined || asn === null ? '' : asLink(`AS${asn}`);
}

function asPath(path) {
  if (!path.length) return el('span', { class: 'muted' }, '(なし)');
  const parts = [];
  path.forEach((as, i) => {
    if (i > 0) parts.push(el('span', { class: 'sep' }, '→'));
    parts.push(asLink(as));
  });
  return el('span', { class: 'path mono' }, parts);
}

function traceLink(key, text) {
  return el('a', { href: `#/t/${key}` }, text);
}

function afText(af) {
  return af ? `v${af}` : '';
}

function num(v, digits = 1) {
  return v === undefined || v === null ? '' : Number(v).toFixed(digits);
}

function table(head, rows) {
  return el('div', { class: 'scroll' },
    el('table', {},
      el('thead', {}, el('tr', {}, head.map(([label, cls]) => el('th', cls ? { class: cls } : {}, label)))),
      el('tbody', {}, rows)));
}

// 一覧

// nullは絞り込まない。''は値が無いものに絞る
const filters = { label: null, client: null, af: null };

function uniq(values) {
  return [...new Set(values)].sort();
}

function filterSelect(name, label, values) {
  if (!values.includes(filters[name])) filters[name] = null;
  const select = el('select', {
    onchange: (ev) => {
      const i = ev.target.selectedIndex;
      filters[name] = i === 0 ? null : values[i - 1];
      render();
    },
  },
  el('option', {}, 'すべて'),
  values.map((v) => {
    const o = el('option', {}, v === '' ? '(なし)' : v);
    if (filters[name] === v) o.selected = true;
    return o;
  }));
  return el('label', {}, `${label} `, select);
}

async function renderList(main) {
  const all = await getSummaries();
  const labelOf = (s) => s.label ?? '';
  const afOf = (s) => afText(s.af);
  const selects = [
    filterSelect('label', 'label', uniq(all.map(labelOf))),
    filterSelect('client', 'client', uniq(all.map((s) => s.client))),
    filterSelect('af', 'af', uniq(all.map(afOf))),
  ];
  const shown = all.filter((s) =>
    (filters.label === null || labelOf(s) === filters.label) &&
    (filters.client === null || s.client === filters.client) &&
    (filters.af === null || afOf(s) === filters.af));

  main.append(
    el('div', { class: 'filters' },
      selects,
      el('span', { class: 'muted' }, `${shown.length} / ${all.length} 件`)),
    table(
      [['日時'], ['label'], ['client'], ['af'], ['target'], ['出口'], ['AS path'], ['hop', 'num'], ['']],
      shown.map((s) => el('tr', {},
        el('td', {}, traceLink(s.key, formatTs(s.ts))),
        el('td', {}, s.label ?? ''),
        el('td', {}, s.client),
        el('td', {}, afText(s.af)),
        el('td', {}, s.target ?? ''),
        el('td', { class: 'wrap' }, s.source ? [asnLink(s.source.asn), ' ', el('span', { class: 'muted' }, s.source.holder)] : ''),
        el('td', { class: 'wrap' }, asPath(s.as_path)),
        el('td', { class: 'num' }, s.hop_count),
        el('td', {}, s.public ? el('span', { class: 'badge' }, '公開') : '')))),
  );
  if (!all.length) {
    main.append(el('p', { class: 'muted' }, token ? '記録が無い' : '公開された記録が無い'));
  }
}

// 詳細

async function renderTrace(main, key) {
  const t = await api(`/api/traces/${key}`);

  const rows = [
    ['日時', formatTs(t.ts)],
    ['client', t.client],
    ['label', t.label ?? ''],
    ['target', t.target ?? ''],
    ['af', afText(t.af)],
    ['形式', t.format],
    ['出口', t.source ? [asnLink(t.source.asn), ' ', t.source.holder, ' ', el('span', { class: 'mono' }, t.source.prefix)] : ''],
    ['送信元', el('span', { class: 'mono' }, t.source_ip ?? '')],
    ['AS path', asPath(t.as_path)],
  ];
  if (t.lookup_failed?.length) {
    rows.push(['補完の失敗', el('span', { class: 'mono' }, t.lookup_failed.join(' '))]);
  }

  main.append(
    el('h2', {}, `${formatTs(t.ts)} ${t.label ?? ''}`.trim()),
    el('dl', {}, rows.map(([k, v]) => [el('dt', {}, k), el('dd', {}, v)])),
  );

  if (token) {
    const status = el('span', { class: 'muted' });
    const button = el('button', {
      type: 'button',
      onclick: async () => {
        button.disabled = true;
        try {
          const res = await api(`/api/traces/${key}/public`, { method: t.public ? 'DELETE' : 'PUT' });
          t.public = res.public;
          summaries = null;
          update();
        } catch (e) {
          status.textContent = e.message;
        } finally {
          button.disabled = false;
        }
      },
    });
    const update = () => {
      button.textContent = t.public ? '非公開にする' : '公開する';
      status.textContent = t.public ? 'トークン無しでも見える' : '閲覧用トークンでだけ見える';
    };
    update();
    main.append(el('p', {}, button, ' ', status));
  } else if (t.public) {
    main.append(el('p', {}, el('span', { class: 'badge' }, '公開')));
  }

  const showStats = t.hops.some((h) => h.stats);
  const head = [['hop', 'num'], ['IP'], ['名前'], ['AS'], ['holder'], ['prefix']];
  if (showStats) {
    head.push(['loss%', 'num'], ['avg', 'num'], ['best', 'num'], ['wrst', 'num'], ['snt', 'num']);
  } else {
    head.push(['RTT (ms)'], ['*', 'num']);
  }

  let prevHop = null;
  const hopRows = t.hops.map((h) => {
    const same = h.hop === prevHop;
    prevHop = h.hop;
    const asn = h.asn ?? h.asn_reported;
    const cells = [
      el('td', { class: 'num' }, same ? '' : h.hop),
      el('td', { class: 'mono' }, h.ip ?? '*'),
      el('td', { class: 'mono' }, h.host ?? ''),
      el('td', {}, asnLink(asn)),
      el('td', { class: 'wrap' }, h.holder ?? ''),
      el('td', { class: 'mono' }, h.prefix ?? ''),
    ];
    if (showStats) {
      const s = h.stats;
      cells.push(
        el('td', { class: 'num' }, s ? num(s.loss) : ''),
        el('td', { class: 'num' }, s ? num(s.avg) : ''),
        el('td', { class: 'num' }, s ? num(s.best) : ''),
        el('td', { class: 'num' }, s ? num(s.wrst) : ''),
        el('td', { class: 'num' }, s ? s.snt : ''),
      );
    } else {
      cells.push(
        el('td', { class: 'mono' }, (h.rtts ?? []).map((v) => num(v, 3)).join(' ')),
        el('td', { class: 'num' }, h.timeouts ?? ''),
      );
    }
    return el('tr', same ? { class: 'secondary' } : {}, cells);
  });

  main.append(el('h2', {}, 'hop'), table(head, hopRows));
}

// 比較

async function renderCompare(main) {
  const all = await getSummaries();
  const groups = new Map();
  for (const s of all) {
    // 表示用の文字列をキーにすると、labelが実際に「(label なし)」の記録と混ざる
    const g = JSON.stringify([s.label ?? null, s.af ?? null]);
    if (!groups.has(g)) groups.set(g, new Map());
    const paths = groups.get(g);
    const p = s.as_path.join(' ');
    if (!paths.has(p)) paths.set(p, { path: s.as_path, items: [] });
    paths.get(p).items.push(s);
  }

  if (!groups.size) {
    main.append(el('p', { class: 'muted' }, token ? '記録が無い' : '公開された記録が無い'));
    return;
  }

  const title = (g) => {
    const [label, af] = JSON.parse(g);
    return `${label ?? '(label なし)'} / ${afText(af) || 'af 不明'}`;
  };
  for (const g of [...groups.keys()].sort((a, b) => title(a).localeCompare(title(b)))) {
    // summariesはtsの降順なので、items[0]が最新でitems.at(-1)が最古
    const paths = [...groups.get(g).values()].sort((a, b) =>
      b.items.length - a.items.length || b.items[0].ts.localeCompare(a.items[0].ts));
    main.append(el('div', { class: 'group' },
      el('h2', {}, title(g)),
      table(
        [['AS path'], ['件数', 'num'], ['初出'], ['最終']],
        paths.map((p) => el('tr', {},
          el('td', { class: 'wrap' }, asPath(p.path)),
          el('td', { class: 'num' }, p.items.length),
          el('td', {}, traceLink(p.items.at(-1).key, formatTs(p.items.at(-1).ts))),
          el('td', {}, traceLink(p.items[0].key, formatTs(p.items[0].ts))))))));
  }
}

// 画面の切り替え

function renderAuth() {
  document.getElementById('auth-status').textContent = token ? '閲覧用トークンで表示中' : '公開分を表示中';
  document.getElementById('token').value = '';
  document.getElementById('token-clear').hidden = !token;
}

let renderSeq = 0;

async function render() {
  const seq = ++renderSeq;
  const hash = location.hash.replace(/^#/, '') || '/';
  const main = el('main', { id: 'main' });
  document.getElementById('nav-list').classList.toggle('current', hash === '/');
  document.getElementById('nav-compare').classList.toggle('current', hash === '/compare');
  renderAuth();

  try {
    if (hash === '/compare') {
      await renderCompare(main);
    } else if (hash.startsWith('/t/')) {
      await renderTrace(main, hash.slice(3));
    } else {
      await renderList(main);
    }
  } catch (e) {
    main.replaceChildren(el('p', { class: 'error' }, e instanceof ApiError ? e.message : `読み込みに失敗した: ${e}`));
  }
  if (seq === renderSeq) document.getElementById('main').replaceWith(main);
}

document.getElementById('auth').addEventListener('submit', (ev) => {
  ev.preventDefault();
  const v = document.getElementById('token').value.trim();
  if (v) saveToken(v);
  render();
});
document.getElementById('token-clear').addEventListener('click', () => {
  saveToken('');
  document.getElementById('main').replaceChildren();
  render();
});
window.addEventListener('hashchange', render);
render();
