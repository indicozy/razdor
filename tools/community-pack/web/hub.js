/* The community hub page: browses index.json the way the game's «Карты и моды» window does
   (src/ui/hub_view.rs, src/hub.rs). Plain JavaScript, no framework, no external resources.
   Catalog text only ever reaches the page through textContent or property assignment. */
(function () {
  'use strict';

  var STRINGS = {
    ru: {
      pageTitle: 'Карты и моды · Раздор',
      title: 'Карты и моды',
      switchLang: 'English',
      loading: 'Читаю каталог…',
      errorTitle: 'Каталог не открылся',
      errorIntro: 'Каталог не читается: ',
      errFetch: 'нет связи или рядом со страницей нет файла index.json',
      errNotJson: 'index.json не JSON (так обычно отвечает страница «не найдено»)',
      errFormat: 'каталог в формате, который эта страница не читает',
      errorGithelp: 'Открыть каталог на GitHub:',
      githubLink: 'Каталог на GitHub',
      retry: 'Повторить',
      listHeading: 'Список',
      tabAll: 'Все',
      tabMap: 'Карты',
      tabCampaign: 'Кампании',
      tabMod: 'Моды',
      search: 'Поиск…',
      empty: 'Здесь пока пусто.',
      noMatch: 'Ничего не подходит к поиску.',
      backToList: 'Назад к списку',
      notFound: 'Записи «{id}» нет в каталоге.',
      kindMap: 'Карта',
      kindCampaign: 'Кампания',
      kindMod: 'Мод',
      byLine: '{kind}, автор: {author}',
      byLineVersion: '{kind}, автор: {author}, версия {version}',
      rating: 'Оценка в Discord: +{up} / -{down}',
      statusFull: 'Работает полностью',
      statusPartial: 'Работает частично',
      statusPartialNote: 'Работает частично: {note}',
      statusBroken: 'Не запускается',
      statusBrokenNote: 'Не запускается: {note}',
      statusUnknown: 'Статус ещё не проверен',
      chapters: 'Глав: {n}',
      languages: 'Языки: {list}',
      langRu: 'русский',
      langEn: 'английский',
      langEnMt: 'английский (машинный перевод)',
      base: 'База: {base}',
      needs: 'Нужно:',
      notInHub: 'нет в хабе',
      needsMod: 'Нужен мод: {names}',
      needsMods: 'Нужны моды: {names}',
      cannotInstall: 'Не установить',
      cannotMissing: 'Не установить: в хабе нет мода «{m}».',
      cannotCycle: 'Не установить: моды требуют друг друга («{m}»).',
      neededBy: 'Нужен для: {names}',
      fromPost: 'Из поста автора',
      noDescription: 'Описания нет.',
      modNote: 'Мод играют из списка «Мод» в окне новой игры, поверх базовой игры.',
      installHeading: 'Как установить',
      installGame: 'Проще всего установить из игры: Раздор → «Карты и моды». Игра скачает и проверит файлы и поставит нужные моды.',
      installByHand: 'Вручную:',
      manualMaps: 'положите файлы карт (.DTm) в папку maps внутри папки данных Раздора: Windows — %APPDATA%\\razdor\\hub\\maps, Linux — ~/.local/share/razdor/hub/maps, macOS — ~/Library/Application Support/razdor/hub/maps. Кампании нужны все её файлы.',
      manualNeedsMods: 'Этой карте нужны моды. Вручную их не поставить: установите их из игры.',
      manualMod: 'Мод вручную не ставится: он накладывается на базовую установку, поэтому установите его из игры.',
      files: 'Файлы',
      download: 'Скачать',
      archive: 'архив',
      sizeKb: '{n} КБ',
      sizeMb: '{n} МБ',
      footer: 'Страница читает index.json из своей папки. Без сторонних скриптов и слежения.'
    },
    en: {
      pageTitle: 'Maps and mods · Razdor',
      title: 'Maps and mods',
      switchLang: 'Русский',
      loading: 'Reading the catalog…',
      errorTitle: 'The catalog did not open',
      errorIntro: 'The catalog can’t be read: ',
      errFetch: 'no connection, or there is no index.json next to this page',
      errNotJson: 'index.json is not JSON (a “not found” page usually looks like this)',
      errFormat: 'the catalog is in a format this page does not read',
      errorGithelp: 'Open the catalog on GitHub:',
      githubLink: 'The catalog on GitHub',
      retry: 'Try again',
      listHeading: 'List',
      tabAll: 'All',
      tabMap: 'Maps',
      tabCampaign: 'Campaigns',
      tabMod: 'Mods',
      search: 'Search…',
      empty: 'Nothing here yet.',
      noMatch: 'Nothing matches the search.',
      backToList: 'Back to the list',
      notFound: 'No entry «{id}» in the catalog.',
      kindMap: 'Map',
      kindCampaign: 'Campaign',
      kindMod: 'Mod',
      byLine: '{kind} by {author}',
      byLineVersion: '{kind} by {author}, version {version}',
      rating: 'Rating on Discord: +{up} / -{down}',
      statusFull: 'Works fully',
      statusPartial: 'Works partly',
      statusPartialNote: 'Works partly: {note}',
      statusBroken: 'Does not start',
      statusBrokenNote: 'Does not start: {note}',
      statusUnknown: 'Not checked yet',
      chapters: 'Chapters: {n}',
      languages: 'Languages: {list}',
      langRu: 'Russian',
      langEn: 'English',
      langEnMt: 'English (machine translation)',
      base: 'Base: {base}',
      needs: 'Needs:',
      notInHub: 'not in the hub',
      needsMod: 'Needs mod: {names}',
      needsMods: 'Needs mods: {names}',
      cannotInstall: 'Can’t be installed',
      cannotMissing: 'Can’t be installed: the hub has no mod «{m}».',
      cannotCycle: 'Can’t be installed: mods need each other («{m}»).',
      neededBy: 'Needed by: {names}',
      fromPost: 'From the author’s post',
      noDescription: 'No description.',
      modNote: 'A mod is played from the new game window’s Mod list, over the base game.',
      installHeading: 'How to install',
      installGame: 'Easiest: in the game, Razdor → “Maps and mods”. The game downloads and checks the files and installs the mods they need.',
      installByHand: 'By hand:',
      manualMaps: 'put the map files (.DTm) into the maps folder inside Razdor’s data folder: Windows — %APPDATA%\\razdor\\hub\\maps, Linux — ~/.local/share/razdor/hub/maps, macOS — ~/Library/Application Support/razdor/hub/maps. A campaign needs all of its files.',
      manualNeedsMods: 'This map needs mods. They can’t be set up by hand: install them in the game.',
      manualMod: 'A mod is not installed by hand: it is laid over the base install, so install it in the game.',
      files: 'Files',
      download: 'Download',
      archive: 'archive',
      sizeKb: '{n} KB',
      sizeMb: '{n} MB',
      footer: 'The page reads index.json from its own folder. No third-party scripts, no tracking.'
    }
  };

  var LANG_KEY = 'razdor-hub-lang';
  // The zipped catalog first (a map is a zip of its file); the bare one of the same folder when it is missing.
  var CATALOG_URLS = ['index-v2.json', 'index.json'];
  var GITHUB_CATALOG = 'https://github.com/indicozy/razdor-community/releases/latest';
  var KINDS = ['map', 'campaign', 'mod'];
  var KIND_KEY = { map: 'kindMap', campaign: 'kindCampaign', mod: 'kindMod' };
  // The tabs, as the window's: all, maps, campaigns, mods (null = every kind).
  var TABS = [
    { key: 'tabAll', kind: null },
    { key: 'tabMap', kind: 'map' },
    { key: 'tabCampaign', kind: 'campaign' },
    { key: 'tabMod', kind: 'mod' }
  ];
  var narrow = window.matchMedia('(max-width: 700px)');

  var lang = readLang();
  var catalog = null;          // { format, entries } after checkCatalog
  var byId = new Map();        // id -> entry
  var dependents = new Map();  // id -> entries that require it
  var tab = 0;
  var query = '';
  var pick = null;             // id shown in the details pane
  var lastError = null;        // error key, kept for a language switch
  var FILES_BASE = filesBase();

  // ---- small helpers -------------------------------------------------------

  function readLang() {
    try {
      var v = window.localStorage.getItem(LANG_KEY);
      if (v === 'en' || v === 'ru') return v;
    } catch (e) { /* private window or blocked storage: Russian */ }
    return 'ru';
  }

  function saveLang(code) {
    try { window.localStorage.setItem(LANG_KEY, code); } catch (e) { /* no storage: keep going */ }
  }

  function t(key, vars) {
    var table = STRINGS[lang] || STRINGS.ru;
    var s = Object.prototype.hasOwnProperty.call(table, key) ? table[key] : STRINGS.ru[key];
    if (s === undefined) return key;
    if (vars) {
      s = s.replace(/\{(\w+)\}/g, function (m, k) {
        return Object.prototype.hasOwnProperty.call(vars, k) ? String(vars[k]) : m;
      });
    }
    return s;
  }

  // A catalog text {ru, en} in the page's language; the other language when this one is empty.
  function text(obj) {
    if (!obj) return '';
    var ru = str(obj.ru);
    var en = str(obj.en);
    return lang === 'en' ? (en || ru) : (ru || en);
  }

  function str(v) {
    return typeof v === 'string' ? v.trim() : '';
  }

  function el(tag, cls, content) {
    var n = document.createElement(tag);
    if (cls) n.className = cls;
    if (content !== undefined && content !== null) n.textContent = content;
    return n;
  }

  function $(id) {
    return document.getElementById(id);
  }

  // The folder the catalog's relative paths resolve against: the optional meta, else this page's folder.
  function filesBase() {
    var meta = document.querySelector('meta[name="razdor-files-base"]');
    var raw = meta ? str(meta.getAttribute('content')) : '';
    var base = raw ? new URL(raw, document.baseURI).href : new URL('.', document.baseURI).href;
    return base.replace(/\/+$/, '');
  }

  // As src/hub.rs `resolve`: an https:// path as it is, else joined under the base.
  function resolve(rel) {
    if (rel.indexOf('https://') === 0) return rel;
    return FILES_BASE + '/' + rel.replace(/^\/+/, '');
  }

  function entryHref(id) {
    return '#' + encodeURIComponent(id);
  }

  function baseName(name) {
    var parts = String(name || '').split('/');
    return parts[parts.length - 1] || name;
  }

  function sizeText(bytes) {
    var n = Number(bytes) || 0;
    if (n >= 1024 * 1024) return t('sizeMb', { n: (n / (1024 * 1024)).toFixed(1) });
    return t('sizeKb', { n: Math.max(1, Math.round(n / 1024)) });
  }

  function langLabel(code, machine) {
    if (code === 'ru') return t('langRu');
    if (code === 'en') return machine ? t('langEnMt') : t('langEn');
    return code;
  }

  // ---- catalog -------------------------------------------------------------

  // Checks the catalog's shape (as the game's reader does) and fills in the defaults the page uses.
  // Returns an error key, or null.
  function checkCatalog(data) {
    if (!data || typeof data !== 'object' || Array.isArray(data)) return 'errFormat';
    if (data.format !== 1 || !Array.isArray(data.entries)) return 'errFormat';
    for (var i = 0; i < data.entries.length; i++) {
      var e = data.entries[i];
      if (!e || typeof e.id !== 'string' || KINDS.indexOf(e.kind) < 0 || !Array.isArray(e.files)) return 'errFormat';
      e.title = e.title || {};
      e.description = e.description || {};
      e.about = e.about || {};
      e.status_note = e.status_note || {};
      e.author = str(e.author);
      e.version = str(e.version);
      e.added = str(e.added);
      e.base = str(e.base);
      e.requires = Array.isArray(e.requires) ? e.requires.filter(function (r) { return typeof r === 'string'; }) : [];
      e.languages = Array.isArray(e.languages) ? e.languages : [];
      e.machine_translated = e.machine_translated === true;
      e.status = ['full', 'partial', 'broken'].indexOf(e.status) >= 0 ? e.status : 'unknown';
      e.rating = e.rating && typeof e.rating.up === 'number' && typeof e.rating.down === 'number'
        ? { up: e.rating.up, down: e.rating.down }
        : null;
      e.picture = e.picture && typeof e.picture.path === 'string' ? e.picture : null;
      e.preview = e.preview && typeof e.preview.path === 'string' ? e.preview : null;
      if (!byId.has(e.id)) byId.set(e.id, e);
    }
    return null;
  }

  function buildDependents() {
    dependents = new Map();
    catalog.entries.forEach(function (e) {
      e.requires.forEach(function (r) {
        if (!dependents.has(r)) dependents.set(r, []);
        dependents.get(r).push(e);
      });
    });
  }

  // The mods an entry needs, dependencies first (as hub::plan_install): null when they can all be
  // installed, else { missing: id } or { cycle: id }.
  function planProblem(id) {
    var done = new Set();
    var busy = new Set();
    function visit(i) {
      var e = byId.get(i);
      if (!e) return { missing: i };
      if (done.has(i)) return null;
      if (busy.has(i)) return { cycle: i };
      busy.add(i);
      for (var k = 0; k < e.requires.length; k++) {
        var problem = visit(e.requires[k]);
        if (problem) return problem;
      }
      busy.delete(i);
      done.add(i);
      return null;
    }
    return visit(id);
  }

  function titleOf(id) {
    var e = byId.get(id);
    return e ? (text(e.title) || e.id) : id;
  }

  // The list's order: rated entries first, by score, then votes; the rest by title (as hub::by_rating).
  function titleCompare(a, b) {
    var x = str(a.title.ru).toLowerCase();
    var y = str(b.title.ru).toLowerCase();
    return x < y ? -1 : x > y ? 1 : 0;
  }

  function byRating(a, b) {
    var ra = a.rating;
    var rb = b.rating;
    if (ra && rb) {
      return (rb.up - rb.down) - (ra.up - ra.down) ||
        (rb.up + rb.down) - (ra.up + ra.down) ||
        titleCompare(a, b);
    }
    if (ra) return -1;
    if (rb) return 1;
    return titleCompare(a, b);
  }

  // The search: the Russian or English title, the author, or the title or id of a mod it needs.
  function matches(e, q) {
    var needle = q.trim().toLowerCase();
    if (!needle) return true;
    var texts = [str(e.title.ru), str(e.title.en)];
    e.requires.forEach(function (id) {
      texts.push(id);
      var m = byId.get(id);
      if (m) texts.push(str(m.title.ru), str(m.title.en));
    });
    for (var i = 0; i < texts.length; i++) {
      if (texts[i].toLowerCase().indexOf(needle) >= 0) return true;
    }
    return e.author.toLowerCase().indexOf(needle) >= 0;
  }

  // The files for a language, as Entry::files_for: those in it or with no language; the Russian ones when
  // the entry has none in that language.
  function filesFor(e, code) {
    var has = e.files.some(function (f) { return f.lang === code; });
    var want = has ? code : 'ru';
    return e.files.filter(function (f) { return !f.lang || f.lang === want; });
  }

  function mapCount(e) {
    return filesFor(e, 'ru').filter(function (f) {
      return /\.dtm$/i.test(String(f.name || ''));
    }).length;
  }

  function statusWords(e) {
    var note = text(e.status_note);
    if (e.status === 'full') return { text: t('statusFull'), cls: 'good' };
    if (e.status === 'partial') return { text: note ? t('statusPartialNote', { note: note }) : t('statusPartial'), cls: 'partial' };
    if (e.status === 'broken') return { text: note ? t('statusBrokenNote', { note: note }) : t('statusBroken'), cls: 'bad' };
    return { text: t('statusUnknown'), cls: 'dim' };
  }

  // ---- list ----------------------------------------------------------------

  function rowState(e) {
    if (!e.requires.length) return null;
    if (planProblem(e.id)) return { text: t('cannotInstall'), cls: 'bad' };
    var names = e.requires.map(titleOf);
    return { text: t(names.length === 1 ? 'needsMod' : 'needsMods', { names: names.join(', ') }), cls: 'partial' };
  }

  function rowNode(e, picked) {
    var li = el('li');
    var a = el('a', 'row');
    a.href = entryHref(e.id);
    if (picked) a.setAttribute('aria-current', 'true');
    var main = el('span', 'row-main');
    main.appendChild(el('span', 'row-title', text(e.title) || e.id));
    var sub = t(KIND_KEY[e.kind]) + ' · ' + (e.author || '—');
    if (e.rating) sub += ' · +' + e.rating.up + ' / -' + e.rating.down;
    main.appendChild(el('span', 'row-sub', sub));
    a.appendChild(main);
    var state = rowState(e);
    if (state) a.appendChild(el('span', 'row-state ' + state.cls, state.text));
    li.appendChild(a);
    return li;
  }

  function renderTabs() {
    var box = $('tabs');
    box.textContent = '';
    TABS.forEach(function (item, i) {
      var b = el('button', 'tab', t(item.key));
      b.type = 'button';
      b.setAttribute('aria-pressed', String(i === tab));
      b.addEventListener('click', function () {
        tab = i;
        renderTabs();
        renderList();
      });
      box.appendChild(b);
    });
  }

  function renderList() {
    if (!catalog) return;
    var kind = TABS[tab].kind;
    var list = catalog.entries
      .filter(function (e) { return !kind || e.kind === kind; })
      .filter(function (e) { return matches(e, query); })
      .sort(byRating);
    var rows = $('rows');
    rows.textContent = '';
    list.forEach(function (e) {
      rows.appendChild(rowNode(e, e.id === pick));
    });
    var empty = $('empty');
    empty.textContent = query.trim() ? t('noMatch') : t('empty');
    empty.hidden = list.length > 0;
  }

  // ---- details -------------------------------------------------------------

  function section(heading) {
    var s = el('section', 'section');
    s.appendChild(el('h3', null, heading));
    return s;
  }

  function linkList(entries) {
    var span = el('span');
    entries.forEach(function (m, i) {
      if (i) span.appendChild(document.createTextNode(', '));
      var a = el('a', null, text(m.title) || m.id);
      a.href = entryHref(m.id);
      span.appendChild(a);
    });
    return span;
  }

  function renderDetail(e) {
    var body = $('detail-body');
    body.textContent = '';
    if (!e) return;

    var picture = e.picture || e.preview;
    if (picture) {
      var box = el('div', 'picture');
      var img = document.createElement('img');
      img.loading = 'lazy';
      img.decoding = 'async';
      img.alt = text(e.title);
      img.onerror = function () { box.remove(); };
      img.src = resolve(picture.path);
      box.appendChild(img);
      body.appendChild(box);
    }

    var title = el('h2', 'title', text(e.title) || e.id);
    title.id = 'detail-title';
    title.tabIndex = -1;
    body.appendChild(title);

    var byLine = e.version
      ? t('byLineVersion', { kind: t(KIND_KEY[e.kind]), author: e.author || '—', version: e.version })
      : t('byLine', { kind: t(KIND_KEY[e.kind]), author: e.author || '—' });
    if (e.added) byLine += ' · ' + e.added;
    body.appendChild(el('p', 'meta', byLine));
    if (e.rating) body.appendChild(el('p', 'meta', t('rating', { up: e.rating.up, down: e.rating.down })));
    var status = statusWords(e);
    body.appendChild(el('p', 'meta ' + status.cls, status.text));
    if (e.kind === 'campaign') body.appendChild(el('p', 'meta', t('chapters', { n: mapCount(e) })));
    if (e.kind !== 'mod' && e.languages.length) {
      var names = e.languages.map(function (code) { return langLabel(code, e.machine_translated); });
      body.appendChild(el('p', 'meta', t('languages', { list: names.join(', ') })));
    }
    if (e.base) body.appendChild(el('p', 'meta', t('base', { base: e.base })));

    if (e.requires.length) {
      var needs = section(t('needs'));
      var list = el('ul', 'needs');
      e.requires.forEach(function (id) {
        var li = el('li');
        var m = byId.get(id);
        if (m) {
          var a = el('a', null, text(m.title) || m.id);
          a.href = entryHref(id);
          li.appendChild(a);
        } else {
          li.appendChild(el('span', 'missing', id + ' — ' + t('notInHub')));
        }
        list.appendChild(li);
      });
      needs.appendChild(list);
      var problem = planProblem(e.id);
      if (problem) {
        needs.appendChild(el('p', 'bad', problem.missing
          ? t('cannotMissing', { m: problem.missing })
          : t('cannotCycle', { m: problem.cycle })));
      }
      body.appendChild(needs);
    }

    var users = dependents.get(e.id) || [];
    if (users.length) {
      // The sentence is split around a marker, so the entries' links can sit where {names} is.
      var parts = t('neededBy', { names: '\u0001' }).split('\u0001');
      var neededBy = el('p', 'meta linked');
      neededBy.appendChild(document.createTextNode(parts[0]));
      neededBy.appendChild(linkList(users));
      neededBy.appendChild(document.createTextNode(parts[1] || ''));
      body.appendChild(neededBy);
    }

    var story = text(e.about);
    var post = text(e.description);
    var about = el('section', 'section');
    if (story) {
      about.appendChild(el('p', 'text', story));
      if (post) {
        about.appendChild(el('h3', null, t('fromPost')));
        about.appendChild(el('p', 'text small', post));
      }
    } else if (post) {
      about.appendChild(el('p', 'text', post));
    } else {
      about.appendChild(el('p', 'text small', t('noDescription')));
    }
    body.appendChild(about);
    if (e.kind === 'mod') body.appendChild(el('p', 'text small', t('modNote')));

    var install = section(t('installHeading'));
    var panel = el('div', 'install');
    panel.appendChild(el('p', null, t('installGame')));
    if (e.kind === 'mod') {
      panel.appendChild(el('p', 'text small', t('manualMod')));
    } else {
      var hand = el('p');
      hand.appendChild(el('strong', null, t('installByHand') + ' '));
      hand.appendChild(document.createTextNode(t('manualMaps')));
      panel.appendChild(hand);
      if (e.requires.length) panel.appendChild(el('p', 'text small', t('manualNeedsMods')));
    }
    install.appendChild(panel);
    body.appendChild(install);

    var files = section(t('files'));
    var ul = el('ul', 'files');
    e.files.forEach(function (f) {
      var li = el('li', 'file-line');
      var a = el('a', 'btn');
      a.href = resolve(f.path);
      // A zipped file (`wrapped`) downloads as its zip: the button says so, and the saved name is the zip's.
      var zipped = f.wrapped === 'zip';
      a.setAttribute('download', baseName(f.name) + (zipped ? '.zip' : ''));
      a.appendChild(el('span', null, t('download')));
      a.appendChild(el('span', null, baseName(f.name) + (zipped ? ' (.zip)' : '')));
      a.appendChild(el('span', 'size', sizeText(f.size)));
      li.appendChild(a);
      var notes = [];
      if (f.lang) notes.push(langLabel(f.lang, e.machine_translated));
      if (f.archive) notes.push(t('archive'));
      if (notes.length) li.appendChild(el('span', 'file-note', notes.join(', ')));
      ul.appendChild(li);
    });
    files.appendChild(ul);
    body.appendChild(files);
  }

  // ---- routing (the hash names the entry; back and forward work through the history) ----

  function hashId() {
    var h = window.location.hash.slice(1);
    try { return decodeURIComponent(h); } catch (e) { return h; }
  }

  function route() {
    if (!catalog) return;
    var raw = hashId();
    var known = raw !== '' && byId.has(raw);
    var notice = $('notice');
    if (raw !== '' && !known) {
      notice.textContent = t('notFound', { id: raw });
      notice.hidden = false;
    } else {
      notice.textContent = '';
      notice.hidden = true;
    }
    // A phone shows the list until an entry is picked; a wide screen always shows an entry.
    pick = known ? raw : (catalog.entries[0] ? catalog.entries[0].id : null);
    $('app').setAttribute('data-view', known ? 'detail' : 'list');
    renderList();
    renderDetail(pick ? byId.get(pick) : null);
    if (known && narrow.matches) {
      window.scrollTo(0, 0);
      $('detail-title').focus({ preventScroll: true });
    }
  }

  // ---- static text and language --------------------------------------------

  function applyStatic() {
    document.documentElement.lang = lang;
    document.title = t('pageTitle');
    document.querySelectorAll('[data-i18n]').forEach(function (n) {
      n.textContent = t(n.getAttribute('data-i18n'));
    });
    document.querySelectorAll('[data-i18n-placeholder]').forEach(function (n) {
      n.setAttribute('placeholder', t(n.getAttribute('data-i18n-placeholder')));
    });
    $('github-link').href = GITHUB_CATALOG;
    if (lastError) $('error-text').textContent = t('errorIntro') + t(lastError) + '.';
  }

  function setLang(code) {
    lang = code;
    saveLang(code);
    applyStatic();
    renderTabs();
    renderList();
    if (catalog) renderDetail(pick ? byId.get(pick) : null);
  }

  // ---- loading -------------------------------------------------------------

  function showError(reason) {
    lastError = reason;
    $('loading').hidden = true;
    $('app').hidden = true;
    $('error-text').textContent = t('errorIntro') + t(reason) + '.';
    $('error').hidden = false;
  }

  function load() {
    $('loading').hidden = false;
    $('error').hidden = true;
    $('app').hidden = true;
    lastError = null;
    byId = new Map();
    // One catalog file: its text, parsed (a host's "not found" page comes back as HTML with status 200).
    function readCatalog(url) {
      return fetch(url, { cache: 'no-cache' })
        .then(function (res) {
          if (!res.ok) throw new Error('HTTP ' + res.status);
          return res.text();
        })
        .then(function (body) {
          return JSON.parse(body);
        });
    }
    return readCatalog(CATALOG_URLS[0])
      .catch(function () {
        return readCatalog(CATALOG_URLS[1]);
      })
      .then(function (data) {
        var problem = checkCatalog(data);
        if (problem) return showError(problem);
        catalog = data;
        buildDependents();
        $('loading').hidden = true;
        $('app').hidden = false;
        applyStatic();
        renderTabs();
        route();
      })
      .catch(function (e) {
        // Neither file parses: the bare one's text says so; no connection (or no file) is errFetch.
        showError(e instanceof SyntaxError ? 'errNotJson' : 'errFetch');
      });
  }

  // ---- wiring --------------------------------------------------------------

  $('lang-switch').addEventListener('click', function () {
    setLang(lang === 'ru' ? 'en' : 'ru');
  });

  $('search').addEventListener('input', function (ev) {
    query = ev.target.value;
    renderList();
  });

  $('back').addEventListener('click', function () {
    // Leave the entry's hash without a new history step, then show the list.
    window.history.replaceState(null, '', window.location.pathname + window.location.search);
    route();
  });

  $('retry').addEventListener('click', load);
  window.addEventListener('hashchange', route);
  window.addEventListener('popstate', route);

  applyStatic();
  renderTabs();
  load();
})();
