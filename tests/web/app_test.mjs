// Web 前端控制器单元测试（缺口 3 P1）
// 基于 Node.js 原生 node --test，零 npm / node_modules 依赖

import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const app = require('../../src/web/assets/app.js');
const cards = require('../../src/web/assets/http_cards.js');

// 极简 Micro-DOM 打桩器
function createMockDom() {
  const elements = new Map();

  function getOrCreateElement(id) {
    if (!elements.has(id)) {
      elements.set(id, {
        id,
        style: {},
        disabled: false,
        title: '',
        textContent: '',
        innerHTML: '',
        dataset: {},
        classList: {
          contains: () => false,
          add: () => {},
          remove: () => {},
        },
      });
    }
    return elements.get(id);
  }

  const mockDocument = {
    getElementById: (id) => getOrCreateElement(id),
    querySelector: () => null,
    querySelectorAll: () => [],
    createElement: (tag) => {
      const el = {
        tagName: tag.toUpperCase(),
        style: {},
        dataset: {},
        children: [],
        textContent: '',
        className: '',
        classList: {
          add: (c) => {
            el.className = `${el.className} ${c}`.trim();
          },
          remove: () => {},
          toggle: () => {},
        },
        appendChild: (child) => {
          el.children.push(child);
        },
      };
      return el;
    },
    documentElement: {
      getAttribute: () => 'dark',
      setAttribute: () => {},
    },
  };

  const mockWindow = {
    matchMedia: () => ({ matches: true }),
  };

  return { mockDocument, mockWindow, getOrCreateElement, elements };
}

test('applyReadonlyState disables actions and displays badge when readonly is true', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;

  // Mock fetch for /api/v1/meta
  global.fetch = async (url) => {
    if (url === '/api/v1/meta') {
      return {
        json: async () => ({
          success: true,
          data: { readonly: true, workspace: '/test/workspace' },
        }),
      };
    }
    throw new Error(`Unexpected fetch URL: ${url}`);
  };

  await app.applyReadonlyState();

  const badge = getOrCreateElement('readonly-badge');
  assert.equal(badge.style.display, 'inline-block');

  const actionButtonIds = ['btn-save', 'btn-run-selection', 'btn-run-all', 'btn-import-modal'];
  for (const id of actionButtonIds) {
    const btn = getOrCreateElement(id);
    assert.equal(btn.disabled, true, `Button ${id} must be disabled`);
    assert.equal(btn.style.opacity, '0.45', `Button ${id} must have dimmed opacity`);
    assert.equal(btn.style.cursor, 'not-allowed', `Button ${id} must have not-allowed cursor`);
    assert.match(btn.title, /只读模式/, `Button ${id} title must explain readonly mode`);
  }

  const statusIndicator = getOrCreateElement('status-indicator');
  assert.equal(statusIndicator.textContent, '[STATUS] Locked (Read-Only)');
});

test('applyReadonlyState keeps controls active when readonly is false', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;

  global.fetch = async (url) => {
    if (url === '/api/v1/meta') {
      return {
        json: async () => ({
          success: true,
          data: { readonly: false, workspace: '/test/workspace' },
        }),
      };
    }
    throw new Error(`Unexpected fetch URL: ${url}`);
  };

  await app.applyReadonlyState();

  const badge = getOrCreateElement('readonly-badge');
  assert.notEqual(badge.style.display, 'inline-block');

  const btnSave = getOrCreateElement('btn-save');
  assert.equal(btnSave.disabled, false);
});

test('runSuite captures network exceptions and renders [NETWORK ERR]', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;

  app.setCurrentFilePath('tests/suite.http');

  // Mock network crash
  global.fetch = async () => {
    throw new Error('Connection refused to backend daemon');
  };

  await app.runSuite();

  const debugOutput = getOrCreateElement('debug-output');
  assert.match(
    debugOutput.innerHTML,
    /\[NETWORK ERR\]/,
    'Error container must render [NETWORK ERR] tag'
  );
  assert.match(
    debugOutput.innerHTML,
    /Connection refused to backend daemon/,
    'Error container must display the network error message'
  );
});

test('runSuite renders suite summary and individual assertion tags on success', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;

  app.setCurrentFilePath('tests/suite.http');

  global.fetch = async (url, opts) => {
    if (url === '/api/v1/run/suite') {
      const payload = JSON.parse(opts.body);
      assert.equal(payload.path, 'tests/suite.http');
      return {
        json: async () => ({
          success: true,
          data: {
            path: 'tests/suite.http',
            total: 2,
            passed: 1,
            failed: 0,
            skipped: 1,
            duration_ms: 42,
            items: [
              {
                name: 'Get User',
                method: 'GET',
                url: 'http://localhost/users',
                status: 200,
                duration_ms: 12,
                success: true,
                skipped: false,
              },
              {
                name: 'Delete User',
                method: 'DELETE',
                url: 'http://localhost/users/1',
                status: 0,
                duration_ms: 0,
                success: false,
                skipped: true,
              },
            ],
          },
        }),
      };
    }
    throw new Error(`Unexpected URL: ${url}`);
  };

  await app.runSuite();

  const debugOutput = getOrCreateElement('debug-output');
  assert.match(debugOutput.innerHTML, /\[SUITE RESULT\] tests\/suite\.http/);
  assert.match(debugOutput.innerHTML, /Total: 2 \| Passed: 1 \| Failed: 0 \| Skipped: 1/);
  assert.match(debugOutput.innerHTML, /\[PASS\]/);
  assert.match(debugOutput.innerHTML, /\[SKIP\]/);
});

test('escapeHtml sanitizes dangerous HTML characters', () => {
  assert.equal(
    app.escapeHtml('<script>alert("XSS & \'attack\'")</script>'),
    '&lt;script&gt;alert(&quot;XSS &amp; &#39;attack&#39;&quot;)&lt;/script&gt;'
  );
  assert.equal(app.escapeHtml(null), '');
  assert.equal(app.escapeHtml(undefined), '');
});

test('toggleDir lazy-loads subdirectory, caches it, and renders nested items with indentation', async () => {
  app.clearTreeState();
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  const fileListEl = getOrCreateElement('file-list');
  fileListEl.children = [];
  Object.defineProperty(fileListEl, 'innerHTML', {
    get: () => '',
    set: (val) => {
      if (val === '') fileListEl.children = [];
    },
  });
  fileListEl.appendChild = (child) => { fileListEl.children.push(child); };

  global.document = mockDocument;
  global.window = mockWindow;

  app.setAllFiles([
    { path: 'doc', name: 'doc', is_dir: true },
    { path: 'empty_dir', name: 'empty_dir', is_dir: true },
    { path: 'README.md', name: 'README.md', is_dir: false },
  ]);

  let fetchCount = 0;
  global.fetch = async (url) => {
    fetchCount += 1;
    if (url === '/api/v1/files?path=doc') {
      return {
        json: async () => ({
          success: true,
          data: [{ path: 'doc/quickstart.md', name: 'quickstart.md', is_dir: false }],
        }),
      };
    }
    if (url === '/api/v1/files?path=empty_dir') {
      return {
        json: async () => ({ success: true, data: [] }),
      };
    }
    throw new Error(`Unexpected URL: ${url}`);
  };

  await app.toggleDir('doc');
  assert.deepEqual(app.getExpandedDirs(), ['doc']);
  assert.equal(fetchCount, 1);
  // Expect: doc, doc/quickstart.md, empty_dir, README.md
  assert.equal(fileListEl.children.length, 4);
  assert.equal(fileListEl.children[0].children[0].textContent, '[-]');
  assert.equal(fileListEl.children[1].dataset.path, 'doc/quickstart.md');
  assert.equal(fileListEl.children[1].style.paddingLeft, '28px');

  // Collapse then re-expand should hit cache (no extra fetch)
  await app.toggleDir('doc');
  assert.deepEqual(app.getExpandedDirs(), []);
  assert.equal(fileListEl.children.length, 3);

  await app.toggleDir('doc');
  assert.deepEqual(app.getExpandedDirs(), ['doc']);
  assert.equal(fetchCount, 1);

  // Expanding empty dir renders (empty) placeholder
  await app.toggleDir('empty_dir');
  assert.equal(fileListEl.children.some((c) => c.textContent === '(empty)'), true);
});

test('filterFiles searches across cached subdirectories', async () => {
  app.clearTreeState();
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  const fileListEl = getOrCreateElement('file-list');
  fileListEl.children = [];
  Object.defineProperty(fileListEl, 'innerHTML', {
    get: () => '',
    set: (val) => {
      if (val === '') fileListEl.children = [];
    },
  });
  fileListEl.appendChild = (child) => { fileListEl.children.push(child); };

  global.document = mockDocument;
  global.window = mockWindow;

  app.setAllFiles([
    { path: 'doc', name: 'doc', is_dir: true },
    { path: 'README.md', name: 'README.md', is_dir: false },
  ]);
  global.fetch = async () => ({
    json: async () => ({
      success: true,
      data: [{ path: 'doc/quickstart.md', name: 'quickstart.md', is_dir: false }],
    }),
  });
  await app.toggleDir('doc');

  const filterInput = getOrCreateElement('file-filter-input');
  filterInput.value = 'quickstart';
  app.filterFiles();

  assert.equal(fileListEl.children.length, 1);
  assert.equal(fileListEl.children[0].dataset.path, 'doc/quickstart.md');
});

test('syncEditorModeForFile defaults .md to preview and .http to source', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;

  const editorEl = getOrCreateElement('code-editor');
  editorEl.value = '# Hello Rupost';
  const modeSwitch = getOrCreateElement('editor-mode-switch');
  const sourcePane = getOrCreateElement('editor-source-pane');
  const previewPane = getOrCreateElement('markdown-preview');
  const wrapper = getOrCreateElement('editor-wrapper');
  wrapper.classList = { toggle: () => {} };

  global.fetch = async (url) => {
    if (url === '/api/v1/markdown/render') {
      return {
        json: async () => ({
          success: true,
          data: { html: '<h1>Hello Rupost</h1>', executable_blocks: [], executable_cards: [] },
        }),
      };
    }
    if (url === '/api/v1/http/parse') {
      return {
        json: async () => ({
          success: true,
          data: {
            variables: {},
            cards: [
              {
                title: 'Demo',
                method: 'GET',
                url: 'https://example.com',
                headers: [],
                body: null,
                directives: [],
                has_mock: false,
                is_websocket: false,
                raw_block: '### Demo\nGET https://example.com',
              },
            ],
          },
        }),
      };
    }
    throw new Error(`Unexpected URL: ${url}`);
  };

  app.setCurrentFilePath('README.md');
  app.syncEditorModeForFile('README.md');
  assert.equal(modeSwitch.style.display, 'inline-flex');
  assert.equal(sourcePane.style.display, 'none');
  assert.equal(previewPane.style.display, 'block');

  // Wait for async renderMarkdownPreview
  await new Promise((r) => setTimeout(r, 0));
  assert.match(previewPane.innerHTML, /<h1>Hello Rupost<\/h1>/);

  app.setCurrentFilePath('test.http');
  editorEl.value = '### Demo\nGET https://example.com\n';
  app.syncEditorModeForFile('test.http');
  assert.equal(modeSwitch.style.display, 'inline-flex');
  assert.equal(sourcePane.style.display, 'none');
  assert.equal(previewPane.style.display, 'block');
  await new Promise((r) => setTimeout(r, 0));
  assert.match(previewPane.innerHTML, /http-cards-stack/);

  app.syncEditorModeForFile('notes.txt');
  assert.equal(modeSwitch.style.display, 'none');
  assert.equal(sourcePane.style.display, 'flex');
  assert.equal(previewPane.style.display, 'none');
});

test('resolveRelativeDocPath resolves internal .md/.http links and ignores external/anchor links', () => {
  assert.equal(
    app.resolveRelativeDocPath('README.md', 'doc/guides/quickstart.md'),
    'doc/guides/quickstart.md'
  );
  assert.equal(
    app.resolveRelativeDocPath('doc/guides/a.md', '../b.http#step1'),
    'doc/b.http'
  );
  assert.equal(
    app.resolveRelativeDocPath('doc/a.md', '../../outside.md'),
    null
  );
  assert.equal(
    app.resolveRelativeDocPath('README.md', 'https://example.com/docs.md'),
    null
  );
  assert.equal(
    app.resolveRelativeDocPath('README.md', '#section-1'),
    null
  );
});

test('toggleDir preserves active file filter view', async () => {
  app.clearTreeState();
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  const fileListEl = getOrCreateElement('file-list');
  fileListEl.children = [];
  Object.defineProperty(fileListEl, 'innerHTML', {
    get: () => '',
    set: (val) => { if (val === '') fileListEl.children = []; },
  });
  fileListEl.appendChild = (child) => { fileListEl.children.push(child); };

  global.document = mockDocument;
  global.window = mockWindow;

  app.setAllFiles([
    { path: 'doc', name: 'doc', is_dir: true },
    { path: 'README.md', name: 'README.md', is_dir: false },
  ]);
  const filterInput = getOrCreateElement('file-filter-input');
  filterInput.value = 'quickstart';

  global.fetch = async () => ({
    json: async () => ({
      success: true,
      data: [{ path: 'doc/quickstart.md', name: 'quickstart.md', is_dir: false }],
    }),
  });

  await app.toggleDir('doc');
  assert.equal(fileListEl.children.length, 1);
  assert.equal(fileListEl.children[0].dataset.path, 'doc/quickstart.md');
});

test('insertIntoEditor refreshes markdown preview when not in source mode', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;

  const editorEl = getOrCreateElement('code-editor');
  editorEl.value = '# Doc';
  editorEl.selectionStart = 5;
  editorEl.selectionEnd = 5;
  const previewPane = getOrCreateElement('markdown-preview');
  const wrapper = getOrCreateElement('editor-wrapper');
  wrapper.classList = { toggle: () => {} };

  let renderCalled = 0;
  global.fetch = async (url) => {
    if (url === '/api/v1/markdown/render') {
      renderCalled += 1;
      return {
        json: async () => ({
          success: true,
          data: { html: '<h1>Doc</h1><p>imported</p>', executable_blocks: [] },
        }),
      };
    }
    throw new Error(`Unexpected URL: ${url}`);
  };

  app.setCurrentFilePath('README.md');
  app.setEditorMode('preview');
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(renderCalled, 1);

  app.insertIntoEditor('```http\nGET https://example.com\n```');
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(renderCalled, 2);
  assert.match(previewPane.innerHTML, /imported/);
});

test('highlightJson highlights tokens, escapes XSS, and falls back for >100KB', () => {
  const html = cards.highlightJson('{"ok": true, "n": 42, "xss": "<script>"}');
  assert.match(html, /class="tok-json-key"/);
  assert.match(html, /class="tok-json-bool">true/);
  assert.match(html, /class="tok-json-num">42/);
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script&gt;/);

  const big = JSON.stringify({ data: 'a'.repeat(110 * 1024) });
  const bigHtml = cards.highlightJson(big);
  assert.doesNotMatch(bigHtml, /tok-json-key/);
});

test('extractHttpFileBlocks extracts top-level @vars and splits ### blocks', () => {
  const src = `@tenant = east_01\n@base_url = https://httpbingo.org\n\n### One\nGET {{base_url}}/get?t={{tenant}}\n\n### Two\nPOST {{base_url}}/post\n\n{"a":1}\n`;
  const { fileVariables, blocks } = cards.extractHttpFileBlocks(src);
  assert.deepEqual(fileVariables, { tenant: 'east_01', base_url: 'https://httpbingo.org' });
  assert.equal(blocks.length, 2);
  assert.match(blocks[0], /### One/);
  assert.match(blocks[1], /POST/);
});

test('parseHttpBlock & rebuildBlockWithBody preserve @mock-when variants and handle empty body', () => {
  const blockWithMock = `### Login\nPOST https://api.example.com/login\nContent-Type: application/json\n\n{"user":"alice"}\n\n# @mock-when body.$.user == "alice"\n200\n{"ok":true}`;
  const parsed = cards.parseHttpBlock(blockWithMock);
  assert.equal(parsed.method, 'POST');
  assert.equal(parsed.hasMock, true);
  assert.equal(parsed.body, '{"user":"alice"}');

  const rebuilt = cards.rebuildBlockWithBody(blockWithMock, '{"user":"bob"}');
  assert.match(rebuilt, /\{"user":"bob"\}/);
  assert.match(rebuilt, /\n\n# @mock-when body\.\$\.user == "alice"/);

  const getBlock = 'GET https://example.com/ping\nAccept: application/json';
  const rebuiltGet = cards.rebuildBlockWithBody(getBlock, '{"ping":1}');
  assert.equal(rebuiltGet, 'GET https://example.com/ping\nAccept: application/json\n\n{"ping":1}');
});

test('formatDurationObj converts Rust Duration {secs, nanos} to ms', () => {
  assert.equal(cards.formatDurationObj({ secs: 1, nanos: 250000000 }), '1250.0 ms');
  assert.equal(cards.formatDurationObj(null), '-');
});

test('loadEnvs populates #env-select and selects default_env', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  const envSelect = getOrCreateElement('env-select');
  envSelect.children = [];
  Object.defineProperty(envSelect, 'innerHTML', {
    get: () => '',
    set: (v) => { if (v === '') envSelect.children = []; },
  });
  envSelect.appendChild = (c) => { envSelect.children.push(c); };

  global.document = mockDocument;
  global.window = mockWindow;
  global.fetch = async (url) => {
    if (url === '/api/v1/envs') {
      return {
        json: async () => ({
          success: true,
          data: { environments: ['dev', 'prod'], default_env: 'prod' },
        }),
      };
    }
    throw new Error(`Unexpected URL: ${url}`);
  };

  await app.loadEnvs();
  assert.equal(envSelect.children.length, 3);
  assert.equal(envSelect.value, 'prod');
  assert.equal(app.getSelectedEnv(), 'prod');
});

test('openExportModal strips markdown fences and sends selected env', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;

  const envSelect = getOrCreateElement('env-select');
  envSelect.value = 'staging';
  const fmtSelect = getOrCreateElement('export-format-select');
  fmtSelect.value = 'curl';
  const preEl = getOrCreateElement('export-command-pre');

  let capturedBody = null;
  global.fetch = async (url, opts) => {
    if (url === '/api/v1/export') {
      capturedBody = JSON.parse(opts.body);
      return {
        json: async () => ({
          success: true,
          data: { format: 'curl', command: 'curl https://example.com', warnings: [] },
        }),
      };
    }
    throw new Error(`Unexpected URL: ${url}`);
  };

  await app.openExportModal('```http\nGET https://example.com\n```');
  assert.equal(capturedBody.raw_block, 'GET https://example.com');
  assert.equal(capturedBody.env, 'staging');
  assert.equal(preEl.textContent, 'curl https://example.com');
});

test('parseKvInput parses JSON and line-based key:value / key=value', () => {
  assert.deepEqual(app.parseKvInput('{"a": 1, "b": "x"}'), { a: '1', b: 'x' });
  assert.deepEqual(app.parseKvInput('Authorization: Bearer tok\nX-Id: 42'), {
    Authorization: 'Bearer tok',
    'X-Id': '42',
  });
  assert.deepEqual(app.parseKvInput('role=admin\npage=2'), { role: 'admin', page: '2' });
});

test('renderSessionTab renders masked session variables', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;
  const container = getOrCreateElement('debug-output');

  global.fetch = async (url) => {
    if (url === '/api/v1/session') {
      return {
        json: async () => ({
          success: true,
          data: {
            has_cookie_state: true,
            captured_variables: { user_id: '42', token: '***' },
          },
        }),
      };
    }
    throw new Error(`Unexpected URL: ${url}`);
  };

  await app.renderSessionTab(container);
  assert.match(container.innerHTML, /\[SESSION STATE\]/);
  assert.match(container.innerHTML, /user_id/);
  assert.match(container.innerHTML, /\*\*\*/);
});

test('runDiagnoseForUrl formats Duration {secs, nanos} and shows 403 SSRF hint', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;
  const debugEl = getOrCreateElement('debug-output');

  global.fetch = async () => ({
    status: 200,
    json: async () => ({
      success: true,
      data: {
        url: 'https://example.com',
        dns_lookup_duration: { secs: 0, nanos: 12000000 },
        tcp_connect_duration: { secs: 0, nanos: 24000000 },
        tls_handshake_duration: null,
        ttfb: { secs: 0, nanos: 50000000 },
        total_duration: { secs: 0, nanos: 90000000 },
        http_status: 200,
        http_version: 'HTTP/1.1',
        resolved_ips: ['93.184.216.34'],
      },
    }),
  });

  await app.runDiagnoseForUrl('https://example.com');
  assert.match(debugEl.innerHTML, /12\.0 ms/);
  assert.match(debugEl.innerHTML, /93\.184\.216\.34/);

  global.fetch = async () => ({
    status: 403,
    json: async () => ({
      success: false,
      error: 'Blocked by SSRF guard',
    }),
  });
  await app.runDiagnoseForUrl('http://127.0.0.1:8080');
  assert.match(debugEl.innerHTML, /--allowed-hosts/);
});

test('renderOverviewTab displays error and warning banners when request fails', () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;
  const debugEl = getOrCreateElement('debug-output');

  app.renderOverviewTab(
    {
      success: false,
      status: null,
      duration_ms: 0,
      assertions: [],
      error: 'relative URL without a base: "/orders"',
      warnings: ['base_url is not defined'],
    },
    debugEl
  );

  assert.match(debugEl.innerHTML, /\[EXECUTION ERROR\]/);
  assert.match(debugEl.innerHTML, /relative URL without a base/);
  assert.match(debugEl.innerHTML, /\[WARNINGS\]/);
  assert.match(debugEl.innerHTML, /base_url is not defined/);
});

test('renderMarkdownPreview falls back gracefully when /api/v1/http/parse returns 405 empty body', async () => {
  const { mockDocument, mockWindow, getOrCreateElement } = createMockDom();
  global.document = mockDocument;
  global.window = mockWindow;

  const editorEl = getOrCreateElement('code-editor');
  editorEl.value = '### Fallback\nGET https://example.com\n';
  const previewPane = getOrCreateElement('markdown-preview');
  const wrapper = getOrCreateElement('editor-wrapper');
  wrapper.classList = { toggle: () => {} };

  global.fetch = async (url) => {
    if (url === '/api/v1/http/parse') {
      return {
        status: 405,
        ok: false,
        text: async () => '',
      };
    }
    throw new Error(`Unexpected URL: ${url}`);
  };

  app.setCurrentFilePath('test.http');
  await app.renderMarkdownPreview();
  assert.match(previewPane.innerHTML, /http-cards-stack/);
  assert.match(previewPane.innerHTML, /https:\/\/example\.com/);
});
