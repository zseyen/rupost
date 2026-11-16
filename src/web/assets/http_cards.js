// Rupost Living Workbench - Pure HTTP Card Parser, Rebuilder & Syntax Highlighter (No Emoji)

(function (root, factory) {
  const api = factory();
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  }
  if (root) {
    root.RupostCards = api;
  }
})(typeof window !== 'undefined' ? window : globalThis, function () {
  const MAX_HIGHLIGHT_BYTES = 100 * 1024;
  const HTTP_METHODS = new Set([
    'GET', 'POST', 'PUT', 'DELETE', 'PATCH', 'HEAD', 'OPTIONS', 'TRACE', 'CONNECT',
  ]);

  function escapeHtml(str) {
    if (str === null || str === undefined) return '';
    return String(str)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  function highlightTemplateVars(rawText) {
    const escaped = escapeHtml(rawText);
    return escaped.replace(/\{\{([^}]+)\}\}/g, '<span class="tok-var">{{$1}}</span>');
  }

  /**
   * Token-first JSON syntax highlighter:
   * - Splits JSON tokens before escaping each token individually (XSS-safe).
   * - Falls back to escaped plain text if payload > 100KB or invalid JSON.
   */
  function highlightJson(text) {
    if (!text) return '';
    const str = String(text);
    if (str.length > MAX_HIGHLIGHT_BYTES) {
      return escapeHtml(str);
    }
    try {
      JSON.parse(str);
    } catch (e) {
      return escapeHtml(str);
    }

    const tokenRe = /("(?:\\u[a-fA-F0-9]{4}|\\[^u]|[^\\"])*"(\s*:)?|\b(?:true|false|null)\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?|[{}\[\],:])/g;
    let out = '';
    let lastIndex = 0;
    let match;

    while ((match = tokenRe.exec(str)) !== null) {
      if (match.index > lastIndex) {
        out += escapeHtml(str.slice(lastIndex, match.index));
      }
      const token = match[0];
      let cls = 'tok-punct';
      if (token.startsWith('"')) {
        if (match[2]) {
          // Key + trailing ":"
          const colonIdx = token.lastIndexOf(':');
          const keyPart = token.slice(0, colonIdx);
          const restPart = token.slice(colonIdx);
          out += `<span class="tok-json-key">${escapeHtml(keyPart)}</span><span class="tok-punct">${escapeHtml(restPart)}</span>`;
          lastIndex = tokenRe.lastIndex;
          continue;
        }
        cls = 'tok-json-str';
      } else if (token === 'true' || token === 'false') {
        cls = 'tok-json-bool';
      } else if (token === 'null') {
        cls = 'tok-json-null';
      } else if (/^-?\d/.test(token)) {
        cls = 'tok-json-num';
      }
      out += `<span class="${cls}">${escapeHtml(token)}</span>`;
      lastIndex = tokenRe.lastIndex;
    }

    if (lastIndex < str.length) {
      out += escapeHtml(str.slice(lastIndex));
    }
    return out;
  }

  function isMockVariantMarker(line) {
    const trimmed = line.trim();
    const cleaned = trimmed.replace(/^(?:#|\/\/)\s*/, '').trim();
    return cleaned.startsWith('@mock-when') || cleaned.startsWith('@mock-default');
  }

  /**
   * Split rawBlock into request part and mock variants part, matching
   * Rust MockVariantParser::split_mock_variants (blank line before @mock-when/@mock-default).
   */
  function splitMockVariants(rawBlock) {
    const lines = String(rawBlock || '').replace(/\r\n/g, '\n').split('\n');
    let prevBlank = false;
    let splitIdx = -1;

    for (let i = 0; i < lines.length; i++) {
      const trimmed = lines[i].trim();
      if (prevBlank && isMockVariantMarker(trimmed)) {
        splitIdx = i;
        break;
      }
      prevBlank = trimmed === '';
    }

    if (splitIdx === -1) {
      return { requestPart: lines.join('\n'), variantsPart: '' };
    }
    return {
      requestPart: lines.slice(0, splitIdx).join('\n').replace(/\n+$/, ''),
      variantsPart: lines.slice(splitIdx).join('\n'),
    };
  }

  /**
   * Extract top-level @key = value declarations and ### request blocks from a .http file.
   */
  function extractHttpFileBlocks(content) {
    const lines = String(content || '').replace(/\r\n/g, '\n').split('\n');
    const fileVariables = {};
    const blocks = [];
    let current = [];
    let seenRequestOrSep = false;

    for (const line of lines) {
      const trimmed = line.trim();
      if (trimmed.startsWith('###')) {
        seenRequestOrSep = true;
        if (current.some((l) => l.trim() !== '')) {
          blocks.push(current.join('\n').trim());
        }
        current = [line];
        continue;
      }

      if (!seenRequestOrSep && !trimmed.startsWith('#') && !trimmed.startsWith('//')) {
        const varMatch = trimmed.match(/^@([A-Za-z0-9_.-]+)\s*=\s*(.+)$/);
        if (varMatch) {
          fileVariables[varMatch[1]] = varMatch[2].trim();
          continue;
        }
        if (isRequestLine(trimmed)) {
          seenRequestOrSep = true;
        }
      }

      current.push(line);
    }

    if (current.some((l) => l.trim() !== '')) {
      blocks.push(current.join('\n').trim());
    }

    return { fileVariables, blocks };
  }

  function isRequestLine(trimmedLine) {
    if (!trimmedLine) return false;
    const firstToken = trimmedLine.split(/\s+/)[0].toUpperCase();
    if (HTTP_METHODS.has(firstToken)) return true;
    return /^(https?:\/\/|ws:\/\/|wss:\/\/|\/)/i.test(trimmedLine);
  }

  function parseHttpBlock(rawBlock) {
    const { requestPart, variantsPart } = splitMockVariants(rawBlock);
    const lines = requestPart.split('\n');

    let title = '';
    const directives = [];
    let method = 'GET';
    let url = '';
    const headers = [];
    const bodyLines = [];
    let hasRequestLine = false;
    let inBody = false;
    let isWebsocket = false;

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const trimmed = line.trim();

      if (!inBody) {
        if (trimmed.startsWith('###')) {
          if (!title) {
            title = trimmed.replace(/^###\s*/, '').trim();
          }
          continue;
        }

        if (!hasRequestLine) {
          if (!trimmed) continue;

          if (trimmed.startsWith('#') || trimmed.startsWith('//')) {
            const inner = trimmed.replace(/^(?:#|\/\/)\s*/, '').trim();
            if (inner.startsWith('@')) {
              directives.push(inner);
              if (inner.startsWith('@name ')) {
                title = title || inner.slice(6).trim();
              }
              if (inner.startsWith('@websocket')) {
                isWebsocket = true;
              }
            } else if (!title && inner) {
              title = inner;
            }
            continue;
          }

          if (trimmed.startsWith('@')) {
            directives.push(trimmed);
            if (trimmed.startsWith('@websocket')) isWebsocket = true;
            if (trimmed.startsWith('@name ')) {
              title = title || trimmed.slice(6).trim();
            }
            continue;
          }

          if (isRequestLine(trimmed)) {
            hasRequestLine = true;
            const parts = trimmed.split(/\s+/);
            if (HTTP_METHODS.has(parts[0].toUpperCase())) {
              method = parts[0].toUpperCase();
              url = parts.slice(1).join(' ');
            } else {
              method = isWebsocket ? 'WS' : 'GET';
              url = trimmed;
            }
            continue;
          }
        } else {
          if (trimmed === '') {
            inBody = true;
            continue;
          }
          if (trimmed.startsWith('@')) {
            directives.push(trimmed);
            continue;
          }
          if (trimmed.startsWith('#') || trimmed.startsWith('//')) {
            continue;
          }
          const colonIdx = line.indexOf(':');
          if (colonIdx > 0) {
            headers.push({
              key: line.slice(0, colonIdx).trim(),
              value: line.slice(colonIdx + 1).trim(),
            });
          } else {
            // Non-header line right after request line -> treat as body start
            inBody = true;
            bodyLines.push(line);
          }
        }
      } else {
        bodyLines.push(line);
      }
    }

    const body = bodyLines.join('\n').replace(/^\n+|\n+$/g, '');
    const hasMock = Boolean(variantsPart.trim()) || directives.some((d) => d.startsWith('@mock'));

    return {
      title: title || (url ? `${method} ${url}` : 'HTTP Request'),
      directives,
      method,
      url,
      headers,
      body,
      variantsText: variantsPart,
      hasMock,
      isWebsocket,
    };
  }

  /**
   * Rebuild a raw request block in memory with an edited body,
   * preserving headers, directives, and any @mock-when/@mock-default variants.
   */
  function rebuildBlockWithBody(rawBlock, newBody) {
    const { requestPart, variantsPart } = splitMockVariants(rawBlock);
    const lines = requestPart.split('\n');

    let headerEndIdx = lines.length;
    let seenRequestLine = false;
    for (let i = 0; i < lines.length; i++) {
      const trimmed = lines[i].trim();
      if (!seenRequestLine) {
        if (isRequestLine(trimmed)) {
          seenRequestLine = true;
        }
      } else if (trimmed === '') {
        headerEndIdx = i;
        break;
      }
    }

    const headPart = lines.slice(0, headerEndIdx).join('\n').replace(/\n+$/, '');
    const cleanBody = String(newBody || '').replace(/^\n+|\n+$/g, '');

    let rebuilt = headPart;
    if (cleanBody) {
      rebuilt += `\n\n${cleanBody}`;
    }
    if (variantsPart.trim()) {
      rebuilt += `\n\n${variantsPart.trim()}`;
    }
    return rebuilt;
  }

  function methodBadgeClass(method) {
    const m = String(method || 'GET').toUpperCase();
    if (m === 'POST') return 'method-post';
    if (m === 'PUT' || m === 'PATCH') return 'method-warn';
    if (m === 'DELETE') return 'method-delete';
    if (m === 'WS') return 'method-ws';
    return 'method-get';
  }

  function renderHttpCardHtml(cardOrRaw, idx, options = {}) {
    const { readonly = false, editedBody = undefined } = options;
    const isDto = cardOrRaw && typeof cardOrRaw === 'object';
    const parsed = isDto
      ? {
          title: cardOrRaw.title || 'HTTP Request',
          directives: Array.isArray(cardOrRaw.directives) ? cardOrRaw.directives : [],
          method: cardOrRaw.method || 'GET',
          url: cardOrRaw.url || '',
          headers: Array.isArray(cardOrRaw.headers) ? cardOrRaw.headers : [],
          body: cardOrRaw.body || '',
          hasMock: Boolean(cardOrRaw.has_mock),
          isWebsocket: Boolean(cardOrRaw.is_websocket),
        }
      : parseHttpBlock(cardOrRaw);
    const activeBody = editedBody !== undefined ? editedBody : parsed.body;
    const isModified = editedBody !== undefined && editedBody !== parsed.body;
    const hasInitialOrEditedBody = Boolean(activeBody);

    const directivesHtml = parsed.directives
      .map((d) => `<span class="http-directive-chip">${highlightTemplateVars(d)}</span>`)
      .join('');

    const headersHtml = parsed.headers.length
      ? `<div class="http-card-headers">${parsed.headers
          .map(
            (h) =>
              `<div class="http-header-row"><span class="http-header-key">${escapeHtml(h.key)}</span><span class="http-header-val">${highlightTemplateVars(h.value)}</span></div>`
          )
          .join('')}</div>`
      : '';

    const mockBtnHtml = parsed.hasMock
      ? `<button type="button" class="btn btn-secondary btn-sm http-card-mock-btn" data-card-index="${idx}">[Mock]</button>`
      : '';

    const bodySectionHtml = parsed.isWebsocket
      ? `<div class="http-card-body-wrap">
           <div class="http-card-section-label">WebSocket Script (Read-only Preview)</div>
           <pre class="http-card-script-pre"><code>${highlightTemplateVars(parsed.body || '')}</code></pre>
         </div>`
      : `<div class="http-card-body-wrap">
           <div class="http-card-body-toolbar">
             <span class="http-card-section-label">Request Body ${isModified ? '<span class="modified-pill">[Modified]</span>' : ''}</span>
             <div class="http-card-body-actions">
               ${!hasInitialOrEditedBody ? `<button type="button" class="btn btn-secondary btn-sm http-card-add-body-btn" data-card-index="${idx}" ${readonly ? 'disabled' : ''}>+ Body</button>` : ''}
               <button type="button" class="btn btn-secondary btn-sm http-card-fmt-btn" data-card-index="${idx}" style="${hasInitialOrEditedBody ? '' : 'display:none;'}" ${readonly ? 'disabled' : ''}>Format JSON</button>
               <button type="button" class="btn btn-secondary btn-sm http-card-reset-btn" data-card-index="${idx}" style="${isModified ? '' : 'display:none;'}" ${readonly ? 'disabled' : ''}>Reset</button>
             </div>
           </div>
           <textarea class="http-card-body-editor" data-card-index="${idx}" spellcheck="false" placeholder="Optional request body (JSON / text)..." style="${hasInitialOrEditedBody ? '' : 'display:none;'}" ${readonly ? 'readonly' : ''}>${escapeHtml(activeBody)}</textarea>
         </div>`;

    return `
      <section class="http-card" data-card-index="${idx}">
        <header class="http-card-header">
          <span class="http-card-title">${escapeHtml(parsed.title)}</span>
          <div class="http-card-actions">
            ${mockBtnHtml}
            <button type="button" class="btn btn-secondary btn-sm http-card-export-btn" data-card-index="${idx}">Export</button>
            <button type="button" class="btn btn-accent btn-sm http-card-run-btn" data-card-index="${idx}" ${readonly ? 'disabled' : ''}>&gt; Run</button>
          </div>
        </header>
        <div class="http-card-request-line">
          <span class="method-badge ${methodBadgeClass(parsed.method)}">${escapeHtml(parsed.method)}</span>
          <span class="http-card-url">${highlightTemplateVars(parsed.url || '(no url)')}</span>
        </div>
        ${directivesHtml ? `<div class="http-card-directives">${directivesHtml}</div>` : ''}
        ${headersHtml}
        ${bodySectionHtml}
      </section>
    `;
  }

  function formatDurationObj(dur) {
    if (dur === null || dur === undefined) return '-';
    if (typeof dur === 'number') return `${dur.toFixed(1)} ms`;
    const secs = Number(dur.secs || 0);
    const nanos = Number(dur.nanos || 0);
    const ms = secs * 1000 + nanos / 1e6;
    return `${ms.toFixed(1)} ms`;
  }

  return {
    escapeHtml,
    highlightTemplateVars,
    highlightJson,
    splitMockVariants,
    extractHttpFileBlocks,
    parseHttpBlock,
    rebuildBlockWithBody,
    renderHttpCardHtml,
    formatDurationObj,
    MAX_HIGHLIGHT_BYTES,
  };
});
