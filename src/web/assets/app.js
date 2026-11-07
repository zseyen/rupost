// Rupost Living Workbench 客户端脚本 (Vanilla ESM)

let currentFilePath = null;

document.addEventListener('DOMContentLoaded', () => {
  loadFiles();
  applyReadonlyState();
});

// 0. 只读态感知：展示徽章并置灰全部写路径按钮
async function applyReadonlyState() {
  try {
    const res = await fetch('/api/v1/meta');
    const json = await res.json();
    if (json.success && json.data && json.data.readonly) {
      const badge = document.getElementById('readonly-badge');
      if (badge) badge.style.display = '';
      ['btn-save', 'btn-run-selection', 'btn-import-modal'].forEach(id => {
        const btn = document.getElementById(id);
        if (btn) {
          btn.disabled = true;
          btn.style.opacity = '0.45';
          btn.style.cursor = 'not-allowed';
          btn.title = '只读模式下不可用';
        }
      });
    }
  } catch (e) {
    console.error('读取工作台元信息失败:', e);
  }
}

// 1. 加载文件列表
async function loadFiles() {
  const listEl = document.getElementById('file-list');
  try {
    const res = await fetch('/api/v1/files');
    const json = await res.json();
    if (json.success && json.data) {
      listEl.innerHTML = '';
      json.data.forEach(file => {
        const li = document.createElement('li');
        li.textContent = (file.is_dir ? '📁 ' : '📄 ') + file.name;
        li.dataset.path = file.path;
        if (!file.is_dir) {
          li.onclick = () => openFile(file.path);
        }
        listEl.appendChild(li);
      });
      // 默认打开第一个文件
      const firstDoc = json.data.find(f => !f.is_dir);
      if (firstDoc && !currentFilePath) {
        openFile(firstDoc.path);
      }
    }
  } catch (e) {
    console.error('加载文件失败:', e);
  }
}

// 2. 打开指定文档
async function openFile(path) {
  try {
    const res = await fetch(`/api/v1/file?path=${encodeURIComponent(path)}`);
    const json = await res.json();
    if (json.success && json.data) {
      currentFilePath = path;
      document.getElementById('active-file-label').textContent = path;
      const fileContent = typeof json.data === 'string' ? json.data : (json.data && json.data.content ? json.data.content : '');
      document.getElementById('code-editor').value = fileContent;
      document.querySelectorAll('#file-list li').forEach(li => {
        li.classList.toggle('active', li.dataset.path === path);
      });
    }
  } catch (e) {
    console.error('读取文档失败:', e);
  }
}

// 3. 保存文档（带后端语法守卫）
async function saveActiveFile() {
  if (!currentFilePath) {
    alert('请先选择一个文件');
    return;
  }
  const content = document.getElementById('code-editor').value;
  const statusEl = document.getElementById('save-status');
  statusEl.textContent = '正在保存...';
  statusEl.style.color = 'var(--text-muted)';

  try {
    const res = await fetch('/api/v1/save', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: currentFilePath, content })
    });
    const json = await res.json();
    if (res.ok && json.success) {
      statusEl.textContent = '✔ 已保存并校验通过';
      statusEl.style.color = 'var(--accent-pass)';
      setTimeout(() => { statusEl.textContent = ''; }, 3000);
    } else {
      statusEl.textContent = '❌ 保存拦截: ' + (json.error || '语法错误');
      statusEl.style.color = 'var(--accent-fail)';
      alert(`保存失败 (语法守卫拦截):\n${json.error}`);
    }
  } catch (e) {
    statusEl.textContent = '❌ 网络异常';
    statusEl.style.color = 'var(--accent-fail)';
  }
}

// 4. 原位单块调试发包
async function runCurrentBlock() {
  const editor = document.getElementById('code-editor');
  const fullText = editor.value;
  const cursorPos = editor.selectionStart;

  // 提取光标所在的代码块，若无光标则提取整个选区或首个代码块
  const rawBlock = extractTargetBlock(fullText, cursorPos);
  const debugEl = document.getElementById('debug-output');
  debugEl.innerHTML = '<div class="empty-hint">⚡ 正在执行请求...</div>';

  try {
    const res = await fetch('/api/v1/run/block', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        file_path: currentFilePath || 'inline.http',
        raw_block: rawBlock
      })
    });
    const json = await res.json();
    if (json.success && json.data) {
      renderDebugResult(json.data);
    } else {
      debugEl.innerHTML = `<div class="error-banner">❌ 执行异常: ${escapeHtml(json.error || '未知错误')}</div>`;
    }
  } catch (e) {
    debugEl.innerHTML = `<div class="error-banner">❌ 请求失败: ${escapeHtml(e.message)}</div>`;
  }
}

// 辅助：从光标位置提取最近的代码块
function extractTargetBlock(text, pos) {
  // 简易定位：若在 ``` 内部，提取该 block；否则返回当前选区或全文
  const before = text.slice(0, pos);
  const after = text.slice(pos);
  const lastBlockStart = before.lastIndexOf('```');
  const nextBlockEnd = after.indexOf('```');

  if (lastBlockStart !== -1 && nextBlockEnd !== -1) {
    return text.slice(lastBlockStart, pos + nextBlockEnd + 3);
  }
  return text; // 默认全文作为单块尝试执行
}

// 5. 渲染调试结果 (状态码、时延瀑布条、断言比对)
function renderDebugResult(data) {
  const debugEl = document.getElementById('debug-output');
  let html = '';

  const statusClass = data.success ? 'pill-ok' : 'pill-err';
  html += `
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px;">
      <span class="status-pill ${statusClass}">${data.status || 'ERR'}</span>
      <span style="font-family: var(--font-mono); color: var(--text-muted);">${data.duration_ms} ms</span>
    </div>
  `;

  // 时延瀑布条
  if (data.timing_waterfall && data.timing_waterfall.total_ms > 0) {
    const wf = data.timing_waterfall;
    const dnsPct = (wf.dns_ms / wf.total_ms) * 100;
    const tcpPct = (wf.tcp_ms / wf.total_ms) * 100;
    const ttfbPct = wf.ttfb_ms ? (wf.ttfb_ms / wf.total_ms) * 100 : 0;
    const transPct = (wf.transfer_ms / wf.total_ms) * 100;

    html += `
      <div style="background: var(--bg-card); padding: 10px; border-radius: 6px; margin-bottom: 12px; font-size: 11px;">
        <div style="margin-bottom: 6px; font-weight: 600; color: var(--text-muted);">网络耗时瀑布图 (总计: ${wf.total_ms.toFixed(1)} ms)</div>
        <div style="display: flex; height: 8px; border-radius: 4px; overflow: hidden; background: hsl(220,16%,20%);">
          ${dnsPct > 0 ? `<div style="width: ${dnsPct}%; background: #60a5fa;" title="DNS: ${wf.dns_ms.toFixed(1)}ms"></div>` : ''}
          ${tcpPct > 0 ? `<div style="width: ${tcpPct}%; background: #2dd4bf;" title="TCP: ${wf.tcp_ms.toFixed(1)}ms"></div>` : ''}
          ${ttfbPct > 0 ? `<div style="width: ${ttfbPct}%; background: #f59e0b;" title="TTFB: ${wf.ttfb_ms.toFixed(1)}ms"></div>` : ''}
          ${transPct > 0 ? `<div style="width: ${transPct}%; background: #ec4899;" title="Transfer: ${wf.transfer_ms.toFixed(1)}ms"></div>` : ''}
        </div>
      </div>
    `;
  }

  // 断言列表
  if (data.assertions && data.assertions.length > 0) {
    html += '<div style="margin-bottom: 12px;"><div style="font-size: 12px; font-weight: 600; margin-bottom: 6px;">断言评估结果:</div><ul style="list-style: none; display: flex; flex-direction: column; gap: 4px;">';
    data.assertions.forEach(a => {
      const icon = a.passed ? '✓' : '✗';
      const color = a.passed ? 'var(--accent-pass)' : 'var(--accent-fail)';
      html += `<li style="font-family: var(--font-mono); font-size: 12px; color: ${color};">${icon} ${escapeHtml(a.raw)}</li>`;
    });
    html += '</ul></div>';
  }

  // 响应体
  if (data.response_body) {
    html += `
      <div>
        <div style="font-size: 12px; font-weight: 600; margin-bottom: 6px;">响应体:</div>
        <pre style="background: hsl(220,20%,8%); padding: 10px; border-radius: 6px; font-family: var(--font-mono); font-size: 12px; max-height: 300px; overflow: auto;"><code>${escapeHtml(data.response_body)}</code></pre>
      </div>
    `;
  }

  debugEl.innerHTML = html;
}

// 6. 多格式向导装入弹窗
function openImportModal() {
  document.getElementById('import-modal').style.display = 'flex';
  document.getElementById('import-textarea').focus();
}

function closeImportModal() {
  document.getElementById('import-modal').style.display = 'none';
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
      alert('装入解析失败: ' + (json.error || '格式不符'));
    }
  } catch (e) {
    alert('请求异常: ' + e.message);
  }
}

function insertIntoEditor(text) {
  const editor = document.getElementById('code-editor');
  const start = editor.selectionStart;
  const end = editor.selectionEnd;
  const current = editor.value;
  editor.value = current.substring(0, start) + '\n\n' + text + '\n\n' + current.substring(end);
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
