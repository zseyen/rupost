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
let lastExecutedUrl = '';
let isReadonly = false;
let currentResponseBodyText = '';

const expandedDirs = new Set();
const dirCache = new Map();
const loadingDirs = new Set();

let editorMode = 'source';
let lastRenderedExecBlocks = [];
let lastRenderedCards = [];
let cardEditedBodies = new Map();
let currentHttpFileVars = {};
let markdownRenderTimer = null;
let renderSeq = 0;
let exportTargetBlock = '';
let mockDraft = {
  method: 'GET',
  path: '/',
  headers: '',
  query: '',
  body: '',
  raw_block: '',
  lastResult: null,
};

// Initialize theme immediately to prevent FOUC
if (typeof window !== 'undefined' && typeof document !== 'undefined') {
  initTheme();

  document.addEventListener('DOMContentLoaded', () => {
    initCustomSelects();
    loadFiles();
    loadEnvs();
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
    isReadonly = Boolean(json.success && json.data && json.data.readonly);
    if (isReadonly) {
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

// 0.1 Environment Selector
async function loadEnvs() {
  const selectEl = document.getElementById('env-select');
  if (!selectEl) return;
  try {
    const res = await fetch('/api/v1/envs');
    const json = await res.json();
    if (json.success && json.data) {
      const envs = Array.isArray(json.data.environments) ? json.data.environments : [];
      const def = json.data.default_env || '';
      selectEl.innerHTML = '';
      const defOpt = document.createElement('option');
      defOpt.value = '';
      defOpt.textContent = '[ENV: default]';
      selectEl.appendChild(defOpt);
      envs.forEach((name) => {
        const opt = document.createElement('option');
        opt.value = name;
        opt.textContent = `[ENV: ${name}]`;
        selectEl.appendChild(opt);
      });
      if (def && envs.includes(def)) {
        selectEl.value = def;
      }
      syncCustomSelect(selectEl);
    }
  } catch (e) {
    console.error('Failed to load environments:', e);
  }
}

function getSelectedEnv() {
  const selectEl = document.getElementById('env-select');
  if (!selectEl || !selectEl.value) return null;
  return selectEl.value;
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
  lastExecutedUrl = RupostCards.parseHttpBlock(rawBlock).url || '';
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
    const envName = getSelectedEnv();
    if (envName) payload.env = envName;
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
      const parsed = RupostCards.parseHttpBlock(rawBlock);
      const diagBtn =
        !isReadonly && parsed.url
          ? `<button type="button" class="btn btn-secondary btn-sm" style="margin-left:auto;" onclick="runDiagnoseForUrl('${escapeHtml(parsed.url)}')">[Diagnose]</button>`
          : '';
      debugEl.innerHTML = `<div class="assertion-item" style="border-color: var(--accent-fail);"><span class="assert-tag fail">[EXEC ERR]</span><span class="assert-expr">${escapeHtml(json.error || 'Execution failed')}</span>${diagBtn}</div>`;
    }
  } catch (e) {
    const parsed = RupostCards.parseHttpBlock(rawBlock);
    const diagBtn =
      !isReadonly && parsed.url
        ? `<button type="button" class="btn btn-secondary btn-sm" style="margin-left:auto;" onclick="runDiagnoseForUrl('${escapeHtml(parsed.url)}')">[Diagnose]</button>`
        : '';
    debugEl.innerHTML = `<div class="assertion-item" style="border-color: var(--accent-fail);"><span class="assert-tag fail">[NETWORK ERR]</span><span class="assert-expr">${escapeHtml(e.message)}</span>${diagBtn}</div>`;
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
    const suitePayload = { path: currentFilePath };
    const envName = getSelectedEnv();
    if (envName) suitePayload.env = envName;

    const res = await fetch('/api/v1/run/suite', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(suitePayload)
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
  if (tab === 'mock') {
    renderMockTab(debugEl);
    return;
  }
  if (tab === 'session') {
    renderSessionTab(debugEl);
    return;
  }
  if (tab === 'history') {
    renderHistoryTab(debugEl);
    return;
  }
  if (tab === 'ws') {
    renderWsTab(debugEl);
    return;
  }
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
  const diagBtn =
    !isReadonly && lastExecutedUrl
      ? `<button type="button" class="btn btn-secondary btn-sm" onclick="runDiagnoseForUrl('${escapeHtml(lastExecutedUrl)}')">[Diagnose]</button>`
      : '';
  const errorHtml = d.error
    ? `<div class="error-banner">
         <div class="error-banner-head">
           <span>[EXECUTION ERROR]</span>
           ${diagBtn}
         </div>
         <div>${escapeHtml(d.error)}</div>
       </div>`
    : '';
  const warnHtml =
    Array.isArray(d.warnings) && d.warnings.length > 0
      ? `<div class="warn-banner"><strong>[WARNINGS]</strong> ${d.warnings.map(escapeHtml).join('<br/>')}</div>`
      : '';

  let html = `
    ${errorHtml}
    ${warnHtml}
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
    // Escape -> Close modal & custom selects
    if (e.key === 'Escape') {
      closeImportModal();
      closeExportModal();
      closeAllCustomSelects();
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
    const cardData = lastRenderedCards[idx] || lastRenderedExecBlocks[idx];
    if (!Number.isNaN(idx) && cardData) {
      const wrapper = document.createElement('div');
      wrapper.innerHTML = RupostCards.renderHttpCardHtml(cardData, idx, {
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

async function safeReadJson(res) {
  if (typeof res.text === 'function') {
    const text = await res.text();
    if (!text || !text.trim()) return null;
    try {
      return JSON.parse(text);
    } catch (e) {
      return null;
    }
  }
  if (typeof res.json === 'function') {
    try {
      return await res.json();
    } catch (e) {
      return null;
    }
  }
  return null;
}

async function renderMarkdownPreview() {
  const editor = document.getElementById('code-editor');
  const previewPane = document.getElementById('markdown-preview');
  if (!editor || !previewPane) return;

  const seq = ++renderSeq;

  if (isHttpFile(currentFilePath)) {
    try {
      const res = await fetch('/api/v1/http/parse', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ content: editor.value || '' }),
      });
      const json = await safeReadJson(res);
      if (seq !== renderSeq) return;

      // 兼容旧版二进制 + --static-dir 场景：404/405 或空响应时回退客户端解析
      if (res.status === 404 || res.status === 405 || (!json && !res.ok)) {
        const extracted = RupostCards.extractHttpFileBlocks(editor.value || '');
        currentHttpFileVars = extracted.fileVariables || {};
        lastRenderedExecBlocks = extracted.blocks || [];
        lastRenderedCards = [];
        const cardsHtml = lastRenderedExecBlocks
          .map((blk, idx) =>
            RupostCards.renderHttpCardHtml(blk, idx, {
              readonly: isReadonly,
              editedBody: cardEditedBodies.get(idx),
            })
          )
          .join('');
        previewPane.innerHTML = lastRenderedExecBlocks.length
          ? `<div class="http-cards-stack">${cardsHtml}</div>`
          : '<div class="empty-hint"><div class="empty-icon">[EMPTY HTTP FILE]</div><div class="empty-text">Switch to [Source] to add HTTP requests.</div></div>';
        syncMarkdownRunButtonsReadonly();
        return;
      }

      if (!json || !json.success || !json.data) {
        previewPane.innerHTML = `
          <div class="error-banner" style="margin: 20px;">
            <div class="error-banner-head">
              <span>[PARSE ERR]</span>
              <button type="button" class="btn btn-secondary btn-sm" onclick="setEditorMode('source')">[Switch to Source]</button>
            </div>
            <div>${escapeHtml((json && json.error) || `HTTP ${res.status}: Failed to parse .http document`)}</div>
          </div>
        `;
        return;
      }
      currentHttpFileVars = json.data.variables || {};
      lastRenderedCards = Array.isArray(json.data.cards) ? json.data.cards : [];
      lastRenderedExecBlocks = lastRenderedCards.map((c) => c.raw_block);
      if (lastRenderedCards.length === 0) {
        previewPane.innerHTML = '<div class="empty-hint"><div class="empty-icon">[EMPTY HTTP FILE]</div><div class="empty-text">Switch to [Source] to add HTTP requests.</div></div>';
        return;
      }
      const cardsHtml = lastRenderedCards
        .map((card, idx) =>
          RupostCards.renderHttpCardHtml(card, idx, {
            readonly: isReadonly,
            editedBody: cardEditedBodies.get(idx),
          })
        )
        .join('');
      previewPane.innerHTML = `<div class="http-cards-stack">${cardsHtml}</div>`;
      syncMarkdownRunButtonsReadonly();
    } catch (e) {
      if (seq === renderSeq) {
        previewPane.innerHTML = `<div class="error-banner" style="margin: 20px;"><div class="error-banner-head"><span>[PARSE ERR]</span><button type="button" class="btn btn-secondary btn-sm" onclick="setEditorMode('source')">[Switch to Source]</button></div><div>${escapeHtml(e.message)}</div></div>`;
      }
    }
    return;
  }

  currentHttpFileVars = {};
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
      lastRenderedCards = Array.isArray(json.data.executable_cards)
        ? json.data.executable_cards
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
  const cardData = lastRenderedCards[idx] || lastRenderedExecBlocks[idx];
  if (!cardEl || !cardData) return;
  const wrapper = document.createElement('div');
  wrapper.innerHTML = RupostCards.renderHttpCardHtml(cardData, idx, {
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

    const exportBtn = e.target.closest ? e.target.closest('.http-card-export-btn') : null;
    if (exportBtn) {
      const idx = Number(exportBtn.dataset.cardIndex);
      const effectiveBlock = getEffectiveCardBlock(idx);
      if (effectiveBlock) {
        openExportModal(effectiveBlock);
      }
      return;
    }

    const mockBtn = e.target.closest ? e.target.closest('.http-card-mock-btn') : null;
    if (mockBtn) {
      const idx = Number(mockBtn.dataset.cardIndex);
      const effectiveBlock = getEffectiveCardBlock(idx);
      if (effectiveBlock) {
        openMockSimulatorForBlock(effectiveBlock);
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

// 9. Block Export Modal (POST /api/v1/export)
function stripMarkdownFence(raw) {
  const lines = String(raw || '').trim().split('\n');
  if (lines.length >= 2 && lines[0].trim().startsWith('```') && lines[lines.length - 1].trim() === '```') {
    return lines.slice(1, -1).join('\n');
  }
  return String(raw || '');
}

async function openExportModal(rawBlock) {
  exportTargetBlock = stripMarkdownFence(rawBlock);
  const modal = document.getElementById('export-modal');
  if (modal) modal.style.display = 'flex';
  await refreshExportCommand();
}

function closeExportModal() {
  const modal = document.getElementById('export-modal');
  if (modal) modal.style.display = 'none';
}

async function refreshExportCommand() {
  const preEl = document.getElementById('export-command-pre');
  const warnEl = document.getElementById('export-warnings');
  const fmtEl = document.getElementById('export-format-select');
  if (!preEl) return;

  const format = fmtEl ? fmtEl.value : 'curl';
  preEl.textContent = 'Generating...';
  if (warnEl) warnEl.textContent = '';

  const payload = {
    raw_block: exportTargetBlock,
    format,
    file_path: currentFilePath || undefined,
  };
  const envName = getSelectedEnv();
  if (envName) payload.env = envName;
  if (currentHttpFileVars && Object.keys(currentHttpFileVars).length > 0) {
    payload.extra_variables = currentHttpFileVars;
  }

  try {
    const res = await fetch('/api/v1/export', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    const json = await res.json();
    if (json.success && json.data) {
      preEl.textContent = json.data.command || '';
      if (warnEl && Array.isArray(json.data.warnings) && json.data.warnings.length > 0) {
        warnEl.textContent = json.data.warnings.join(' | ');
      }
    } else {
      preEl.textContent = `[ERR] ${json.error || 'Export failed'}`;
    }
  } catch (e) {
    preEl.textContent = `[NETWORK ERR] ${e.message}`;
  }
}

function copyExportCommand() {
  const preEl = document.getElementById('export-command-pre');
  if (!preEl || !navigator.clipboard) return;
  navigator.clipboard.writeText(preEl.textContent || '').then(() => {
    const btn = document.getElementById('btn-copy-export');
    if (btn) {
      const orig = btn.textContent;
      btn.textContent = '[Copied!]';
      setTimeout(() => { btn.textContent = orig; }, 1500);
    }
  });
}

// 10. Mock Rule Simulator Tab (POST /api/v1/mock/simulate)
function parseKvInput(raw) {
  const trimmed = String(raw || '').trim();
  if (!trimmed) return {};
  if (trimmed.startsWith('{')) {
    try {
      const parsed = JSON.parse(trimmed);
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        const out = {};
        Object.entries(parsed).forEach(([k, v]) => {
          out[k] = String(v);
        });
        return out;
      }
    } catch (e) {
      // fall through to line parser
    }
  }
  const map = {};
  trimmed.split('\n').forEach((line) => {
    const sepIdx = line.includes(':') ? line.indexOf(':') : line.indexOf('=');
    if (sepIdx > 0) {
      const k = line.slice(0, sepIdx).trim();
      const v = line.slice(sepIdx + 1).trim();
      if (k) map[k] = v;
    }
  });
  return map;
}

function extractPathFromUrl(rawUrl) {
  if (!rawUrl) return '/';
  try {
    const u = new URL(rawUrl.replace(/\{\{[^}]+\}\}/g, 'placeholder'));
    return u.pathname || '/';
  } catch (e) {
    const clean = rawUrl.split('?')[0];
    return clean.startsWith('/') ? clean : '/';
  }
}

function openMockSimulatorForBlock(rawBlock) {
  const parsed = RupostCards.parseHttpBlock(rawBlock);
  mockDraft.method = parsed.method || 'GET';
  mockDraft.path = extractPathFromUrl(parsed.url);
  mockDraft.body = parsed.body || '';
  mockDraft.raw_block = rawBlock;
  mockDraft.lastResult = null;
  switchInspectorTab('mock');
}

function renderMockTab(container) {
  const simResult = mockDraft.lastResult;
  let resultHtml = '';
  if (simResult) {
    if (simResult.error) {
      resultHtml = `<div class="assertion-item"><span class="assert-tag fail">[ERR]</span><span class="assert-expr">${escapeHtml(simResult.error)}</span></div>`;
    } else {
      const evalsHtml = (simResult.evaluations || [])
        .map(
          (ev) => `
          <li class="assertion-item">
            <span class="assert-tag ${ev.passed ? 'pass' : 'fail'}">${ev.passed ? '[PASS]' : '[FAIL]'}</span>
            <span class="assert-expr">${escapeHtml(ev.condition_str)}</span>
            <span class="assert-detail">Expected: ${escapeHtml(ev.expected)} | Actual: ${escapeHtml(ev.actual ?? '<none>')}</span>
          </li>`
        )
        .join('');
      resultHtml = `
        <div class="timing-panel">
          <div class="timing-header">
            <span>[MATCHED] ${escapeHtml(simResult.matched_variant_name)}</span>
            <span>HTTP ${simResult.expected_status}</span>
          </div>
        </div>
        ${evalsHtml ? `<ul class="assertions-list">${evalsHtml}</ul>` : '<div class="empty-hint"><div class="empty-text">Matched default fallback variant (no conditions).</div></div>'}
      `;
    }
  }

  container.innerHTML = `
    <div class="mock-probe-form">
      <div class="mock-probe-row">
        <select id="mock-method" class="env-select" style="width: 96px;">
          ${['GET', 'POST', 'PUT', 'DELETE', 'PATCH'].map((m) => `<option value="${m}" ${mockDraft.method === m ? 'selected' : ''}>${m}</option>`).join('')}
        </select>
        <input id="mock-path" class="mock-input" type="text" value="${escapeHtml(mockDraft.path)}" placeholder="/api/resource" />
      </div>
      <textarea id="mock-headers" class="mock-textarea" placeholder="Headers (JSON or Key: Value per line)">${escapeHtml(mockDraft.headers)}</textarea>
      <textarea id="mock-query" class="mock-textarea" placeholder="Query (JSON or key=value per line)">${escapeHtml(mockDraft.query)}</textarea>
      <textarea id="mock-body" class="mock-textarea" placeholder="Request Body (optional)">${escapeHtml(mockDraft.body)}</textarea>
      <button type="button" id="btn-mock-simulate" class="btn btn-primary" onclick="runMockSimulate()">&gt; Simulate Mock Match</button>
    </div>
    <div id="mock-sim-result">${resultHtml}</div>
  `;
  enhanceSelect(document.getElementById('mock-method'));
}

async function runMockSimulate() {
  const methodEl = document.getElementById('mock-method');
  const pathEl = document.getElementById('mock-path');
  const headersEl = document.getElementById('mock-headers');
  const queryEl = document.getElementById('mock-query');
  const bodyEl = document.getElementById('mock-body');

  if (methodEl) mockDraft.method = methodEl.value;
  if (pathEl) mockDraft.path = pathEl.value;
  if (headersEl) mockDraft.headers = headersEl.value;
  if (queryEl) mockDraft.query = queryEl.value;
  if (bodyEl) mockDraft.body = bodyEl.value;

  const rawBlock = mockDraft.raw_block || (lastRenderedExecBlocks[0] || '');
  if (!rawBlock) {
    mockDraft.lastResult = { error: 'Select a document or click [Mock] on a request card first.' };
    renderMockTab(document.getElementById('debug-output'));
    return;
  }

  try {
    const res = await fetch('/api/v1/mock/simulate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        method: mockDraft.method,
        path: mockDraft.path,
        headers: parseKvInput(mockDraft.headers),
        query: parseKvInput(mockDraft.query),
        body: mockDraft.body || null,
        raw_block: rawBlock,
      }),
    });
    const json = await res.json();
    if (json.success && json.data) {
      mockDraft.lastResult = json.data;
    } else {
      mockDraft.lastResult = { error: json.error || 'Simulation failed' };
    }
  } catch (e) {
    mockDraft.lastResult = { error: e.message };
  }

  if (activeInspectorTab === 'mock') {
    renderMockTab(document.getElementById('debug-output'));
  }
}

// 11. P2 Observability: Session, History, Diagnose, WebSocket Bridge
let activeWsBridge = null;
let wsLogEntries = [];

async function renderSessionTab(container) {
  container.innerHTML = '<div class="empty-hint"><div class="empty-icon">[LOADING]</div><div class="empty-text">Loading session state...</div></div>';
  try {
    const res = await fetch('/api/v1/session');
    const json = await res.json();
    if (!json.success || !json.data) {
      container.innerHTML = `<div class="assertion-item"><span class="assert-tag fail">[ERR]</span><span class="assert-expr">${escapeHtml(json.error || 'Failed to load session')}</span></div>`;
      return;
    }
    const { captured_variables: vars = {}, has_cookie_state } = json.data;
    const keys = Object.keys(vars);
    const rowsHtml = keys.length
      ? `<table class="headers-table"><thead><tr><th>Variable</th><th>Value (Masked)</th></tr></thead><tbody>${keys
          .map((k) => `<tr><td class="header-name">${escapeHtml(k)}</td><td class="header-value">${escapeHtml(vars[k])}</td></tr>`)
          .join('')}</tbody></table>`
      : '<div class="empty-hint"><div class="empty-text">No @capture variables recorded in current browser session yet.</div></div>';

    container.innerHTML = `
      <div class="timing-panel">
        <div class="timing-header">
          <span>[SESSION STATE]</span>
          <button type="button" class="btn btn-secondary btn-sm" onclick="switchInspectorTab('session')">[Refresh]</button>
        </div>
        <div style="font-family: var(--font-mono); font-size: 11px; margin-top: 6px; color: var(--text-secondary);">
          Upstream Cookie Bridge: <strong>${has_cookie_state ? 'Active' : 'Empty'}</strong> | Variables: <strong>${keys.length}</strong>
        </div>
      </div>
      ${rowsHtml}
    `;
  } catch (e) {
    container.innerHTML = `<div class="assertion-item"><span class="assert-tag fail">[NETWORK ERR]</span><span class="assert-expr">${escapeHtml(e.message)}</span></div>`;
  }
}

async function renderHistoryTab(container) {
  if (isReadonly) {
    container.innerHTML = '<div class="empty-hint"><div class="empty-icon">[LOCKED]</div><div class="empty-text">History access is disabled in read-only mode.</div></div>';
    return;
  }
  container.innerHTML = '<div class="empty-hint"><div class="empty-icon">[LOADING]</div><div class="empty-text">Loading workspace history...</div></div>';
  try {
    const res = await fetch('/api/v1/history?limit=25');
    const json = await res.json();
    if (!json.success || !Array.isArray(json.data)) {
      container.innerHTML = `<div class="assertion-item"><span class="assert-tag fail">[ERR]</span><span class="assert-expr">${escapeHtml(json.error || 'Failed to load history')}</span></div>`;
      return;
    }
    if (json.data.length === 0) {
      container.innerHTML = '<div class="empty-hint"><div class="empty-icon">[EMPTY HISTORY]</div><div class="empty-text">No workspace-scoped history entries found.</div></div>';
      return;
    }
    const itemsHtml = json.data
      .map(
        (item) => `
        <li class="assertion-item">
          <span class="assert-tag ${item.status < 400 ? 'pass' : 'fail'}">${escapeHtml(item.method)} ${item.status}</span>
          <span class="assert-expr">${escapeHtml(item.url)}</span>
          <span class="assert-detail">${item.duration_ms} ms | ${escapeHtml(item.source || '')}</span>
        </li>`
      )
      .join('');
    container.innerHTML = `
      <div class="timing-panel">
        <div class="timing-header">
          <span>[WORKSPACE HISTORY]</span>
          <button type="button" class="btn btn-secondary btn-sm" onclick="switchInspectorTab('history')">[Refresh]</button>
        </div>
      </div>
      <ul class="assertions-list">${itemsHtml}</ul>
    `;
  } catch (e) {
    container.innerHTML = `<div class="assertion-item"><span class="assert-tag fail">[NETWORK ERR]</span><span class="assert-expr">${escapeHtml(e.message)}</span></div>`;
  }
}

async function runDiagnoseForUrl(targetUrl) {
  const debugEl = document.getElementById('debug-output');
  if (!debugEl) return;
  if (isReadonly) {
    debugEl.innerHTML = '<div class="empty-hint"><div class="empty-icon">[LOCKED]</div><div class="empty-text">Diagnose is disabled in read-only mode.</div></div>';
    return;
  }
  debugEl.innerHTML = `<div class="empty-hint"><div class="empty-icon">[DIAGNOSING]</div><div class="empty-text">Probing ${escapeHtml(targetUrl)}...</div></div>`;
  try {
    const res = await fetch('/api/v1/diagnose', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ url: targetUrl }),
    });
    const json = await res.json();
    if (!json.success || !json.data) {
      const hint = res.status === 403 ? ' (If targeting a local host, pass --allowed-hosts)' : '';
      debugEl.innerHTML = `<div class="assertion-item"><span class="assert-tag fail">[DIAGNOSE ERR]</span><span class="assert-expr">${escapeHtml((json.error || 'Diagnose failed') + hint)}</span></div>`;
      return;
    }
    const r = json.data;
    const certHtml = r.cert_info
      ? `<div class="assertion-item"><span class="assert-tag pass">[TLS CERT]</span><span class="assert-expr">${escapeHtml(r.cert_info.subject)}</span><span class="assert-detail">Issuer: ${escapeHtml(r.cert_info.issuer)} | Days Left: ${r.cert_info.days_remaining}</span></div>`
      : '';
    const errHtml = r.connection_error
      ? `<div class="assertion-item"><span class="assert-tag fail">[CONN ERR]</span><span class="assert-expr">${escapeHtml(r.connection_error)}</span></div>`
      : '';
    debugEl.innerHTML = `
      <div class="timing-panel">
        <div class="timing-header">
          <span>[DIAGNOSE REPORT]</span>
          <span>Total: ${RupostCards.formatDurationObj(r.total_duration)}</span>
        </div>
        <div style="font-family: var(--font-mono); font-size: 11px; margin-top: 6px; display: grid; grid-template-columns: 1fr 1fr; gap: 6px;">
          <div>DNS: <strong>${RupostCards.formatDurationObj(r.dns_lookup_duration)}</strong></div>
          <div>TCP: <strong>${RupostCards.formatDurationObj(r.tcp_connect_duration)}</strong></div>
          <div>TLS: <strong>${RupostCards.formatDurationObj(r.tls_handshake_duration)}</strong></div>
          <div>TTFB: <strong>${RupostCards.formatDurationObj(r.ttfb)}</strong></div>
          <div>HTTP: <strong>${r.http_status || '-'} (${escapeHtml(r.http_version || '-')})</strong></div>
          <div>IPs: <strong>${escapeHtml((r.resolved_ips || []).join(', ') || '-')}</strong></div>
        </div>
      </div>
      ${certHtml}
      ${errHtml}
    `;
  } catch (e) {
    debugEl.innerHTML = `<div class="assertion-item"><span class="assert-tag fail">[NETWORK ERR]</span><span class="assert-expr">${escapeHtml(e.message)}</span></div>`;
  }
}

function renderWsTab(container) {
  if (isReadonly) {
    container.innerHTML = '<div class="empty-hint"><div class="empty-icon">[LOCKED]</div><div class="empty-text">WebSocket bridge is disabled in read-only mode.</div></div>';
    return;
  }
  const connected = activeWsBridge && activeWsBridge.readyState === 1;
  const logsHtml = wsLogEntries
    .map(
      (entry) => `
      <li class="assertion-item">
        <span class="assert-tag ${entry.dir === 'IN' ? 'pass' : entry.dir === 'ERR' ? 'fail' : ''}">[${escapeHtml(entry.dir)}]</span>
        <span class="assert-expr">${escapeHtml(entry.text)}</span>
      </li>`
    )
    .join('');

  container.innerHTML = `
    <div class="mock-probe-form">
      <div class="mock-probe-row">
        <input id="ws-bridge-url" class="mock-input" type="text" placeholder="wss://echo.websocket.events" value="wss://echo.websocket.events" />
        <button type="button" class="btn ${connected ? 'btn-secondary' : 'btn-primary'}" onclick="${connected ? 'disconnectWsBridge()' : 'connectWsBridge()'}">${connected ? 'Disconnect' : 'Connect'}</button>
      </div>
      <div class="mock-probe-row">
        <input id="ws-bridge-msg" class="mock-input" type="text" placeholder="Message frame to send..." ${connected ? '' : 'disabled'} />
        <button type="button" class="btn btn-accent" onclick="sendWsBridgeFrame()" ${connected ? '' : 'disabled'}>Send</button>
      </div>
    </div>
    <ul class="assertions-list">${logsHtml}</ul>
  `;
}

function appendWsLog(dir, text) {
  wsLogEntries.push({ dir, text });
  if (wsLogEntries.length > 100) wsLogEntries.shift();
  if (activeInspectorTab === 'ws') {
    renderWsTab(document.getElementById('debug-output'));
  }
}

function connectWsBridge() {
  const urlEl = document.getElementById('ws-bridge-url');
  const targetUrl = urlEl ? urlEl.value.trim() : '';
  if (!targetUrl) return;
  if (activeWsBridge) activeWsBridge.close();

  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const bridgeUrl = `${proto}//${window.location.host}/api/v1/ws/bridge?url=${encodeURIComponent(targetUrl)}`;
  const ws = new WebSocket(bridgeUrl);
  activeWsBridge = ws;

  ws.onopen = () => appendWsLog('SYS', `Connected bridge -> ${targetUrl}`);
  ws.onmessage = (ev) => appendWsLog('IN', String(ev.data));
  ws.onerror = () => appendWsLog('ERR', 'WebSocket bridge error (check SSRF / allowed-hosts)');
  ws.onclose = () => {
    appendWsLog('SYS', 'Bridge closed');
    activeWsBridge = null;
    if (activeInspectorTab === 'ws') renderWsTab(document.getElementById('debug-output'));
  };
}

function disconnectWsBridge() {
  if (activeWsBridge) {
    activeWsBridge.close();
    activeWsBridge = null;
  }
}

function sendWsBridgeFrame() {
  const msgEl = document.getElementById('ws-bridge-msg');
  const msg = msgEl ? msgEl.value : '';
  if (activeWsBridge && activeWsBridge.readyState === 1 && msg) {
    activeWsBridge.send(msg);
    appendWsLog('OUT', msg);
    if (msgEl) msgEl.value = '';
  }
}

// 12. Custom Positioned Select Dropdowns (Fixes native <select> OS popup offset)
function initCustomSelects() {
  ['env-select', 'import-format-select', 'export-format-select'].forEach((id) => {
    enhanceSelect(document.getElementById(id));
  });
  if (typeof document !== 'undefined' && document.addEventListener) {
    document.addEventListener('click', (e) => {
      if (!e.target.closest || !e.target.closest('.ui-select')) {
        closeAllCustomSelects();
      }
    });
  }
}

function closeAllCustomSelects(exceptEl = null) {
  if (!document.querySelectorAll) return;
  document.querySelectorAll('.ui-select.open').forEach((wrap) => {
    if (wrap !== exceptEl) wrap.classList.remove('open');
  });
}

function enhanceSelect(selectEl) {
  if (!selectEl || !selectEl.parentNode || selectEl.dataset.enhanced === '1') return;
  selectEl.dataset.enhanced = '1';

  const wrapper = document.createElement('div');
  wrapper.className = 'ui-select' + (selectEl.classList && selectEl.classList.contains('form-select') ? ' ui-select-full' : '');
  if (selectEl.style && selectEl.style.width) {
    wrapper.style.width = selectEl.style.width;
  }

  const trigger = document.createElement('button');
  trigger.type = 'button';
  trigger.className = 'ui-select-trigger';
  trigger.setAttribute('aria-haspopup', 'listbox');

  const labelSpan = document.createElement('span');
  labelSpan.className = 'ui-select-label';
  const caretSpan = document.createElement('span');
  caretSpan.className = 'ui-select-caret';
  caretSpan.textContent = 'v';
  trigger.appendChild(labelSpan);
  trigger.appendChild(caretSpan);

  const menu = document.createElement('ul');
  menu.className = 'ui-select-menu';
  menu.setAttribute('role', 'listbox');

  trigger.onclick = (e) => {
    e.stopPropagation();
    const isOpen = wrapper.classList.contains('open');
    closeAllCustomSelects(wrapper);
    wrapper.classList.toggle('open', !isOpen);
  };

  wrapper.appendChild(trigger);
  wrapper.appendChild(menu);

  selectEl.parentNode.insertBefore(wrapper, selectEl.nextSibling);
  selectEl.style.display = 'none';
  selectEl._customWrapper = wrapper;

  syncCustomSelect(selectEl);
}

function syncCustomSelect(selectEl) {
  if (!selectEl || !selectEl._customWrapper) return;
  const wrapper = selectEl._customWrapper;
  const labelSpan = wrapper.querySelector('.ui-select-label');
  const menu = wrapper.querySelector('.ui-select-menu');
  if (!menu || !labelSpan) return;

  menu.innerHTML = '';
  const options = Array.from(selectEl.options || []);
  const selectedOpt = options.find((o) => o.value === selectEl.value) || options[0];
  labelSpan.textContent = selectedOpt ? selectedOpt.textContent : '';

  options.forEach((opt) => {
    const li = document.createElement('li');
    li.className = 'ui-select-option' + (opt.value === selectEl.value ? ' selected' : '');
    li.setAttribute('role', 'option');
    li.textContent = opt.textContent;
    li.onclick = (e) => {
      e.stopPropagation();
      selectEl.value = opt.value;
      wrapper.classList.remove('open');
      syncCustomSelect(selectEl);
      if (typeof selectEl.onchange === 'function') {
        selectEl.onchange();
      } else if (typeof Event !== 'undefined' && selectEl.dispatchEvent) {
        selectEl.dispatchEvent(new Event('change'));
      }
    };
    menu.appendChild(li);
  });
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
    loadEnvs,
    getSelectedEnv,
    openExportModal,
    refreshExportCommand,
    runMockSimulate,
    parseKvInput,
    renderSessionTab,
    renderHistoryTab,
    runDiagnoseForUrl,
    renderOverviewTab,
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
