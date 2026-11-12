// Web 前端控制器单元测试（缺口 3 P1）
// 基于 Node.js 原生 node --test，零 npm / node_modules 依赖

import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const app = require('../../src/web/assets/app.js');

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
    createElement: (tag) => ({
      tagName: tag.toUpperCase(),
      style: {},
      dataset: {},
      classList: { add: () => {}, remove: () => {} },
      appendChild: () => {},
    }),
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
