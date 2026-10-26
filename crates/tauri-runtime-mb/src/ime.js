// Offscreen mb132 exposes committed key presses, but no preedit API. Keep the
// composition in the focused editor so its font, color and layout come from CSS.
(function () {
  if (window.__mbIme) { return; }
  var state = null, last = '', mirror, decoration;
  var textStyles = [
    'boxSizing', 'fontFamily', 'fontSize', 'fontWeight', 'fontStyle',
    'fontVariant', 'fontStretch', 'lineHeight', 'letterSpacing', 'wordSpacing',
    'textTransform', 'textIndent', 'textAlign', 'direction', 'tabSize',
    'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft',
    'borderTopWidth', 'borderRightWidth', 'borderBottomWidth', 'borderLeftWidth'
  ];

  function editor() {
    var el = document.activeElement;
    if (!el || el.disabled || el.readOnly) { return null; }
    if (el.tagName === 'TEXTAREA' ||
        (el.tagName === 'INPUT' && el.selectionStart !== null)) { return el; }
    return el.isContentEditable ? el : null;
  }

  function layers() {
    if (mirror) { return; }
    var host = document.createElement('div');
    host.setAttribute('aria-hidden', 'true');
    host.style.cssText = 'all:initial!important;position:fixed!important;' +
      'left:0!important;top:0!important;pointer-events:none!important;' +
      'z-index:2147483647!important;';
    var root = host.attachShadow({ mode: 'closed' });
    mirror = document.createElement('div');
    decoration = document.createElement('div');
    root.appendChild(mirror);
    root.appendChild(decoration);
    document.documentElement.appendChild(host);
  }

  // A mirror is used only for geometry; the actual text stays in the editor.
  // The closed shadow tree isolates measurements/decorations from page styles.
  function measure(el, start, end) {
    layers();
    var css = getComputedStyle(el), rect = el.getBoundingClientRect();
    mirror.style.cssText = 'position:fixed;visibility:hidden;overflow:hidden;' +
      'border-style:solid;overflow-wrap:break-word;';
    textStyles.forEach(function (name) { mirror.style[name] = css[name]; });
    mirror.style.left = rect.left + 'px';
    mirror.style.top = rect.top + 'px';
    // Computed width/height preserve the editor's content-box/border-box sizing.
    mirror.style.width = css.width;
    mirror.style.height = css.height;
    mirror.style.whiteSpace = el.tagName === 'INPUT' ? 'pre' : 'pre-wrap';
    if (el.tagName === 'INPUT') {
      mirror.style.lineHeight = (el.clientHeight - parseFloat(css.paddingTop) -
        parseFloat(css.paddingBottom)) + 'px';
    }
    var value = el.type === 'password' ? '\u2022'.repeat(el.value.length) : el.value;
    mirror.textContent = value.slice(0, start);
    var span = document.createElement('span');
    span.textContent = value.slice(start, end);
    mirror.appendChild(span);
    var marker = document.createElement('span');
    marker.textContent = '\u200b';
    mirror.appendChild(marker);
    mirror.appendChild(document.createTextNode(value.slice(end)));
    mirror.scrollLeft = el.scrollLeft;
    mirror.scrollTop = el.scrollTop;
    return { caret: marker.getBoundingClientRect(), rects: span.getClientRects() };
  }

  function geometry(el) {
    if (el.selectionStart !== undefined && el.selectionStart !== null) {
      return measure(el, el.selectionStart, el.selectionStart).caret;
    }
    var selection = window.getSelection();
    if (!selection.rangeCount || !el.contains(selection.focusNode)) { return null; }
    var range = document.createRange();
    range.setStart(selection.focusNode, selection.focusOffset);
    range.collapse(true);
    var rect = range.getClientRects()[0];
    // An empty contenteditable has no text rectangle yet.
    return rect || el.getBoundingClientRect();
  }

  function decorate(el) {
    if (!decoration) { return; }
    decoration.textContent = '';
    if (!state || !state.text) { return; }
    var rects;
    if (state.input) {
      rects = measure(el, state.start, state.end).rects;
    } else if (state.node && state.node.isConnected) {
      var range = document.createRange();
      range.selectNodeContents(state.node);
      rects = range.getClientRects();
    } else { return; }
    var clip = el.getBoundingClientRect(), css = getComputedStyle(el);
    Array.prototype.forEach.call(rects, function (rect) {
      var left = Math.max(rect.left, clip.left + el.clientLeft);
      var right = Math.min(rect.right, clip.right - parseFloat(css.borderRightWidth));
      if (right <= left || rect.bottom > clip.bottom || rect.bottom < clip.top) { return; }
      var line = document.createElement('div');
      line.style.cssText = 'position:fixed;pointer-events:none;border-bottom:1px dotted;';
      line.style.left = left + 'px';
      line.style.top = (rect.bottom - 1) + 'px';
      line.style.width = (right - left) + 'px';
      line.style.color = css.color;
      decoration.appendChild(line);
    });
  }

  function report() {
    var el = editor(), rect = el && geometry(el);
    if (state && el === state.el) { decorate(el); }
    var payload = JSON.stringify({
      x: rect ? rect.left : -1, y: rect ? rect.top : -1,
      height: rect ? rect.height : 0, dpr: window.devicePixelRatio,
      viewport: window.innerWidth
    });
    if (payload === last) { return; }
    last = payload;
    fetch('http://mbcaret.localhost/report', { method: 'POST', body: payload })
      .catch(function () {});
  }

  function composition(name, text) {
    state.el.dispatchEvent(new CompositionEvent(name, { bubbles: true, data: text }));
  }

  function input(name, text, composing, cancelable) {
    return state.el.dispatchEvent(new InputEvent(name, {
      bubbles: true, cancelable: !!cancelable, composed: true,
      data: text, inputType: composing ? 'insertCompositionText' : 'insertText',
      isComposing: composing
    }));
  }

  function start() {
    if (state) { return; }
    var el = editor();
    if (!el) { return; }
    layers();
    state = { el: el, input: el.selectionStart !== undefined && el.selectionStart !== null,
      text: '', changed: false };
    if (state.input) {
      state.start = el.selectionStart;
      state.end = el.selectionEnd;
      state.original = el.value.slice(state.start, state.end);
      state.direction = el.selectionDirection;
    } else {
      var selection = window.getSelection();
      if (!selection.rangeCount || !el.contains(selection.anchorNode) ||
          !el.contains(selection.focusNode)) { state = null; return; }
      state.range = selection.getRangeAt(0).cloneRange();
    }
    composition('compositionstart', '');
    report();
  }

  function replace(text, cursor) {
    var el = state.el;
    if (state.input) {
      el.setRangeText(text, state.start, state.end, 'end');
      state.end = state.start + text.length;
      el.setSelectionRange(state.start + cursor, state.start + cursor);
    } else {
      if (!state.changed) {
        state.original = state.range.extractContents();
        state.node = document.createTextNode('');
        state.range.insertNode(state.node);
      }
      state.node.data = text;
      var range = document.createRange();
      range.setStart(state.node, cursor);
      range.collapse(true);
      var selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    }
    state.text = text;
    state.changed = true;
    // setRangeText/setSelectionRange do not scroll the native editor like a
    // keyboard edit does. Reveal the composing caret before reporting it.
    var caret = geometry(el), rect = el.getBoundingClientRect(), css = getComputedStyle(el);
    if (caret) {
      var left = rect.left + el.clientLeft + parseFloat(css.paddingLeft);
      var right = rect.left + el.clientLeft + el.clientWidth - parseFloat(css.paddingRight);
      if (caret.left < left) { el.scrollLeft += caret.left - left; }
      else if (caret.right >= right) { el.scrollLeft += caret.right - right + 1; }
      var top = rect.top + el.clientTop + parseFloat(css.paddingTop);
      var bottom = rect.top + el.clientTop + el.clientHeight - parseFloat(css.paddingBottom);
      if (caret.top < top) { el.scrollTop += caret.top - top; }
      else if (caret.bottom > bottom) { el.scrollTop += caret.bottom - bottom; }
    }
  }

  function finish(text) {
    if (!state) { start(); }
    if (!state) { return; }
    // Restore the selection first, then commit through the native editor so
    // committed text participates in its undo history (including rich text).
    state.ending = true;
    restore();
    composition('compositionend', text);
    var el = state.el;
    if (document.activeElement !== el || !el.isConnected) {
      state = null;
      decoration.textContent = '';
      report();
      return;
    }
    var committed = document.execCommand('insertText', false, text);
    if (!committed && input('beforeinput', text, false, true)) {
      replace(text, text.length);
      input('input', text, false);
    }
    state = null;
    decoration.textContent = '';
    report();
  }

  function restore() {
    if (!state.changed) { return; }
    if (state.input) {
      state.el.setRangeText(state.original, state.start, state.end, 'select');
      state.end = state.start + state.original.length;
      state.el.setSelectionRange(state.start, state.end, state.direction);
    } else if (state.node.isConnected) {
      var range = document.createRange();
      range.selectNode(state.node);
      var fragment = state.original;
      var first = fragment.firstChild, lastNode = fragment.lastChild;
      range.deleteContents();
      range.insertNode(fragment);
      if (first) { range.setStartBefore(first); range.setEndAfter(lastNode); }
      if (document.activeElement === state.el) {
        var selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
      }
      state.range = range.cloneRange();
    }
    state.changed = false;
    state.text = '';
  }

  function end() {
    if (!state || state.ending) { return; }
    state.ending = true;
    var changed = state.changed;
    restore();
    composition('compositionend', '');
    if (changed) { input('input', '', false); }
    state = null;
    decoration.textContent = '';
    report();
  }

  function update(message) {
    if (!state) { start(); }
    if (!state) { return; }
    if (document.activeElement !== state.el || !state.el.isConnected) { end(); return; }
    if (message.result !== null) { finish(message.result); }
    if (message.text !== null) {
      if (!state) { start(); }
      if (!state) { return; }
      var cursor = Math.max(0, Math.min(message.cursor, message.text.length));
      composition('compositionupdate', message.text);
      input('beforeinput', message.text, true);
      replace(message.text, cursor);
      input('input', message.text, true);
      report();
    }
  }

  ['focusin', 'focusout', 'keyup', 'mouseup', 'input', 'scroll', 'selectionchange'].forEach(function (name) {
    document.addEventListener(name, report, true);
  });
  document.addEventListener('focusout', function (event) {
    if (state && event.target === state.el) { end(); }
  }, true);
  window.addEventListener('resize', report);
  document.addEventListener('DOMContentLoaded', report);
  window.__mbIme = { start: start, update: update, end: end };
  report();
})();
