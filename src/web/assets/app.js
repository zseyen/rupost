// Rupost Living Workbench - Client Controller (Vanilla ESM, No Emoji)

const RupostCards =
  typeof window !== 'undefined' && window.RupostCards
    ? window.RupostCards
    : typeof require === 'function'
      ? require('./http_cards.js')
      : null;

let currentFilePath = null;
let allFiles = [];
let activeInspectorTab = 'overview';
let lastDebugData = null;
let isReadonly = false;
let currentResponseBodyText = '';

const expandedDirs = new Set();
const dirCache = new Map();
const loadingDirs = new Set();

let editorMode = 'source';
let lastRenderedExecBlocks = [];
let cardEditedBodies = new Map();
let currentHttpFileVars = {};
let markdownRenderTimer = null;
let renderSeq = 0;

// Initialize theme immediately to prevent FOUC
if (typeof window !== 'undefined' && typeof document !== 'undefined') {
  initTheme();

  document.addEventListener('DOMContentLoaded', () => {
    loadFiles();
    applyReadonlyState();
    initEditorEvents();
    initMarkdownPreviewEvents();
    initKeyboardShortcuts();
    updateGutter();
    updateEditorMeta();
  });
}

// Theme Management (Light / Dark)
function initTheme() {
  let saved = null;
  try {
    saved = localStorage.getItem('rupost-theme');
  } catch (e) {
    // 受限存储环境：裸访问会杀死整个 app.js（审查 P2-4）
  }
  const prefersDark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
  const theme = saved || (prefersDark ? 'dark' : 'light');
  applyTheme(theme);
}

function applyTheme(theme, persist = false) {
  document.documentElement.setAttribute('data-theme', theme);
  // 持久化仅限用户显式切换：自动探测结果不写入，OS 偏好变化保持生效（审查 P2）
  if (persist) {
    try {
      localStorage.setItem('rupost-theme', theme);
    } catch (e) {
      // 受限存储环境：toggle 点击不再抛未捕获异常（审查 P2④）
    }
  }
  const btn = document.getElementById('btn-theme-toggle');
  if (btn) {
    btn.textContent = theme === 'dark' ? '[THEME: DARK]' : '[THEME: LIGHT]';
    btn.title = `Current: ${theme.toUpperCase()} (Click to toggle)`;
  }
}

function toggleTheme() {
  const current = document.documentElement.getAttribute('data-theme') || 'dark';
  const next = current === 'dark' ? 'light' : 'dark';
  applyTheme(next, true);
}

// 0. Readonly state perception & control disabling
async function applyReadonlyState() {
  try {
    const res = await fetch('/api/v1/meta');
    const json = await res.json();
    if (json.success && json.data && json.data.readonly) {
      isReadonly = true;
      const badge = document.getElementById('readonly-badge');
      if (badge) badge.style.display = 'inline-block';

      ['btn-save', 'btn-run-selection', 'btn-run-all', 'btn-import-modal'].forEach(id => {
        const btn = document.getElementById(id);
        if (btn) {
          btn.disabled = true;
          btn.style.opacity = '0.45';
          btn.style.cursor = 'not-allowed';
          btn.title = '只读模式下不可用';
        }
      });
      syncMarkdownRunButtonsReadonly();
      const statusInd = document.getElementById('status-indicator');
      if (statusInd) statusInd.textContent = '[STATUS] Locked (Read-Only)';
    }
  } catch (e) {
    console.error('Failed to query workbench meta:', e);
  }
}

// 1. Load files from workspace
async function loadFiles() {
  try {
    dirCache.clear();
    const res = await fetch('/api/v1/files');
    const json = await res.json();
    if (json.success && Array.isArray(json.data)) {
      allFiles = json.data;

      // 保留已展开目录的状态并刷新其子内容
      if (expandedDirs.size > 0) {
        await Promise.all(
          Array.from(expandedDirs).map(async (dirPath) => {
            try {
              const subRes = await fetch(`/api/v1/files?path=${encodeURIComponent(dirPath)}`);
              const subJson = await subRes.json();
              if (subJson.success && Array.isArray(subJson.data)) {
                dirCache.set(dirPath, subJson.data);
              } else {
                expandedDirs.delete(dirPath);
              }
            } catch (err) {
              expandedDirs.delete(dirPath);
            }
          })
        );
      }

      refreshFileTreeView();

      // Auto-open first document if not selected
      const firstDoc = allFiles.find(f => !f.is_dir);
      if (firstDoc && !currentFilePath) {
        openFile(firstDoc.path);
      }
    }
  } catch (e) {
    console.error('Failed to load files:', e);
  }
}

function refreshFileTreeView() {
  const queryEl = document.getElementById('file-filter-input');
  if (queryEl && queryEl.value && queryEl.value.trim()) {
    filterFiles();
  } else {
    renderFileList(allFiles);
  }
}

// Toggle directory expand/collapse with lazy loading & concurrency guard
async function toggleDir(dirPath) {
  if (expandedDirs.has(dirPath)) {
    expandedDirs.delete(dirPath);
    refreshFileTreeView();
    return;
  }

  if (dirCache.has(dirPath)) {
    expandedDirs.add(dirPath);
    refreshFileTreeView();
    return;
  }

  if (loadingDirs.has(dirPath)) {
    return;
  }

  loadingDirs.add(dirPath);
  refreshFileTreeView();

  try {
    const res = await fetch(`/api/v1/files?path=${encodeURIComponent(dirPath)}`);
    const json = await res.json();
    if (json.success && Array.isArray(json.data)) {
      dirCache.set(dirPath, json.data);
      expandedDirs.add(dirPath);
    }
  } catch (e) {
    console.error(`Failed to load directory ${dirPath}:`, e);
  } finally {
    loadingDirs.delete(dirPath);
    refreshFileTreeView();
  }
}

// Render file tree recursively (or flat list when filtering)
function renderFileList(files, options = {}) {
  const listEl = document.getElementById('file-list');
  if (!listEl) return;
  listEl.innerHTML = '';

  const { flatSearch = false } = options;

  const appendItems = (entries, depth) => {
    entries.forEach(file => {
      const li = document.createElement('li');
      li.dataset.path = file.path;
      li.tabIndex = 0;
      if (depth > 0) {
        li.style.paddingLeft = `${14 + depth * 14}px`;
      }

      if (file.is_dir) {
        li.classList.add('is-dir');
        const toggle = document.createElement('span');
        toggle.className = 'dir-toggle';
        if (loadingDirs.has(file.path)) {
          toggle.textContent = '[..]';
        } else if (expandedDirs.has(file.path)) {
          toggle.textContent = '[-]';
        } else {
          toggle.textContent = '[+]';
        }
        li.appendChild(toggle);
      }

      const tag = document.createElement('span');
      tag.className = 'item-tag';
      if (file.is_dir) {
        tag.textContent = '[DIR]';
      } else if (file.name.endsWith('.http')) {
        tag.textContent = '[HTTP]';
      } else {
        tag.textContent = '[DOC]';
      }

      const name = document.createElement('span');
      name.className = 'item-name';
      name.textContent = flatSearch ? file.path : file.name;
      name.title = file.path;

      li.appendChild(tag);
      li.appendChild(name);

      const activate = () => {
        if (file.is_dir) {
          toggleDir(file.path);
        } else {
          openFile(file.path);
        }
      };

      li.onclick = activate;
      li.onkeydown = (e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          activate();
        }
      };

      if (file.path === currentFilePath) {
        li.classList.add('active');
      }
      listEl.appendChild(li);

      if (!flatSearch && file.is_dir && expandedDirs.has(file.path)) {
        const children = dirCache.get(file.path) || [];
        if (children.length === 0) {
          const emptyLi = document.createElement('li');
          emptyLi.className = 'tree-empty';
          emptyLi.style.paddingLeft = `${14 + (depth + 1) * 14}px`;
          emptyLi.textContent = '(empty)';
          listEl.appendChild(emptyLi);
        } else {
          appendItems(children, depth + 1);
        }
      }
    });
  };

  appendItems(files, 0);
}

// Filter file tree across root and all cached subdirectories
function filterFiles() {
  const inputEl = document.getElementById('file-filter-input');
  const query = inputEl ? inputEl.value.trim().toLowerCase() : '';
  if (!query) {
    renderFileList(allFiles);
    return;
  }

  const seen = new Set();
  const pool = [];
  const collect = (entries) => {
    entries.forEach(entry => {
      if (!seen.has(entry.path)) {
        seen.add(entry.path);
        pool.push(entry);
      }
      if (entry.is_dir && dirCache.has(entry.path)) {
        collect(dirCache.get(entry.path));
      }
    });
  };
  collect(allFiles);

  const filtered = pool.filter(
    f => f.name.toLowerCase().includes(query) || f.path.toLowerCase().includes(query)
  );
  renderFileList(filtered, { flatSearch: true });
}

// 2. Open active document
async function openFile(path) {
  try {
    const res = await fetch(`/api/v1/file?path=${encodeURIComponent(path)}`);
    const json = await res.json();
    if (json.success && json.data) {
      currentFilePath = path;

      const fileLabel = document.getElementById('active-file-label');
      if (fileLabel) fileLabel.textContent = path;

      const editorTitle = document.getElementById('editor-file-title');
      if (editorTitle) editorTitle.textContent = path;

      const fileContent = typeof json.data === 'string' ? json.data : (json.data && json.data.content ? json.data.content : '');
      const editor = document.getElementById('code-editor');
      editor.value = fileContent;
      cardEditedBodies.clear();

      updateGutter();
      updateEditorMeta();
      syncEditorModeForFile(path);

      document.querySelectorAll('#file-list li').forEach(li => {
        li.classList.toggle('active', li.dataset.path === path);
      });
    }
  } catch (e) {
    console.error('Failed to open document:', e);
  }
}

// 3. Save document with backend syntax guard
async function saveActiveFile() {
  if (!currentFilePath) {
    alert('Please select a file first.');
    return;
  }
  const content = document.getElementById('code-editor').value;
  const statusEl = document.getElementById('save-status');
  statusEl.textContent = '[SAVING] ...';
  statusEl.style.color = 'var(--text-secondary)';

  try {
    const res = await fetch('/api/v1/save', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: currentFilePath, content })
    });
    const json = await res.json();
    if (res.ok && json.success) {
      statusEl.textContent = '[OK] Saved & Verified';
      statusEl.style.color = 'var(--accent-pass)';
      setTimeout(() => { statusEl.textContent = ''; }, 3000);
    } else {
      statusEl.textContent = '[ERR] ' + (json.error || 'Syntax Guard Error');
      statusEl.style.color = 'var(--accent-fail)';
      alert(`Save Rejected (Syntax Guard):\n${json.error}`);
    }
  } catch (e) {
    statusEl.textContent = '[ERR] Network Error';
    statusEl.style.color = 'var(--accent-fail)';
  }
}

// 4. In-situ single block debug execution (shared by editor & Markdown Run buttons)
async function runCurrentBlock() {
  const editor = document.getElementById('code-editor');
  const fullText = editor.value;
  const cursorPos = editor.selectionStart;
  const rawBlock = extractTargetBlock(fullText, cursorPos);
  await executeRawBlock(rawBlock);
}

async function executeRawBlock(rawBlock, triggerBtn = null, extraVars = null) {
  const debugEl = document.getElementById('debug-output');
  debugEl.innerHTML = '<div class="empty-hint"><div class="empty-icon">[EXECUTING]</div><div class="empty-text">Dispatching request to runtime engine...</div></div>';

  const origLabel = triggerBtn ? triggerBtn.textContent : '';
  if (triggerBtn) {
    triggerBtn.disabled = true;
    triggerBtn.textContent = '[RUNNING...]';
  }

  try {
    const payload = {
      file_path: currentFilePath || 'inline.http',
      raw_block: rawBlock,
    };
    if (extraVars && Object.keys(extraVars).length > 0) {
      payload.extra_variables = extraVars;
    }

    const res = await fetch('/api/v1/run/block', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    const json = await res.json();
    if (json.success && json.data) {
      lastDebugData = json.data;
      updateSummaryPill(json.data);
      renderInspectorTab(activeInspectorTab);
    } else {
      debugEl.innerHTML = `<div class="assertion-item" style="border-color: var(--accent-fail);"><span class="assert-tag fail">[EXEC ERR]</span><span class="assert-expr">${escapeHtml(json.error || 'Execution failed')}</span></div>`;
    }
  } catch (e) {
    debugEl.innerHTML = `<div class="assertion-item" style="border-color: var(--accent-fail);"><span class="assert-tag fail">[NETWORK ERR]</span><span class="assert-expr">${escapeHtml(e.message)}</span></div>`;
  } finally {
    if (triggerBtn) {
      triggerBtn.disabled = isReadonly;
      triggerBtn.textContent = origLabel;
    }
  }
}

// 4.1 Run entire file suite
async function runSuite() {
  if (!currentFilePath) {
    alert('Please select a file first.');
    return;
  }
  const debugEl = document.getElementById('debug-output');
  debugEl.innerHTML = '<div class="empty-hint"><div class="empty-icon">[RUNNING SUITE]</div><div class="empty-text">Executing all requests in active document...</div></div>';

  try {
    const res = await fetch('/api/v1/run/suite', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        path: currentFilePath
      })
    });
    const json = await res.json();
    if (json.success && json.data) {
      const suite = json.data;
      let html = `
        <div class="timing-panel">
          <div class="timing-header">
            <span>[SUITE RESULT] ${escapeHtml(suite.path)}</span>
            <span>Total: ${suite.total} | Passed: ${suite.passed} | Failed: ${suite.failed} | Skipped: ${suite.skipped}</span>
          </div>
          <div style="font-family: var(--font-mono); font-size: 11px; margin-top: 6px; color: var(--text-secondary);">
            Duration: ${suite.duration_ms} ms
          </div>
        </div>
      `;

      if (suite.items && suite.items.length > 0) {
        html += '<ul class="assertions-list">';
        suite.items.forEach((r, idx) => {
          let tag;
          if (r.skipped) {
            tag = '<span class="assert-tag" style="background: rgba(110, 118, 129, 0.2); color: var(--text-muted); border: 1px solid var(--border);">[SKIP]</span>';
          } else if (r.success) {
            tag = '<span class="assert-tag pass">[PASS]</span>';
          } else {
            tag = '<span class="assert-tag fail">[FAIL]</span>';
          }
          const itemTitle = r.name ? `${r.name} (${r.method} ${r.url})` : `${r.method} ${r.url}`;
          html += `
            <li class="assertion-item">
              ${tag}
              <div style="display: flex; flex-direction: column; gap: 2px; flex: 1;">
                <span class="assert-expr">#${idx + 1} ${escapeHtml(itemTitle)} - ${r.status || 'ERR'} (${r.duration_ms}ms)</span>
                ${r.error ? `<span style="color: var(--accent-fail); font-size: 10px;">${escapeHtml(r.error)}</span>` : ''}
              </div>
            </li>
          `;
        });
        html += '</ul>';
      }
      debugEl.innerHTML = html;
    } else {
      debugEl.innerHTML = `<div class="assertion-item" style="border-color: var(--accent-fail);"><span class="assert-tag fail">[SUITE ERR]</span><span class="assert-expr">${escapeHtml(json.error || 'Suite execution failed')}</span></div>`;
    }
  } catch (e) {
    debugEl.innerHTML = `<div class="assertion-item" style="border-color: var(--accent-fail);"><span class="assert-tag fail">[NETWORK ERR]</span><span class="assert-expr">${escapeHtml(e.message)}</span></div>`;
  }
}

// Extract closest code block from text around cursor
function extractTargetBlock(text, pos) {
  const before = text.slice(0, pos);
  const after = text.slice(pos);
  const lastBlockStart = before.lastIndexOf('```');
  const nextBlockEnd = after.indexOf('```');

  if (lastBlockStart !== -1 && nextBlockEnd !== -1) {
    return text.slice(lastBlockStart, pos + nextBlockEnd + 3);
  }
  return text;
}

// Update Top Pill Summary
function updateSummaryPill(data) {
  const pill = document.getElementById('inspector-summary-pill');
  const statusEl = document.getElementById('pill-status');
  const durEl = document.getElementById('pill-duration');

  if (!pill) return;
  pill.style.display = 'flex';

  const isOk = data.success && data.status && data.status < 400;
  statusEl.className = 'pill-code ' + (isOk ? 'pill-pass' : 'pill-fail');
  statusEl.textContent = (data.status ? data.status + ' ' : '') + (isOk ? 'OK' : 'ERR');
  durEl.textContent = `${data.duration_ms} ms`;
}

// 5. Inspector Tabs Switcher & Renderer
function switchInspectorTab(tab) {
  activeInspectorTab = tab;
  document.querySelectorAll('.tab-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.tab === tab);
  });
  renderInspectorTab(tab);
}

function renderInspectorTab(tab) {
  const debugEl = document.getElementById('debug-output');
  if (!lastDebugData) {
    debugEl.innerHTML = `
      <div class="empty-hint">
        <div class="empty-icon">[NO RUN DATA]</div>
        <div class="empty-text">Select an HTTP request block in editor and press [Run Block] (Shift+Enter) to evaluate in real-time.</div>
      </div>
    `;
    return;
  }

  const d = lastDebugData;

  switch (tab) {
    case 'overview':
      renderOverviewTab(d, debugEl);
      break;
    case 'body':
      renderBodyTab(d, debugEl);
      break;
    case 'headers':
      renderHeadersTab(d, debugEl);
      break;
    case 'assertions':
      renderAssertionsTab(d, debugEl);
      break;
    case 'waterfall':
      renderWaterfallTab(d, debugEl);
      break;
    case 'raw':
      renderRawTab(d, debugEl);
      break;
  }
}

function renderOverviewTab(d, container) {
  let html = `
    <div class="timing-panel">
      <div class="timing-header">
        <span>[EXECUTION SUMMARY]</span>
        <span>${d.duration_ms} ms</span>
      </div>
      <div style="font-family: var(--font-mono); font-size: 11px; display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 6px;">
        <div>Status: <strong style="color: ${d.success ? 'var(--accent-pass)' : 'var(--accent-fail)'};">${d.status || 'ERR'}</strong></div>
        <div>Assertions: <strong>${d.assertions ? d.assertions.length : 0} defined</strong></div>
      </div>
    </div>
  `;

  // Mini Waterfall preview
  if (d.timing_waterfall && d.timing_waterfall.total_ms > 0) {
    const wf = d.timing_waterfall;
    const dnsPct = (wf.dns_ms / wf.total_ms) * 100;
    const tcpPct = (wf.tcp_ms / wf.total_ms) * 100;
    const ttfbPct = wf.ttfb_ms ? (wf.ttfb_ms / wf.total_ms) * 100 : 0;
    const transPct = (wf.transfer_ms / wf.total_ms) * 100;

    html += `
      <div class="timing-panel">
        <div class="timing-header">
          <span>TIMING BREAKDOWN</span>
          <span>${wf.total_ms.toFixed(1)} ms</span>
        </div>
        <div class="waterfall-track">
          ${dnsPct > 0 ? `<div class="waterfall-segment wf-dns" style="width: ${dnsPct}%;" title="DNS: ${wf.dns_ms.toFixed(1)}ms"></div>` : ''}
          ${tcpPct > 0 ? `<div class="waterfall-segment wf-tcp" style="width: ${tcpPct}%;" title="TCP: ${wf.tcp_ms.toFixed(1)}ms"></div>` : ''}
          ${ttfbPct > 0 ? `<div class="waterfall-segment wf-ttfb" style="width: ${ttfbPct}%;" title="TTFB: ${wf.ttfb_ms.toFixed(1)}ms"></div>` : ''}
          ${transPct > 0 ? `<div class="waterfall-segment wf-xfr" style="width: ${transPct}%;" title="Transfer: ${wf.transfer_ms.toFixed(1)}ms"></div>` : ''}
        </div>
      </div>
    `;
  }

  // Quick Assertions Summary
  if (d.assertions && d.assertions.length > 0) {
    html += '<div style="margin-bottom: 8px; font-weight: 600; color: var(--text-secondary); font-family: var(--font-mono);">ASSERTIONS:</div>';
    html += '<ul class="assertions-list">';
    d.assertions.forEach(a => {
      const tag = a.passed ? '<span class="assert-tag pass">[PASS]</span>' : '<span class="assert-tag fail">[FAIL]</span>';
      html += `
        <li class="assertion-item">
          ${tag}
          <span class="assert-expr">${escapeHtml(a.raw)}</span>
        </li>
      `;
    });
    html += '</ul>';
  }

  container.innerHTML = html;
}

function renderBodyTab(d, container) {
  currentResponseBodyText = d.response_body || '';
  const displayHtml = currentResponseBodyText
    ? RupostCards.highlightJson(currentResponseBodyText)
    : '<span class="tok-null">[EMPTY BODY]</span>';
  container.innerHTML = `
    <div class="code-container">
      <div class="code-toolbar">
        <span class="code-title">Response Body</span>
        <div style="display: flex; gap: 6px;">
          <button class="btn btn-secondary btn-sm" onclick="formatBodyJson()">[Format JSON]</button>
          <button id="btn-copy-body" class="btn btn-secondary btn-sm" onclick="copyResponseBody()">[Copy Body]</button>
        </div>
      </div>
      <pre class="code-pre"><code id="response-body-code">${displayHtml}</code></pre>
    </div>
  `;
}

function renderHeadersTab(d, container) {
  const headers = d.response_headers || {};
  const keys = Object.keys(headers);
  if (keys.length === 0) {
    container.innerHTML = '<div class="empty-hint"><div class="empty-text">No response headers captured.</div></div>';
    return;
  }
  let html = '<table class="headers-table"><thead><tr><th>Header</th><th>Value</th></tr></thead><tbody>';
  keys.forEach(k => {
    html += `<tr><td class="header-name">${escapeHtml(k)}</td><td class="header-value">${escapeHtml(headers[k])}</td></tr>`;
  });
  html += '</tbody></table>';
  container.innerHTML = html;
}

function renderAssertionsTab(d, container) {
  const assertions = d.assertions || [];
  if (assertions.length === 0) {
    container.innerHTML = '<div class="empty-hint"><div class="empty-text">No assertions defined in this block. Add `@assert status == 200` to evaluate.</div></div>';
    return;
  }
  let html = '<ul class="assertions-list">';
  assertions.forEach(a => {
    const tag = a.passed ? '<span class="assert-tag pass">[PASS]</span>' : '<span class="assert-tag fail">[FAIL]</span>';
    html += `
      <li class="assertion-item">
        ${tag}
        <div style="display: flex; flex-direction: column; gap: 2px;">
          <span class="assert-expr">${escapeHtml(a.raw)}</span>
          ${a.error ? `<span style="color: var(--accent-fail); font-size: 10px;">${escapeHtml(a.error)}</span>` : ''}
        </div>
      </li>
    `;
  });
  html += '</ul>';
  container.innerHTML = html;
}

function renderWaterfallTab(d, container) {
  if (!d.timing_waterfall || d.timing_waterfall.total_ms <= 0) {
    container.innerHTML = '<div class="empty-hint"><div class="empty-text">No timing waterfall metrics available for this request.</div></div>';
    return;
  }
  const wf = d.timing_waterfall;
  const dnsPct = (wf.dns_ms / wf.total_ms) * 100;
  const tcpPct = (wf.tcp_ms / wf.total_ms) * 100;
  const ttfbPct = wf.ttfb_ms ? (wf.ttfb_ms / wf.total_ms) * 100 : 0;
  const transPct = (wf.transfer_ms / wf.total_ms) * 100;

  container.innerHTML = `
    <div class="timing-panel">
      <div class="timing-header">
        <span>NETWORK LATENCY WATERFALL</span>
        <span>Total: ${wf.total_ms.toFixed(1)} ms</span>
      </div>
      <div class="waterfall-track">
        ${dnsPct > 0 ? `<div class="waterfall-segment wf-dns" style="width: ${dnsPct}%;"></div>` : ''}
        ${tcpPct > 0 ? `<div class="waterfall-segment wf-tcp" style="width: ${tcpPct}%;"></div>` : ''}
        ${ttfbPct > 0 ? `<div class="waterfall-segment wf-ttfb" style="width: ${ttfbPct}%;"></div>` : ''}
        ${transPct > 0 ? `<div class="waterfall-segment wf-xfr" style="width: ${transPct}%;"></div>` : ''}
      </div>
      <div class="timing-legend">
        <div class="legend-item">
          <span><span class="legend-dot" style="background: var(--wf-dns);"></span>DNS Lookup</span>
          <strong>${wf.dns_ms.toFixed(1)} ms</strong>
        </div>
        <div class="legend-item">
          <span><span class="legend-dot" style="background: var(--wf-tcp);"></span>TCP Handshake</span>
          <strong>${wf.tcp_ms.toFixed(1)} ms</strong>
        </div>
        <div class="legend-item">
          <span><span class="legend-dot" style="background: var(--wf-ttfb);"></span>TTFB (Server)</span>
          <strong>${(wf.ttfb_ms || 0).toFixed(1)} ms</strong>
        </div>
        <div class="legend-item">
          <span><span class="legend-dot" style="background: var(--wf-transfer);"></span>Data Transfer</span>
          <strong>${wf.transfer_ms.toFixed(1)} ms</strong>
        </div>
      </div>
    </div>
  `;
}

function renderRawTab(d, container) {
  container.innerHTML = `
    <div class="code-container">
      <div class="code-toolbar">
        <span class="code-title">Raw Debug JSON Response</span>
      </div>
      <pre class="code-pre"><code>${escapeHtml(JSON.stringify(d, null, 2))}</code></pre>
    </div>
  `;
}

function formatBodyJson() {
  const codeEl = document.getElementById('response-body-code');
  if (!codeEl) return;
  const raw = currentResponseBodyText || codeEl.textContent || '';
  try {
    const obj = JSON.parse(raw);
    const formatted = JSON.stringify(obj, null, 2);
    currentResponseBodyText = formatted;
    codeEl.innerHTML = RupostCards.highlightJson(formatted);
  } catch (e) {
    // not valid JSON, ignore
  }
}

function copyResponseBody() {
  const codeEl = document.getElementById('response-body-code');
  if (!codeEl) return;
  const raw = currentResponseBodyText || codeEl.textContent || '';
  navigator.clipboard.writeText(raw).then(() => {
    const btn = document.getElementById('btn-copy-body');
    if (btn) {
      const orig = btn.textContent;
      btn.textContent = '[Copied!]';
      setTimeout(() => { btn.textContent = orig; }, 1500);
    }
  });
}

// 6. Editor Gutter and Meta Synchronizer
function initEditorEvents() {
  const editor = document.getElementById('code-editor');
  const gutter = document.getElementById('editor-gutter');

  if (!editor || !gutter) return;

  editor.addEventListener('input', () => {
    updateGutter();
    updateEditorMeta();
    if (editorMode === 'split') {
      scheduleMarkdownRender();
    }
  });

  editor.addEventListener('scroll', () => {
    gutter.scrollTop = editor.scrollTop;
  });

  const updateCursor = () => {
    const val = editor.value;
    const pos = editor.selectionStart;
    const lines = val.slice(0, pos).split('\n');
    const lineNum = lines.length;
    const colNum = lines[lines.length - 1].length + 1;
    const ind = document.getElementById('cursor-pos-indicator');
    if (ind) ind.textContent = `Ln ${lineNum}, Col ${colNum}`;
  };

  editor.addEventListener('keyup', updateCursor);
  editor.addEventListener('click', updateCursor);
}

function updateGutter() {
  const editor = document.getElementById('code-editor');
  const gutter = document.getElementById('editor-gutter');
  if (!editor || !gutter) return;

  const lineCount = editor.value.split('\n').length;
  let html = '';
  for (let i = 1; i <= lineCount; i++) {
    html += `${i}<br/>`;
  }
  gutter.innerHTML = html;
}

function updateEditorMeta() {
  const editor = document.getElementById('code-editor');
  const metaEl = document.getElementById('editor-meta');
  if (!editor || !metaEl) return;

  const count = editor.value.split('\n').length;
  metaEl.textContent = `${count} lines | UTF-8`;
}

// 7. Global Keyboard Shortcuts
function initKeyboardShortcuts() {
  window.addEventListener('keydown', (e) => {
    // Ctrl+S or Cmd+S -> Save
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      saveActiveFile();
      return;
    }
    // Shift+Enter in editor -> Run current block
    if (e.shiftKey && e.key === 'Enter') {
      if (document.activeElement === document.getElementById('code-editor')) {
        e.preventDefault();
        runCurrentBlock();
        return;
      }
    }
    // Ctrl+I or Cmd+I -> Import modal
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'i') {
      e.preventDefault();
      openImportModal();
      return;
    }
    // Escape -> Close modal
    if (e.key === 'Escape') {
      closeImportModal();
    }
  });
}

// 8. Multi-format Importer Modal
function openImportModal() {
  const modal = document.getElementById('import-modal');
  if (modal) modal.style.display = 'flex';
  const textarea = document.getElementById('import-textarea');
  if (textarea) textarea.focus();
}

function closeImportModal() {
  const modal = document.getElementById('import-modal');
  if (modal) modal.style.display = 'none';
}

async function submitImport() {
  const content = document.getElementById('import-textarea').value.trim();
  if (!content) return;

  const format = document.getElementById('import-format-select').value;
  const templatizeHost = document.getElementById('import-opt-baseurl').checked;
  const generateMock = document.getElementById('import-opt-mock').checked;

  try {
    const res = await fetch('/api/v1/import', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        format,
        content,
        templatize_host: templatizeHost,
        generate_mock: generateMock
      })
    });
    const json = await res.json();
    if (json.success && json.data) {
      const generatedCode = json.data.markdown_blocks.join('\n\n');
      insertIntoEditor(generatedCode);
      closeImportModal();
      document.getElementById('import-textarea').value = '';
    } else {
      alert('Import failed: ' + (json.error || 'Invalid format'));
    }
  } catch (e) {
    alert('Request exception: ' + e.message);
  }
}

function insertIntoEditor(text) {
  const editor = document.getElementById('code-editor');
  const start = editor.selectionStart ?? editor.value.length;
  const end = editor.selectionEnd ?? editor.value.length;
  const current = editor.value;
  editor.value = current.substring(0, start) + '\n\n' + text + '\n\n' + current.substring(end);
  updateGutter();
  updateEditorMeta();
  if (editorMode !== 'source') {
    renderMarkdownPreview();
  }
}

// 6.1 Markdown & HTTP Card View Mode + Live Preview Renderer
function isMarkdownFile(path) {
  return typeof path === 'string' && path.toLowerCase().endsWith('.md');
}

function isHttpFile(path) {
  return typeof path === 'string' && path.toLowerCase().endsWith('.http');
}

function syncEditorModeForFile(path) {
  const modeSwitch = document.getElementById('editor-mode-switch');
  const splitBtn = document.querySelector('#editor-mode-switch .mode-btn[data-mode="split"]');
  if (isMarkdownFile(path)) {
    if (modeSwitch) modeSwitch.style.display = 'inline-flex';
    if (splitBtn && splitBtn.style) splitBtn.style.display = '';
    setEditorMode('preview');
  } else if (isHttpFile(path)) {
    if (modeSwitch) modeSwitch.style.display = 'inline-flex';
    if (splitBtn && splitBtn.style) splitBtn.style.display = 'none';
    setEditorMode('preview');
  } else {
    if (modeSwitch) modeSwitch.style.display = 'none';
    setEditorMode('source');
  }
}

function setEditorMode(mode) {
  editorMode = mode;
  const sourcePane = document.getElementById('editor-source-pane');
  const previewPane = document.getElementById('markdown-preview');
  const wrapper = document.getElementById('editor-wrapper');

  if (sourcePane) {
    sourcePane.style.display = mode === 'preview' ? 'none' : 'flex';
  }
  if (previewPane) {
    previewPane.style.display = mode === 'source' ? 'none' : 'block';
  }
  if (wrapper && wrapper.classList) {
    wrapper.classList.toggle('is-split', mode === 'split');
  }

  document.querySelectorAll('#editor-mode-switch .mode-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.mode === mode);
  });

  if (mode === 'preview' || mode === 'split') {
    renderMarkdownPreview();
  }
}

function scheduleMarkdownRender() {
  if (markdownRenderTimer) clearTimeout(markdownRenderTimer);
  markdownRenderTimer = setTimeout(() => {
    renderMarkdownPreview();
  }, 120);
}

function upgradePreviewCodeBlocks(previewPane) {
  if (!previewPane || !previewPane.querySelectorAll) return;

  // 1. Upgrade Markdown ```http / ```rest blocks into unified HTTP Request Cards
  previewPane.querySelectorAll('.md-http-block').forEach((blockEl) => {
    const runBtn = blockEl.querySelector('.md-run-btn');
    const idx = runBtn ? Number(runBtn.dataset.blockIndex) : -1;
    const rawBlock = lastRenderedExecBlocks[idx];
    if (!Number.isNaN(idx) && typeof rawBlock === 'string') {
      const wrapper = document.createElement('div');
      wrapper.innerHTML = RupostCards.renderHttpCardHtml(rawBlock, idx, {
        readonly: isReadonly,
        editedBody: cardEditedBodies.get(idx),
      });
      if (wrapper.firstElementChild) {
        blockEl.replaceWith(wrapper.firstElementChild);
      }
    }
  });

  // 2. Highlight ```json fenced blocks
  previewPane.querySelectorAll('.md-code-block').forEach((blockEl) => {
    const langEl = blockEl.querySelector('.md-code-lang');
    if (langEl && langEl.textContent === 'JSON') {
      const codeEl = blockEl.querySelector('pre code');
      if (codeEl) {
        codeEl.innerHTML = RupostCards.highlightJson(codeEl.textContent || '');
      }
    }
  });
}

async function renderMarkdownPreview() {
  const editor = document.getElementById('code-editor');
  const previewPane = document.getElementById('markdown-preview');
  if (!editor || !previewPane) return;

  if (isHttpFile(currentFilePath)) {
    const extracted = RupostCards.extractHttpFileBlocks(editor.value || '');
    currentHttpFileVars = extracted.fileVariables || {};
    lastRenderedExecBlocks = extracted.blocks || [];
    if (lastRenderedExecBlocks.length === 0) {
      previewPane.innerHTML = '<div class="empty-hint"><div class="empty-icon">[EMPTY HTTP FILE]</div><div class="empty-text">Switch to [Source] to add HTTP requests.</div></div>';
      return;
    }
    const cardsHtml = lastRenderedExecBlocks
      .map((blk, idx) =>
        RupostCards.renderHttpCardHtml(blk, idx, {
          readonly: isReadonly,
          editedBody: cardEditedBodies.get(idx),
        })
      )
      .join('');
    previewPane.innerHTML = `<div class="http-cards-stack">${cardsHtml}</div>`;
    syncMarkdownRunButtonsReadonly();
    return;
  }

  currentHttpFileVars = {};
  const seq = ++renderSeq;
  try {
    const res = await fetch('/api/v1/markdown/render', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ content: editor.value || '' })
    });
    const json = await res.json();
    if (seq !== renderSeq) return;
    if (json.success && json.data) {
      lastRenderedExecBlocks = Array.isArray(json.data.executable_blocks)
        ? json.data.executable_blocks
        : [];
      previewPane.innerHTML = `<article class="markdown-body">${json.data.html || ''}</article>`;
      upgradePreviewCodeBlocks(previewPane);
      syncMarkdownRunButtonsReadonly();
    }
  } catch (e) {
    if (seq === renderSeq) {
      previewPane.innerHTML = `<div class="assertion-item" style="border-color: var(--accent-fail);"><span class="assert-tag fail">[RENDER ERR]</span><span class="assert-expr">${escapeHtml(e.message)}</span></div>`;
    }
  }
}

function syncMarkdownRunButtonsReadonly() {
  const previewPane = document.getElementById('markdown-preview');
  if (!previewPane || !previewPane.querySelectorAll) return;
  previewPane
    .querySelectorAll('.md-run-btn, .http-card-run-btn, .http-card-add-body-btn, .http-card-fmt-btn, .http-card-reset-btn')
    .forEach(btn => {
      btn.disabled = isReadonly;
      if (isReadonly) {
        btn.title = '只读模式下不可用';
      }
    });
  previewPane.querySelectorAll('.http-card-body-editor').forEach(ta => {
    ta.readOnly = isReadonly;
  });
}

function resolveRelativeDocPath(baseFilePath, href) {
  if (!href || typeof href !== 'string') return null;
  const trimmed = href.trim();
  if (
    !trimmed ||
    trimmed.startsWith('#') ||
    /^(https?:|mailto:|javascript:|data:)/i.test(trimmed)
  ) {
    return null;
  }

  const cleanHref = trimmed.split('#')[0].split('?')[0];
  if (!cleanHref || !/\.(md|http)$/i.test(cleanHref)) {
    return null;
  }

  const baseParts =
    !cleanHref.startsWith('/') && baseFilePath
      ? String(baseFilePath).split('/').slice(0, -1)
      : [];
  const rawParts = cleanHref.replace(/^\/+/, '').split('/');
  const resolved = [...baseParts];

  for (const seg of rawParts) {
    if (!seg || seg === '.') continue;
    if (seg === '..') {
      if (resolved.length === 0) return null;
      resolved.pop();
    } else {
      resolved.push(seg);
    }
  }

  return resolved.length > 0 ? resolved.join('/') : null;
}

function getEffectiveCardBlock(idx) {
  const rawBlock = lastRenderedExecBlocks[idx];
  if (typeof rawBlock !== 'string') return null;
  if (cardEditedBodies.has(idx)) {
    return RupostCards.rebuildBlockWithBody(rawBlock, cardEditedBodies.get(idx));
  }
  return rawBlock;
}

function rerenderSingleCard(cardEl, idx) {
  const rawBlock = lastRenderedExecBlocks[idx];
  if (!cardEl || typeof rawBlock !== 'string') return;
  const wrapper = document.createElement('div');
  wrapper.innerHTML = RupostCards.renderHttpCardHtml(rawBlock, idx, {
    readonly: isReadonly,
    editedBody: cardEditedBodies.get(idx),
  });
  if (wrapper.firstElementChild) {
    cardEl.replaceWith(wrapper.firstElementChild);
  }
}

function initMarkdownPreviewEvents() {
  const previewPane = document.getElementById('markdown-preview');
  if (!previewPane) return;

  previewPane.addEventListener('input', (e) => {
    const editorEl = e.target.closest ? e.target.closest('.http-card-body-editor') : null;
    if (!editorEl) return;
    const idx = Number(editorEl.dataset.cardIndex);
    if (Number.isNaN(idx)) return;
    const rawBlock = lastRenderedExecBlocks[idx];
    const origBody = typeof rawBlock === 'string' ? RupostCards.parseHttpBlock(rawBlock).body : '';
    const nextVal = editorEl.value;
    if (nextVal === origBody) {
      cardEditedBodies.delete(idx);
    } else {
      cardEditedBodies.set(idx, nextVal);
    }
    const card = editorEl.closest('.http-card');
    if (card) {
      const resetBtn = card.querySelector('.http-card-reset-btn');
      if (resetBtn) resetBtn.style.display = cardEditedBodies.has(idx) ? '' : 'none';
      const label = card.querySelector('.http-card-section-label');
      if (label) {
        label.innerHTML = `Request Body ${cardEditedBodies.has(idx) ? '<span class="modified-pill">[Modified]</span>' : ''}`;
      }
    }
  });

  previewPane.addEventListener('click', (e) => {
    const runBtn = e.target.closest ? e.target.closest('.http-card-run-btn, .md-run-btn') : null;
    if (runBtn) {
      if (isReadonly) return;
      const idx = Number(runBtn.dataset.cardIndex ?? runBtn.dataset.blockIndex);
      const effectiveBlock = getEffectiveCardBlock(idx);
      if (effectiveBlock) {
        executeRawBlock(effectiveBlock, runBtn, currentHttpFileVars);
      }
      return;
    }

    const addBodyBtn = e.target.closest ? e.target.closest('.http-card-add-body-btn') : null;
    if (addBodyBtn) {
      if (isReadonly) return;
      const card = addBodyBtn.closest('.http-card');
      const ta = card && card.querySelector('.http-card-body-editor');
      const fmtBtn = card && card.querySelector('.http-card-fmt-btn');
      if (ta) {
        ta.style.display = 'block';
        ta.focus();
      }
      if (fmtBtn) fmtBtn.style.display = '';
      addBodyBtn.style.display = 'none';
      return;
    }

    const fmtBtn = e.target.closest ? e.target.closest('.http-card-fmt-btn') : null;
    if (fmtBtn) {
      if (isReadonly) return;
      const idx = Number(fmtBtn.dataset.cardIndex);
      const card = fmtBtn.closest('.http-card');
      const ta = card && card.querySelector('.http-card-body-editor');
      if (ta) {
        try {
          const formatted = JSON.stringify(JSON.parse(ta.value), null, 2);
          ta.value = formatted;
          cardEditedBodies.set(idx, formatted);
          rerenderSingleCard(card, idx);
        } catch (err) {
          // Non-JSON body: ignore
        }
      }
      return;
    }

    const resetBtn = e.target.closest ? e.target.closest('.http-card-reset-btn') : null;
    if (resetBtn) {
      if (isReadonly) return;
      const idx = Number(resetBtn.dataset.cardIndex);
      cardEditedBodies.delete(idx);
      rerenderSingleCard(resetBtn.closest('.http-card'), idx);
      return;
    }

    const link = e.target.closest ? e.target.closest('a[href]') : null;
    if (!link) return;
    const rawHref = link.getAttribute('href') || '';
    const resolvedDoc = resolveRelativeDocPath(currentFilePath, rawHref);
    if (resolvedDoc) {
      e.preventDefault();
      openFile(resolvedDoc);
    } else if (/^https?:\/\//i.test(rawHref)) {
      e.preventDefault();
      window.open(rawHref, '_blank', 'noopener,noreferrer');
    }
  });
}

function escapeHtml(str) {
  if (!str) return '';
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

if (typeof module !== 'undefined' && module.exports) {
  module.exports = {
    applyReadonlyState,
    runSuite,
    executeRawBlock,
    escapeHtml,
    applyTheme,
    toggleTheme,
    toggleDir,
    renderFileList,
    filterFiles,
    isMarkdownFile,
    setEditorMode,
    syncEditorModeForFile,
    renderMarkdownPreview,
    resolveRelativeDocPath,
    insertIntoEditor,
    setCurrentFilePath: (p) => { currentFilePath = p; },
    getCurrentFilePath: () => currentFilePath,
    setAllFiles: (f) => { allFiles = f; },
    getAllFiles: () => allFiles,
    getExpandedDirs: () => Array.from(expandedDirs),
    clearTreeState: () => {
      expandedDirs.clear();
      dirCache.clear();
      loadingDirs.clear();
    },
  };
}
